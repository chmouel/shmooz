use std::time::{Duration, Instant};

use wayland_client::{
    Dispatch, QueueHandle, WEnum,
    protocol::{wl_keyboard, wl_pointer, wl_seat, wl_surface},
};

use crate::{
    overlay,
    state::{ActiveAnnotation, AnnotationPoint, AnnotationTool, AppState, InteractionMode},
    window,
    zoom::{Point, Size, apply_zoom, restore_view, screen_center},
};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

const KEY_ESC: u32 = 1;
const KEY_0: u32 = 11;
const KEY_MINUS: u32 = 12;
const KEY_EQUAL: u32 = 13;
const KEY_W: u32 = 17;
const KEY_E: u32 = 18;
const KEY_R: u32 = 19;
const KEY_U: u32 = 22;
const KEY_P: u32 = 25;
const KEY_LEFTBRACE: u32 = 26;
const KEY_RIGHTBRACE: u32 = 27;
const KEY_S: u32 = 31;
const KEY_D: u32 = 32;
const KEY_H: u32 = 35;
const KEY_L: u32 = 38;
const KEY_C: u32 = 46;
const KEY_KPMINUS: u32 = 74;
const KEY_KPPLUS: u32 = 78;
const KEY_KP0: u32 = 82;
const KEY_UP: u32 = 103;
const KEY_LEFT: u32 = 105;
const KEY_RIGHT: u32 = 106;
const KEY_DOWN: u32 = 108;

const DOUBLE_CLICK_TIME_MS: u32 = 400;
const KEYBOARD_PAN_STEP: f64 = 50.0;
const KEYBOARD_ZOOM_STEP: f64 = 10.0;
const KEY_REPEAT_DELAY: Duration = Duration::from_millis(500);

pub fn repeat_timer_tick(state: &mut AppState) {
    let (Some(key), Some(deadline)) = (state.repeat_key, state.repeat_deadline) else {
        return;
    };

    if Instant::now() < deadline {
        return;
    }

    handle_key_action(state, key);
    state.repeat_deadline = Some(Instant::now() + state.repeat_interval);
}

impl Dispatch<wl_seat::WlSeat, ()> for AppState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &wayland_client::Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            if capabilities.contains(wl_seat::Capability::Pointer)
                && state.globals.pointer.is_none()
            {
                state.globals.pointer = Some(seat.get_pointer(qh, ()));
            } else if !capabilities.contains(wl_seat::Capability::Pointer) {
                state.globals.pointer = None;
            }

            if capabilities.contains(wl_seat::Capability::Keyboard)
                && state.globals.keyboard.is_none()
            {
                state.globals.keyboard = Some(seat.get_keyboard(qh, ()));
            } else if !capabilities.contains(wl_seat::Capability::Keyboard) {
                state.globals.keyboard = None;
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                ..
            } => focus_surface(state, &surface, surface_x, surface_y),
            wl_pointer::Event::Leave { surface, .. } => unfocus_surface(state, &surface),
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => pointer_motion(state, surface_x, surface_y),
            wl_pointer::Event::Button {
                time,
                button,
                state: WEnum::Value(button_state),
                ..
            } => pointer_button(state, time, button, button_state),
            wl_pointer::Event::Axis {
                axis: WEnum::Value(wl_pointer::Axis::VerticalScroll),
                value,
                ..
            } => pointer_axis(state, value),
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &wayland_client::Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Enter { .. } | wl_keyboard::Event::Leave { .. } => {}
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(key_state),
                ..
            } => handle_key_event(state, key, key_state),
            wl_keyboard::Event::Keymap { .. }
            | wl_keyboard::Event::Modifiers { .. }
            | wl_keyboard::Event::RepeatInfo { .. }
            | _ => {}
        }
    }
}

fn focus_surface(state: &mut AppState, surface: &wl_surface::WlSurface, x: f64, y: f64) {
    for (output_id, window) in &mut state.windows {
        if &window.surface == surface {
            state.focused_window = Some(*output_id);
            window.pointer_x = x;
            window.pointer_y = y;
            return;
        }
    }
}

fn unfocus_surface(_: &mut AppState, _: &wl_surface::WlSurface) {
    // Keep the last focused window on pointer leave for parity with wooz:
    // overlay commits can trigger leave/enter churn and clearing focus here
    // breaks pan/keyboard actions until a fresh enter arrives.
}

fn pointer_motion(state: &mut AppState, x: f64, y: f64) {
    let Some(output_id) = active_window_id(state) else {
        return;
    };

    let (prev_x, prev_y, pointer_pressed) = match state.windows.get(&output_id) {
        Some(window) => (window.pointer_x, window.pointer_y, window.pointer_pressed),
        None => return,
    };
    let delta_x = x - prev_x;
    let delta_y = y - prev_y;

    if state.interaction_mode.is_annotating() {
        if let Some(window) = state.windows.get_mut(&output_id) {
            window.pointer_x = x;
            window.pointer_y = y;
        }
        if pointer_pressed {
            update_active_annotation(state, output_id, x, y);
        } else {
            overlay::refresh_annotation_overlay(state, output_id);
        }
        return;
    }

    if pointer_pressed {
        let scale = view_scale(state, output_id);
        if let Some(window) = state.windows.get_mut(&output_id) {
            window.view_source.x -= delta_x * scale;
            window.view_source.y -= delta_y * scale;
        }
        window::render_window(state, output_id);
    } else if state.config.mouse_track {
        if let Some(window) = state.windows.get_mut(&output_id) {
            window.view_source.x += delta_x;
            window.view_source.y += delta_y;
        }
        window::render_window(state, output_id);
    }

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.pointer_x = x;
        window.pointer_y = y;
    }

    overlay::refresh_spotlight_for_motion(state, output_id, delta_x, delta_y);
}

fn pointer_button(
    state: &mut AppState,
    time: u32,
    button: u32,
    button_state: wl_pointer::ButtonState,
) {
    let Some(output_id) = active_window_id(state) else {
        return;
    };

    if state.interaction_mode.is_annotating() {
        match button {
            BTN_LEFT if button_state == wl_pointer::ButtonState::Pressed => {
                start_annotation(state, output_id);
            }
            BTN_LEFT if button_state == wl_pointer::ButtonState::Released => {
                finish_annotation(state, output_id);
            }
            BTN_RIGHT if button_state == wl_pointer::ButtonState::Released => {
                state.request_exit();
            }
            _ => {}
        }
        return;
    }

    match button {
        BTN_LEFT => {
            if button_state == wl_pointer::ButtonState::Pressed {
                let is_double_click = state
                    .windows
                    .get(&output_id)
                    .map(|window| {
                        window.last_click_button == BTN_LEFT
                            && time.saturating_sub(window.last_click_time) < DOUBLE_CLICK_TIME_MS
                    })
                    .unwrap_or(false);

                if let Some(window) = state.windows.get_mut(&output_id) {
                    if is_double_click {
                        restore_view(&mut window.view_source, window.initial_view_source);
                        window.last_click_time = 0;
                    } else {
                        window.last_click_time = time;
                        window.last_click_button = button;
                    }
                    window.pointer_pressed = true;
                }

                if is_double_click {
                    window::render_window(state, output_id);
                }
            } else if let Some(window) = state.windows.get_mut(&output_id) {
                window.pointer_pressed = false;
            }
        }
        BTN_RIGHT if button_state == wl_pointer::ButtonState::Released => {
            state.request_exit();
        }
        _ => {}
    }
}

fn pointer_axis(state: &mut AppState, value: f64) {
    if state.interaction_mode.is_annotating() {
        return;
    }

    let Some(output_id) = active_window_id(state) else {
        return;
    };

    let scale = scroll_scale(state, output_id);
    let mut scroll = value * scale * 10.0;
    if state.config.invert_scroll {
        scroll = -scroll;
    }

    zoom_focused_window_at_pointer(state, output_id, scroll);
}

fn handle_key_event(state: &mut AppState, key: u32, key_state: wl_keyboard::KeyState) {
    if key_state == wl_keyboard::KeyState::Released {
        if state.repeat_key == Some(key) {
            state.stop_repeat();
        }
        return;
    }

    if key == KEY_ESC
        && state.interaction_mode.is_annotating()
        && !state
            .config
            .close_key
            .is_some_and(|close_key| close_key.key_code() == KEY_ESC)
    {
        set_interaction_mode(state, InteractionMode::Navigate);
        return;
    }

    if state
        .config
        .close_key
        .is_some_and(|close_key| close_key.key_code() == key)
    {
        state.request_exit();
        return;
    }

    match key {
        KEY_ESC if state.config.close_key.is_none() => state.request_exit(),
        KEY_D => toggle_annotation_mode(state, InteractionMode::AnnotateZoomed),
        KEY_W => toggle_annotation_mode(state, InteractionMode::AnnotateUnzoomed),
        KEY_P => select_annotation_tool(state, AnnotationTool::Pen),
        KEY_H => select_annotation_tool(state, AnnotationTool::Highlighter),
        KEY_L => select_annotation_tool(state, AnnotationTool::Line),
        KEY_R => select_annotation_tool(state, AnnotationTool::Rectangle),
        KEY_E => select_annotation_tool(state, AnnotationTool::Ellipse),
        KEY_U => undo_annotation(state),
        KEY_C => clear_annotations(state),
        KEY_0 | KEY_KP0 if !state.interaction_mode.is_annotating() => restore_focused_window(state),
        KEY_S => {
            state.spotlight_enabled = !state.spotlight_enabled;
            overlay::update_spotlight_overlays(state);
        }
        _ => {
            handle_key_action(state, key);
            if !state.interaction_mode.is_annotating() && is_repeatable_key(key) {
                state.repeat_key = Some(key);
                state.repeat_deadline = Some(Instant::now() + KEY_REPEAT_DELAY);
            }
        }
    }
}

fn handle_key_action(state: &mut AppState, key: u32) {
    let Some(output_id) = state.focused_window else {
        return;
    };

    if state.interaction_mode.is_annotating() {
        match key {
            KEY_LEFTBRACE => adjust_spotlight_radius(state, -0.05),
            KEY_RIGHTBRACE => adjust_spotlight_radius(state, 0.05),
            _ => {}
        }
        return;
    }

    match key {
        KEY_EQUAL | KEY_KPPLUS => {
            zoom_focused_window_at_center(state, output_id, KEYBOARD_ZOOM_STEP);
        }
        KEY_MINUS | KEY_KPMINUS => {
            zoom_focused_window_at_center(state, output_id, -KEYBOARD_ZOOM_STEP);
        }
        KEY_LEFT => pan_focused_window(state, output_id, -KEYBOARD_PAN_STEP, 0.0),
        KEY_RIGHT => pan_focused_window(state, output_id, KEYBOARD_PAN_STEP, 0.0),
        KEY_UP => pan_focused_window(state, output_id, 0.0, -KEYBOARD_PAN_STEP),
        KEY_DOWN => pan_focused_window(state, output_id, 0.0, KEYBOARD_PAN_STEP),
        KEY_LEFTBRACE => adjust_spotlight_radius(state, -0.05),
        KEY_RIGHTBRACE => adjust_spotlight_radius(state, 0.05),
        _ => {}
    }
}

fn toggle_annotation_mode(state: &mut AppState, mode: InteractionMode) {
    let next_mode = if state.interaction_mode == mode {
        InteractionMode::Navigate
    } else {
        mode
    };
    set_interaction_mode(state, next_mode);
}

fn set_interaction_mode(state: &mut AppState, mode: InteractionMode) {
    if mode == InteractionMode::AnnotateUnzoomed {
        if let Some(output_id) = active_window_id(state) {
            if let Some(window) = state.windows.get_mut(&output_id) {
                restore_view(&mut window.view_source, window.initial_view_source);
            }
            window::render_window(state, output_id);
        }
    }

    cancel_active_annotations(state);
    state.interaction_mode = mode;
    state.stop_repeat();
    overlay::update_spotlight_overlays(state);
    overlay::refresh_zoom_badge_overlays(state);
    overlay::update_annotation_overlays(state);
}

fn select_annotation_tool(state: &mut AppState, tool: AnnotationTool) {
    state.annotation_tool = tool;
    overlay::refresh_zoom_badge_overlays(state);
    overlay::update_annotation_overlays(state);
}

fn start_annotation(state: &mut AppState, output_id: u32) {
    let point = state
        .windows
        .get(&output_id)
        .map(|window| AnnotationPoint::new(window.pointer_x, window.pointer_y));

    let Some(point) = point else {
        return;
    };

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.pointer_pressed = true;
        window.active_annotation = Some(ActiveAnnotation::new(state.annotation_tool, point));
    }
    overlay::update_annotation_overlays(state);
}

fn update_active_annotation(state: &mut AppState, output_id: u32, x: f64, y: f64) {
    let point = AnnotationPoint::new(x, y);
    if let Some(window) = state.windows.get_mut(&output_id) {
        if let Some(active_annotation) = window.active_annotation.as_mut() {
            active_annotation.update(point);
        }
    }
    overlay::refresh_annotation_overlay(state, output_id);
}

fn finish_annotation(state: &mut AppState, output_id: u32) {
    let completed = state.windows.get_mut(&output_id).and_then(|window| {
        window.pointer_pressed = false;
        window
            .active_annotation
            .take()
            .and_then(ActiveAnnotation::finish)
    });

    if let Some(annotation) = completed {
        if let Some(window) = state.windows.get_mut(&output_id) {
            window.annotations.push(annotation);
        }
    }

    overlay::update_annotation_overlays(state);
}

fn undo_annotation(state: &mut AppState) {
    let Some(output_id) = active_window_id(state) else {
        return;
    };

    if let Some(window) = state.windows.get_mut(&output_id) {
        if window.active_annotation.take().is_none() {
            window.annotations.pop();
        }
        window.pointer_pressed = false;
    }
    overlay::update_annotation_overlays(state);
}

fn clear_annotations(state: &mut AppState) {
    let Some(output_id) = active_window_id(state) else {
        return;
    };

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.annotations.clear();
        window.active_annotation = None;
        window.pointer_pressed = false;
    }
    overlay::update_annotation_overlays(state);
}

fn cancel_active_annotations(state: &mut AppState) {
    let output_ids = state.windows.keys().copied().collect::<Vec<_>>();
    for output_id in output_ids {
        if let Some(window) = state.windows.get_mut(&output_id) {
            window.active_annotation = None;
            window.pointer_pressed = false;
        }
        overlay::update_window_overlays(state, output_id);
    }
}

fn restore_focused_window(state: &mut AppState) {
    let Some(output_id) = state.focused_window else {
        return;
    };

    if let Some(window) = state.windows.get_mut(&output_id) {
        restore_view(&mut window.view_source, window.initial_view_source);
    }
    window::render_window(state, output_id);
}

fn pan_focused_window(state: &mut AppState, output_id: u32, dx: f64, dy: f64) {
    if let Some(window) = state.windows.get_mut(&output_id) {
        window.view_source.x += dx;
        window.view_source.y += dy;
    }
    window::render_window(state, output_id);
}

fn active_window_id(state: &AppState) -> Option<u32> {
    state.focused_window.or_else(|| {
        if state.windows.len() == 1 {
            state.windows.keys().next().copied()
        } else {
            None
        }
    })
}

fn zoom_focused_window_at_center(state: &mut AppState, output_id: u32, zoom_change: f64) {
    let logical_size = logical_size(state, output_id);
    let center = screen_center(logical_size);
    zoom_focused_window(state, output_id, zoom_change, center);
}

fn zoom_focused_window_at_pointer(state: &mut AppState, output_id: u32, zoom_change: f64) {
    let logical_size = logical_size(state, output_id);
    let center = state
        .windows
        .get(&output_id)
        .map(|window| Point {
            x: window.pointer_x,
            y: window.pointer_y,
        })
        .unwrap_or_else(|| screen_center(logical_size));
    zoom_focused_window(state, output_id, zoom_change, center);
}

fn zoom_focused_window(state: &mut AppState, output_id: u32, zoom_change: f64, center: Point) {
    let logical_size = logical_size(state, output_id);
    let buffer_size = buffer_size(state, output_id);

    if let Some(window) = state.windows.get_mut(&output_id) {
        apply_zoom(
            &mut window.view_source,
            zoom_change,
            center,
            logical_size,
            buffer_size,
        );
    }
    window::render_window(state, output_id);
}

fn adjust_spotlight_radius(state: &mut AppState, delta: f64) {
    let previous = state.spotlight_radius_frac;
    state.spotlight_radius_frac = (state.spotlight_radius_frac + delta).clamp(0.05, 0.90);
    if (state.spotlight_radius_frac - previous).abs() > f64::EPSILON {
        overlay::refresh_visible_spotlight_overlays(state);
    }
}

fn is_repeatable_key(key: u32) -> bool {
    matches!(
        key,
        KEY_EQUAL
            | KEY_KPPLUS
            | KEY_MINUS
            | KEY_KPMINUS
            | KEY_LEFT
            | KEY_RIGHT
            | KEY_UP
            | KEY_DOWN
            | KEY_LEFTBRACE
            | KEY_RIGHTBRACE
    )
}

fn logical_size(state: &AppState, output_id: u32) -> Size {
    state
        .outputs
        .get(&output_id)
        .map(|output| {
            let (w, h) = output.logical_size();
            Size {
                width: w as f64,
                height: h as f64,
            }
        })
        .unwrap_or_default()
}

fn buffer_size(state: &AppState, output_id: u32) -> Size {
    state
        .outputs
        .get(&output_id)
        .and_then(|output| output.buffer_dimensions())
        .map(|(w, h)| Size {
            width: w as f64,
            height: h as f64,
        })
        .unwrap_or_default()
}

fn view_scale(state: &AppState, output_id: u32) -> f64 {
    let logical_size = logical_size(state, output_id);
    state
        .windows
        .get(&output_id)
        .map(|window| {
            if logical_size.width > 0.0 {
                window.view_source.width / logical_size.width
            } else {
                1.0
            }
        })
        .unwrap_or(1.0)
}

fn scroll_scale(state: &AppState, output_id: u32) -> f64 {
    let geometry_width = state
        .outputs
        .get(&output_id)
        .map(|output| output.geometry.width as f64)
        .filter(|width| *width > 0.0)
        .unwrap_or(1.0);

    state
        .windows
        .get(&output_id)
        .map(|window| window.view_source.width / geometry_width)
        .unwrap_or(1.0)
}
