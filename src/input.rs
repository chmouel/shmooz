use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, Instant};

use wayland_client::{
    Dispatch, QueueHandle, WEnum,
    protocol::{wl_keyboard, wl_pointer, wl_seat, wl_surface},
};
use xkbcommon::xkb;

use crate::{
    clipboard, overlay, screenshot,
    state::{
        ActiveAnnotation, ActiveMove, AnnotationPoint, AnnotationTool, AppState, InteractionMode,
        KeyboardTextState, TextAnnotation,
    },
    window,
    zoom::{Point, Size, restore_view, screen_center, zoom_towards_factor},
};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

const KEY_ESC: u32 = 1;
const KEY_1: u32 = 2;
const KEY_2: u32 = 3;
const KEY_3: u32 = 4;
const KEY_4: u32 = 5;
const KEY_5: u32 = 6;
const KEY_6: u32 = 7;
const KEY_7: u32 = 8;
const KEY_8: u32 = 9;
const KEY_9: u32 = 10;
const KEY_0: u32 = 11;
const KEY_MINUS: u32 = 12;
const KEY_EQUAL: u32 = 13;
const KEY_BACKSPACE: u32 = 14;
const KEY_ENTER: u32 = 28;
const KEY_A: u32 = 30;
const KEY_B: u32 = 48;
const KEY_F: u32 = 33;
const KEY_G: u32 = 34;
const KEY_I: u32 = 23;
const KEY_J: u32 = 36;
const KEY_K: u32 = 37;
const KEY_M: u32 = 50;
const KEY_N: u32 = 49;
const KEY_O: u32 = 24;
const KEY_W: u32 = 17;
const KEY_E: u32 = 18;
const KEY_R: u32 = 19;
const KEY_T: u32 = 20;
const KEY_U: u32 = 22;
const KEY_P: u32 = 25;
const KEY_Q: u32 = 16;
const KEY_LEFTBRACE: u32 = 26;
const KEY_RIGHTBRACE: u32 = 27;
const KEY_S: u32 = 31;
const KEY_D: u32 = 32;
const KEY_H: u32 = 35;
const KEY_L: u32 = 38;
const KEY_V: u32 = 47;
const KEY_X: u32 = 45;
const KEY_Y: u32 = 21;
const KEY_Z: u32 = 44;
const KEY_C: u32 = 46;
const KEY_DOT: u32 = 52;
const KEY_SLASH: u32 = 53;
const KEY_SPACE: u32 = 57;
const KEY_KPMINUS: u32 = 74;
const KEY_KPPLUS: u32 = 78;
const KEY_KP0: u32 = 82;
const KEY_UP: u32 = 103;
const KEY_LEFT: u32 = 105;
const KEY_RIGHT: u32 = 106;
const KEY_DOWN: u32 = 108;

const DOUBLE_CLICK_TIME_MS: u32 = 400;
const KEYBOARD_PAN_STEP: f64 = 50.0;
const KEYBOARD_ZOOM_IN_FACTOR: f64 = 0.90;
const KEYBOARD_ZOOM_OUT_FACTOR: f64 = 1.0 / KEYBOARD_ZOOM_IN_FACTOR;
const SCROLL_ZOOM_BASE_FACTOR: f64 = 0.92;
const SCROLL_ZOOM_UNIT: f64 = 10.0;
const SCROLL_ZOOM_ANIMATION_DURATION: Duration = Duration::from_millis(80);
const RESET_ZOOM_ANIMATION_DURATION: Duration = Duration::from_millis(180);
const KEY_REPEAT_DELAY: Duration = Duration::from_millis(500);
const MOVE_HIT_RADIUS: f64 = 12.0;
const SCREENSHOT_TOAST_MAX_CHARS: usize = 72;
const COPY_SCREENSHOT_TOAST: &str = "Screenshot copied to clipboard";
const CLIPBOARD_UNAVAILABLE_TOAST: &str = "Clipboard copy unavailable";

pub fn repeat_timer_tick(state: &mut AppState) {
    overlay::expire_toast(state);

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
                serial,
                time,
                button,
                state: WEnum::Value(button_state),
                ..
            } => pointer_button(state, time, button, button_state, serial),
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
            wl_keyboard::Event::Keymap {
                format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
                fd,
                size,
            } => install_keyboard_keymap(state, fd, size as usize),
            wl_keyboard::Event::Key {
                key,
                serial,
                state: WEnum::Value(key_state),
                ..
            } => handle_key_event(state, key, key_state, serial),
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => update_keyboard_modifiers(state, mods_depressed, mods_latched, mods_locked, group),
            wl_keyboard::Event::Keymap { .. } | wl_keyboard::Event::RepeatInfo { .. } | _ => {}
        }
    }
}

fn focus_surface(state: &mut AppState, surface: &wl_surface::WlSurface, x: f64, y: f64) {
    let previous_focus = state.focused_window;
    let mut focus_changed = false;
    let mut focused_output = None;
    for (output_id, window) in &mut state.windows {
        if &window.surface == surface {
            state.focused_window = Some(*output_id);
            window.pointer_x = x;
            window.pointer_y = y;
            focus_changed = previous_focus != state.focused_window;
            focused_output = Some(*output_id);
            break;
        }
    }

    if focus_changed {
        overlay::update_annotation_overlays(state);
        overlay::update_color_picker_overlays(state);
    } else if let Some(output_id) = focused_output {
        overlay::refresh_annotation_overlay(state, output_id);
        if state.interaction_mode == InteractionMode::ColorPicker {
            overlay::refresh_color_picker_overlay(state, output_id);
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

    if state.interaction_mode == InteractionMode::ColorPicker {
        if let Some(window) = state.windows.get_mut(&output_id) {
            window.pointer_x = x;
            window.pointer_y = y;
            window.pointer_pressed = false;
        }
        state.color_picker_copied = false;
        overlay::refresh_annotation_overlay(state, output_id);
        overlay::refresh_color_picker_overlay(state, output_id);
        return;
    }

    if state.interaction_mode.is_annotating() {
        let active_tool = state.effective_annotation_tool();
        if let Some(window) = state.windows.get_mut(&output_id) {
            window.pointer_x = x;
            window.pointer_y = y;
        }
        if pointer_pressed {
            if active_tool == AnnotationTool::Move {
                update_active_move(state, output_id, x, y);
            } else {
                update_active_annotation(state, output_id, x, y);
            }
        } else {
            overlay::refresh_annotation_overlay(state, output_id);
        }
        return;
    }

    if pointer_pressed {
        let scale = view_scale(state, output_id);
        window::cancel_zoom_animation(state, output_id);
        if let Some(window) = state.windows.get_mut(&output_id) {
            window.view_source.x -= delta_x * scale;
            window.view_source.y -= delta_y * scale;
        }
        window::render_window(state, output_id);
    }

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.pointer_x = x;
        window.pointer_y = y;
    }

    if state
        .windows
        .get(&output_id)
        .is_some_and(|w| w.annotation_visible)
    {
        overlay::refresh_annotation_overlay(state, output_id);
    }
    overlay::refresh_spotlight_for_motion(state, output_id, delta_x, delta_y);
}

fn pointer_button(
    state: &mut AppState,
    time: u32,
    button: u32,
    button_state: wl_pointer::ButtonState,
    serial: u32,
) {
    let Some(output_id) = active_window_id(state) else {
        return;
    };

    if state.interaction_mode == InteractionMode::ColorPicker {
        match button {
            BTN_LEFT if button_state == wl_pointer::ButtonState::Released => {
                copy_color_to_clipboard(state, serial);
            }
            BTN_RIGHT if button_state == wl_pointer::ButtonState::Released => {
                state.request_exit();
            }
            _ => {}
        }
        return;
    }

    if state.interaction_mode.is_annotating() {
        let active_tool = state.effective_annotation_tool();
        match button {
            BTN_LEFT if button_state == wl_pointer::ButtonState::Pressed => {
                if active_tool == AnnotationTool::Text {
                    start_text_entry(state, output_id);
                } else if active_tool == AnnotationTool::Move {
                    start_move_annotation(state, output_id);
                } else {
                    start_annotation(state, output_id, active_tool);
                }
            }
            BTN_LEFT
                if button_state == wl_pointer::ButtonState::Released
                    && active_tool != AnnotationTool::Text =>
            {
                if active_tool == AnnotationTool::Move {
                    finish_move_annotation(state, output_id);
                } else {
                    finish_annotation(state, output_id);
                }
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
                        window.last_click_time = 0;
                    } else {
                        window.last_click_time = time;
                        window.last_click_button = button;
                    }
                    window.pointer_pressed = true;
                }

                if is_double_click {
                    animate_focused_window_to_initial(state, output_id);
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
    if state.interaction_mode.is_annotating()
        || state.interaction_mode == InteractionMode::ColorPicker
    {
        return;
    }

    let Some(output_id) = active_window_id(state) else {
        return;
    };

    let mut scroll = value / SCROLL_ZOOM_UNIT;
    if state.config.invert_scroll {
        scroll = -scroll;
    }

    zoom_focused_window_at_pointer(
        state,
        output_id,
        SCROLL_ZOOM_BASE_FACTOR.powf(scroll),
        SCROLL_ZOOM_ANIMATION_DURATION,
    );
}

fn handle_key_event(state: &mut AppState, key: u32, key_state: wl_keyboard::KeyState, serial: u32) {
    if key_state == wl_keyboard::KeyState::Released {
        if state.repeat_key == Some(key) {
            state.stop_repeat();
        }
        return;
    }

    if handle_active_text_input(state, key) {
        return;
    }

    if key == KEY_ESC && state.interaction_mode == InteractionMode::ColorPicker {
        set_interaction_mode(state, InteractionMode::Navigate);
        return;
    }

    if state.interaction_mode == InteractionMode::ColorPicker {
        if key == KEY_I {
            toggle_color_picker_mode(state);
        }
        return;
    }

    if key == KEY_ESC && state.interaction_mode.is_annotating() {
        if state.tool_override.is_some() {
            set_tool_override(state, None);
            return;
        }
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

    if is_copy_screenshot_shortcut(state, key) {
        copy_screenshot_to_clipboard(state, serial);
        return;
    }

    if key == KEY_I {
        toggle_color_picker_mode(state);
        return;
    }

    if apply_annotation_palette_shortcut(state, key) {
        return;
    }

    match key {
        KEY_ESC if state.config.close_key.is_none() => state.request_exit(),
        KEY_D => toggle_annotation_mode(state, InteractionMode::AnnotateZoomed),
        KEY_W => toggle_annotation_mode(state, InteractionMode::AnnotateUnzoomed),
        KEY_P => select_annotation_tool(state, AnnotationTool::Pen),
        KEY_H => select_annotation_tool(state, AnnotationTool::Highlighter),
        KEY_L => select_annotation_tool(state, AnnotationTool::Line),
        KEY_M if state.interaction_mode.is_annotating() => {
            toggle_tool_override(state, AnnotationTool::Move)
        }
        KEY_T if state.interaction_mode.is_annotating() => {
            toggle_tool_override(state, AnnotationTool::Text)
        }
        KEY_R => select_annotation_tool(state, AnnotationTool::Rectangle),
        KEY_E => select_annotation_tool(state, AnnotationTool::Ellipse),
        KEY_U => undo_annotation(state),
        KEY_C => clear_annotations(state),
        KEY_0 | KEY_KP0 if !state.interaction_mode.is_annotating() => restore_focused_window(state),
        KEY_S => {
            if let Some(output_id) = active_window_id(state) {
                match screenshot::save_output(state, output_id) {
                    Ok(path) => {
                        overlay::show_toast(state, output_id, screenshot_toast_message(&path));
                    }
                    Err(err) => state.record_fatal(err),
                }
            }
        }
        KEY_F => {
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

fn is_copy_screenshot_shortcut(state: &AppState, key: u32) -> bool {
    key == KEY_C && ctrl_modifier_active(state)
}

fn ctrl_modifier_active(state: &AppState) -> bool {
    state.keyboard_text.as_ref().is_some_and(|keyboard_text| {
        keyboard_text
            .state
            .mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE)
    })
}

fn copy_screenshot_to_clipboard(state: &mut AppState, serial: u32) {
    if let Some(output_id) = active_window_id(state) {
        if !clipboard::is_available(state) {
            overlay::show_toast(state, output_id, CLIPBOARD_UNAVAILABLE_TOAST);
            return;
        }
        match screenshot::output_png_bytes(state, output_id)
            .and_then(|png| clipboard::set_png_selection(state, serial, png))
        {
            Ok(()) => overlay::show_toast(state, output_id, COPY_SCREENSHOT_TOAST),
            Err(err) => state.record_fatal(err),
        }
    }
}

fn copy_color_to_clipboard(state: &mut AppState, serial: u32) {
    if let Some(output_id) = active_window_id(state) {
        if !clipboard::is_available(state) {
            overlay::show_toast(state, output_id, CLIPBOARD_UNAVAILABLE_TOAST);
            return;
        }
        match screenshot::output_color_at_pointer(state, output_id).and_then(|color| {
            clipboard::set_text_selection(state, serial, color.clone()).map(|_| color)
        }) {
            Ok(_) => {
                state.color_picker_copied = true;
                overlay::refresh_color_picker_overlay(state, output_id);
            }
            Err(err) => state.record_fatal(err),
        }
    }
}

fn handle_key_action(state: &mut AppState, key: u32) {
    if state.interaction_mode.is_annotating() {
        match key {
            KEY_LEFTBRACE => adjust_annotation_text_scale(state, -1),
            KEY_RIGHTBRACE => adjust_annotation_text_scale(state, 1),
            _ => {}
        }
        return;
    }

    match key {
        KEY_LEFTBRACE => {
            adjust_spotlight_radius(state, -0.05);
            return;
        }
        KEY_RIGHTBRACE => {
            adjust_spotlight_radius(state, 0.05);
            return;
        }
        _ => {}
    }

    let Some(output_id) = state.focused_window else {
        return;
    };

    match key {
        KEY_EQUAL | KEY_KPPLUS => {
            zoom_focused_window_at_center(state, output_id, KEYBOARD_ZOOM_IN_FACTOR);
        }
        KEY_MINUS | KEY_KPMINUS => {
            zoom_focused_window_at_center(state, output_id, KEYBOARD_ZOOM_OUT_FACTOR);
        }
        KEY_LEFT => pan_focused_window(state, output_id, -KEYBOARD_PAN_STEP, 0.0),
        KEY_RIGHT => pan_focused_window(state, output_id, KEYBOARD_PAN_STEP, 0.0),
        KEY_UP => pan_focused_window(state, output_id, 0.0, -KEYBOARD_PAN_STEP),
        KEY_DOWN => pan_focused_window(state, output_id, 0.0, KEYBOARD_PAN_STEP),
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

fn toggle_color_picker_mode(state: &mut AppState) {
    let next_mode = if state.interaction_mode == InteractionMode::ColorPicker {
        InteractionMode::Navigate
    } else {
        InteractionMode::ColorPicker
    };
    set_interaction_mode(state, next_mode);
}

fn set_interaction_mode(state: &mut AppState, mode: InteractionMode) {
    if mode == InteractionMode::AnnotateUnzoomed {
        restore_all_windows(state);
    }

    cancel_active_annotations(state);
    if !mode.is_annotating() {
        state.tool_override = None;
    }
    state.color_picker_copied = false;
    state.interaction_mode = mode;
    state.stop_repeat();
    overlay::update_spotlight_overlays(state);
    overlay::refresh_zoom_badge_overlays(state);
    overlay::update_annotation_overlays(state);
    overlay::update_color_picker_overlays(state);
}

fn select_annotation_tool(state: &mut AppState, tool: AnnotationTool) {
    if matches!(tool, AnnotationTool::Move | AnnotationTool::Text) {
        set_tool_override(state, Some(tool));
        return;
    }
    if state.annotation_tool != tool {
        cancel_active_text_entries(state, true);
    }
    state.annotation_tool = tool;
    set_tool_override(state, None);
}

fn toggle_tool_override(state: &mut AppState, tool: AnnotationTool) {
    let next_override = if state.tool_override == Some(tool) {
        None
    } else {
        Some(tool)
    };
    set_tool_override(state, next_override);
}

fn set_tool_override(state: &mut AppState, tool: Option<AnnotationTool>) {
    if state.tool_override == tool {
        return;
    }

    if state.tool_override == Some(AnnotationTool::Move) && tool != Some(AnnotationTool::Move) {
        clear_active_moves(state);
    }
    if tool != Some(AnnotationTool::Text) {
        cancel_active_text_entries(state, true);
    }

    state.tool_override = tool;
    overlay::refresh_zoom_badge_overlays(state);
    overlay::update_annotation_overlays(state);
}

fn start_annotation(state: &mut AppState, output_id: u32, tool: AnnotationTool) {
    let point = state.windows.get(&output_id).map(|window| {
        screen_to_annotation_point(state, output_id, window.pointer_x, window.pointer_y)
    });
    let color = state.annotation_color_for(tool);

    let Some(point) = point else {
        return;
    };

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.pointer_pressed = true;
        window.active_annotation = Some(ActiveAnnotation::new(tool, point, color));
        window.active_move = None;
    }
    overlay::update_annotation_overlays(state);
}

fn start_move_annotation(state: &mut AppState, output_id: u32) {
    let point = state.windows.get(&output_id).map(|window| {
        screen_to_annotation_point(state, output_id, window.pointer_x, window.pointer_y)
    });
    let Some(point) = point else {
        return;
    };

    let active_move = state.windows.get(&output_id).and_then(|window| {
        let tolerance = move_hit_tolerance(state, output_id);
        window
            .annotations
            .iter()
            .enumerate()
            .rev()
            .find(|(_, annotation)| annotation.hit_test(point, tolerance))
            .map(|(annotation_index, _)| ActiveMove {
                annotation_index,
                last_point: point,
            })
    });

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.pointer_pressed = active_move.is_some();
        window.active_annotation = None;
        window.active_move = active_move;
    }
    overlay::update_annotation_overlays(state);
}

fn update_active_annotation(state: &mut AppState, output_id: u32, x: f64, y: f64) {
    let point = screen_to_annotation_point(state, output_id, x, y);
    if let Some(window) = state.windows.get_mut(&output_id)
        && let Some(active_annotation) = window.active_annotation.as_mut()
    {
        active_annotation.update(point);
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

    if let Some(annotation) = completed
        && let Some(window) = state.windows.get_mut(&output_id)
    {
        window.annotations.push(annotation);
    }

    overlay::update_annotation_overlays(state);
}

fn update_active_move(state: &mut AppState, output_id: u32, x: f64, y: f64) {
    let point = screen_to_annotation_point(state, output_id, x, y);
    if let Some(window) = state.windows.get_mut(&output_id)
        && let Some(active_move) = window.active_move.as_mut()
    {
        let dx = point.x - active_move.last_point.x;
        let dy = point.y - active_move.last_point.y;
        if dx != 0 || dy != 0 {
            if let Some(annotation) = window.annotations.get_mut(active_move.annotation_index) {
                annotation.translate(dx, dy);
            }
            active_move.last_point = point;
        }
    }
    overlay::refresh_annotation_overlay(state, output_id);
}

fn finish_move_annotation(state: &mut AppState, output_id: u32) {
    if let Some(window) = state.windows.get_mut(&output_id) {
        window.pointer_pressed = false;
        window.active_move = None;
    }
    overlay::update_annotation_overlays(state);
}

fn start_text_entry(state: &mut AppState, output_id: u32) {
    cancel_active_text_entries(state, false);

    let point = state.windows.get(&output_id).map(|window| {
        screen_to_annotation_point(state, output_id, window.pointer_x, window.pointer_y)
    });
    let Some(point) = point else {
        return;
    };
    let text_scale = state.text_annotation_scale;
    let text_color = state.annotation_color_for(AnnotationTool::Text);

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.pointer_pressed = false;
        window.active_annotation = None;
        window.active_move = None;
        window.active_text = Some(TextAnnotation {
            position: point,
            text: String::new(),
            color: text_color,
            scale: text_scale,
        });
    }
    overlay::update_annotation_overlays(state);
}

fn handle_active_text_input(state: &mut AppState, key: u32) -> bool {
    let Some(output_id) = active_text_output_id(state) else {
        return false;
    };

    match key {
        KEY_ENTER => commit_or_discard_active_text_entry(state, output_id),
        KEY_BACKSPACE => {
            if let Some(window) = state.windows.get_mut(&output_id)
                && let Some(text) = window.active_text.as_mut()
            {
                text.text.pop();
            }
            overlay::refresh_annotation_overlay(state, output_id);
        }
        KEY_LEFTBRACE => adjust_annotation_text_scale(state, -1),
        KEY_RIGHTBRACE => adjust_annotation_text_scale(state, 1),
        KEY_ESC => {
            if state.tool_override == Some(AnnotationTool::Text) {
                commit_or_discard_active_text_entry(state, output_id);
                set_tool_override(state, None);
            } else {
                if let Some(window) = state.windows.get_mut(&output_id) {
                    window.active_text = None;
                }
                overlay::update_annotation_overlays(state);
            }
        }
        _ => {
            if let Some(text_input) = current_text_input(state, key) {
                let changed = if let Some(window) = state.windows.get_mut(&output_id)
                    && let Some(text) = window.active_text.as_mut()
                    && text.text.chars().count() + text_input.chars().count() <= 64
                {
                    text.text.push_str(&text_input);
                    true
                } else {
                    false
                };
                if changed {
                    overlay::refresh_annotation_overlay(state, output_id);
                }
            }
        }
    }

    true
}

fn commit_or_discard_active_text_entry(state: &mut AppState, output_id: u32) {
    let entry = state
        .windows
        .get_mut(&output_id)
        .and_then(|window| window.active_text.take());

    if let Some(entry) = entry.filter(|entry| !entry.text.is_empty())
        && let Some(window) = state.windows.get_mut(&output_id)
    {
        window
            .annotations
            .push(crate::state::AnnotationItem::Text(entry));
    }

    overlay::update_annotation_overlays(state);
}

fn undo_annotation(state: &mut AppState) {
    let Some(output_id) = active_window_id(state) else {
        return;
    };

    if let Some(window) = state.windows.get_mut(&output_id) {
        if window.active_text.take().is_none()
            && window.active_annotation.take().is_none()
            && window.active_move.take().is_none()
        {
            window.annotations.pop();
        }
        window.pointer_pressed = false;
    }
    overlay::update_annotation_overlays(state);
}

fn reset_active_state(window: &mut crate::window::WindowState) {
    window.active_annotation = None;
    window.active_move = None;
    window.active_text = None;
    window.pointer_pressed = false;
}

fn clear_annotations(state: &mut AppState) {
    let Some(output_id) = active_window_id(state) else {
        return;
    };

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.annotations.clear();
        reset_active_state(window);
    }
    overlay::update_annotation_overlays(state);
}

fn cancel_active_annotations(state: &mut AppState) {
    let output_ids = state.windows.keys().copied().collect::<Vec<_>>();
    for output_id in output_ids {
        if let Some(window) = state.windows.get_mut(&output_id) {
            reset_active_state(window);
        }
        overlay::update_window_overlays(state, output_id);
    }
}

fn clear_active_moves(state: &mut AppState) {
    for window in state.windows.values_mut() {
        if window.active_move.take().is_some() {
            window.pointer_pressed = false;
        }
    }
}

fn cancel_active_text_entries(state: &mut AppState, discard_only: bool) {
    if state.windows.values().all(|w| w.active_text.is_none()) {
        return;
    }
    let output_ids = state.windows.keys().copied().collect::<Vec<_>>();
    if discard_only {
        for output_id in &output_ids {
            if let Some(window) = state.windows.get_mut(output_id) {
                window.active_text = None;
            }
        }
        overlay::update_annotation_overlays(state);
    } else {
        for output_id in output_ids {
            commit_or_discard_active_text_entry(state, output_id);
        }
    }
}

fn restore_focused_window(state: &mut AppState) {
    let Some(output_id) = state.focused_window else {
        return;
    };

    animate_focused_window_to_initial(state, output_id);
}

fn animate_focused_window_to_initial(state: &mut AppState, output_id: u32) {
    let Some(target) = state
        .windows
        .get(&output_id)
        .map(|window| window.initial_view_source)
    else {
        return;
    };

    window::animate_window_to_view(state, output_id, target, RESET_ZOOM_ANIMATION_DURATION);
}

fn restore_all_windows(state: &mut AppState) {
    let output_ids = state.windows.keys().copied().collect::<Vec<_>>();
    for output_id in output_ids {
        window::cancel_zoom_animation(state, output_id);
        if let Some(window) = state.windows.get_mut(&output_id) {
            restore_view(&mut window.view_source, window.initial_view_source);
        }
        window::render_window(state, output_id);
    }
}

fn pan_focused_window(state: &mut AppState, output_id: u32, dx: f64, dy: f64) {
    window::cancel_zoom_animation(state, output_id);
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

fn active_text_output_id(state: &AppState) -> Option<u32> {
    if let Some(output_id) = state.focused_window.filter(|output_id| {
        state
            .windows
            .get(output_id)
            .is_some_and(|window| window.active_text.is_some())
    }) {
        return Some(output_id);
    }

    let mut active_outputs = state
        .windows
        .iter()
        .filter_map(|(output_id, window)| window.active_text.as_ref().map(|_| *output_id));
    let output_id = active_outputs.next()?;
    if active_outputs.next().is_some() {
        return None;
    }
    Some(output_id)
}

fn screenshot_toast_message(path: &Path) -> String {
    let prefix = "File saved to ";
    let display = path.display().to_string();
    let max_path_chars = SCREENSHOT_TOAST_MAX_CHARS.saturating_sub(prefix.chars().count());
    let shortened = if display.chars().count() <= max_path_chars {
        display
    } else {
        format!(
            "...{}",
            tail_chars(&display, max_path_chars.saturating_sub(3))
        )
    };

    format!("{prefix}{shortened}")
}

fn tail_chars(text: &str, max_chars: usize) -> &str {
    let skip = text.chars().count().saturating_sub(max_chars);
    text.char_indices()
        .nth(skip)
        .map(|(i, _)| &text[i..])
        .unwrap_or(text)
}

fn zoom_focused_window_at_center(state: &mut AppState, output_id: u32, zoom_factor: f64) {
    let logical_size = logical_size(state, output_id);
    let center = screen_center(logical_size);
    zoom_focused_window(
        state,
        output_id,
        zoom_factor,
        center,
        window::ZOOM_ANIMATION_DURATION,
    );
}

fn zoom_focused_window_at_pointer(
    state: &mut AppState,
    output_id: u32,
    zoom_factor: f64,
    duration: Duration,
) {
    let logical_size = logical_size(state, output_id);
    let center = state
        .windows
        .get(&output_id)
        .map(|window| Point {
            x: window.pointer_x,
            y: window.pointer_y,
        })
        .unwrap_or_else(|| screen_center(logical_size));
    zoom_focused_window(state, output_id, zoom_factor, center, duration);
}

fn zoom_focused_window(
    state: &mut AppState,
    output_id: u32,
    zoom_factor: f64,
    center: Point,
    duration: Duration,
) {
    let logical_size = logical_size(state, output_id);
    let buffer_size = buffer_size(state, output_id);

    let Some(mut target) = state.windows.get(&output_id).map(|window| {
        window
            .zoom_animation
            .as_ref()
            .map(|animation| animation.target)
            .unwrap_or(window.view_source)
    }) else {
        return;
    };

    zoom_towards_factor(&mut target, zoom_factor, center, logical_size, buffer_size);
    window::animate_window_to_view(state, output_id, target, duration);
}

fn adjust_spotlight_radius(state: &mut AppState, delta: f64) {
    let previous = state.spotlight_radius_frac;
    state.spotlight_radius_frac = (state.spotlight_radius_frac + delta).clamp(0.05, 0.90);
    if (state.spotlight_radius_frac - previous).abs() > f64::EPSILON {
        overlay::refresh_visible_spotlight_overlays(state);
    }
}

fn apply_annotation_palette_shortcut(state: &mut AppState, key: u32) -> bool {
    if !state.interaction_mode.is_annotating() {
        return false;
    }

    let Some(index) = palette_index_for_key(key) else {
        return false;
    };

    apply_annotation_palette_index(state, index);
    true
}

fn apply_annotation_palette_index(state: &mut AppState, index: usize) {
    let palette_changed = state.select_annotation_color(index);
    let palette_color = state.selected_palette_color();
    let mut refreshed_outputs = Vec::new();

    for (output_id, window) in &mut state.windows {
        let mut overlay_changed = false;

        if let Some(active_annotation) = window.active_annotation.as_mut() {
            active_annotation.recolor(palette_color);
            overlay_changed = true;
        }
        if let Some(text) = window.active_text.as_mut() {
            text.recolor(palette_color);
            overlay_changed = true;
        }
        if let Some(active_move) = window.active_move
            && let Some(annotation) = window.annotations.get_mut(active_move.annotation_index)
        {
            annotation.recolor(palette_color);
            overlay_changed = true;
        }

        if overlay_changed {
            refreshed_outputs.push(*output_id);
        }
    }

    if palette_changed {
        overlay::refresh_zoom_badge_overlays(state);
    }
    for output_id in refreshed_outputs {
        overlay::refresh_annotation_overlay(state, output_id);
    }
}

fn palette_index_for_key(key: u32) -> Option<usize> {
    match key {
        KEY_1 => Some(0),
        KEY_2 => Some(1),
        KEY_3 => Some(2),
        KEY_4 => Some(3),
        KEY_5 => Some(4),
        KEY_6 => Some(5),
        KEY_7 => Some(6),
        KEY_8 => Some(7),
        KEY_9 => Some(8),
        KEY_0 => Some(9),
        KEY_MINUS => Some(10),
        KEY_EQUAL => Some(11),
        _ => None,
    }
}

fn adjust_annotation_text_scale(state: &mut AppState, delta: i32) {
    if !state.adjust_text_annotation_scale(delta) {
        return;
    }

    let scale = state.text_annotation_scale;
    let output_ids = state
        .windows
        .iter_mut()
        .filter_map(|(output_id, window)| {
            window.active_text.as_mut().map(|text| {
                text.scale = scale;
                *output_id
            })
        })
        .collect::<Vec<_>>();

    for output_id in output_ids {
        overlay::refresh_annotation_overlay(state, output_id);
    }
    overlay::refresh_zoom_badge_overlays(state);
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

fn move_hit_tolerance(state: &AppState, output_id: u32) -> f64 {
    (view_scale(state, output_id) * MOVE_HIT_RADIUS).max(2.0)
}

fn screen_to_annotation_point(state: &AppState, output_id: u32, x: f64, y: f64) -> AnnotationPoint {
    let logical_size = logical_size(state, output_id);
    let Some(window) = state.windows.get(&output_id) else {
        return AnnotationPoint::new(x, y);
    };

    let normalized_x = if logical_size.width > 0.0 {
        (x / logical_size.width).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let normalized_y = if logical_size.height > 0.0 {
        (y / logical_size.height).clamp(0.0, 1.0)
    } else {
        0.0
    };

    AnnotationPoint::new(
        window.view_source.x + normalized_x * window.view_source.width,
        window.view_source.y + normalized_y * window.view_source.height,
    )
}

#[cfg(test)]
mod tests {
    use wayland_client::protocol::wl_keyboard;
    use xkbcommon::xkb;

    use crate::{
        config::{APP_ID, CloseKey, Config},
        state::{ANNOTATION_COLOR_PALETTE, ActiveAnnotation, DEFAULT_TEXT_ANNOTATION_SCALE},
    };

    use super::{
        AnnotationPoint, AnnotationTool, AppState, InteractionMode, KEY_1, KEY_3, KEY_C, KEY_ESC,
        KEY_L, KEY_M, KEY_RIGHTBRACE, KEY_T, apply_annotation_palette_index, ctrl_modifier_active,
        current_text_input, handle_key_event, is_copy_screenshot_shortcut, select_annotation_tool,
    };

    fn test_config() -> Config {
        Config {
            app_id: APP_ID,
            close_key: None,
            initial_zoom: 0.0,
            output_filter: None,
            invert_scroll: false,
            spotlight: false,
            screenshot_dir: "shots".into(),
            show_indicator: true,
        }
    }

    fn test_config_with_close_key(close_key: CloseKey) -> Config {
        Config {
            close_key: Some(close_key),
            ..test_config()
        }
    }

    fn ctrl_pressed_state() -> AppState {
        let mut state = AppState::new(test_config());
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap =
            xkb::Keymap::new_from_names(&context, "", "", "us", "", None, xkb::COMPILE_NO_FLAGS)
                .unwrap();
        let ctrl_mask = 1u32 << keymap.mod_get_index(xkb::MOD_NAME_CTRL);
        let mut state_machine = xkb::State::new(&keymap);
        state_machine.update_mask(ctrl_mask, 0, 0, 0, 0, 0);
        state.keyboard_text = Some(super::KeyboardTextState {
            _context: context,
            _keymap: keymap,
            state: state_machine,
            compose: None,
        });
        state
    }

    #[test]
    fn move_mode_is_toggled_without_losing_selected_tool() {
        let mut state = AppState::new(test_config());
        state.interaction_mode = InteractionMode::AnnotateZoomed;
        state.annotation_tool = AnnotationTool::Rectangle;

        handle_key_event(&mut state, KEY_M, wl_keyboard::KeyState::Pressed, 0);
        assert_eq!(state.tool_override, Some(AnnotationTool::Move));
        assert_eq!(state.annotation_tool, AnnotationTool::Rectangle);
        assert_eq!(state.effective_annotation_tool(), AnnotationTool::Move);

        handle_key_event(&mut state, KEY_M, wl_keyboard::KeyState::Pressed, 0);
        assert_eq!(state.tool_override, None);
        assert_eq!(state.effective_annotation_tool(), AnnotationTool::Rectangle);
    }

    #[test]
    fn escape_leaves_move_mode_before_navigation() {
        let mut state = AppState::new(test_config());
        state.interaction_mode = InteractionMode::AnnotateZoomed;
        state.annotation_tool = AnnotationTool::Line;
        state.tool_override = Some(AnnotationTool::Move);

        handle_key_event(&mut state, KEY_ESC, wl_keyboard::KeyState::Pressed, 0);

        assert_eq!(state.interaction_mode, InteractionMode::AnnotateZoomed);
        assert_eq!(state.tool_override, None);
        assert_eq!(state.effective_annotation_tool(), AnnotationTool::Line);
    }

    #[test]
    fn selecting_another_tool_disables_move_mode() {
        let mut state = AppState::new(test_config());
        state.annotation_tool = AnnotationTool::Pen;
        state.tool_override = Some(AnnotationTool::Move);

        select_annotation_tool(&mut state, AnnotationTool::Text);

        assert_eq!(state.tool_override, Some(AnnotationTool::Text));
        assert_eq!(state.annotation_tool, AnnotationTool::Pen);
        assert_eq!(state.effective_annotation_tool(), AnnotationTool::Text);
    }

    #[test]
    fn text_mode_is_toggled_without_losing_selected_tool() {
        let mut state = AppState::new(test_config());
        state.interaction_mode = InteractionMode::AnnotateZoomed;
        state.annotation_tool = AnnotationTool::Ellipse;

        handle_key_event(&mut state, KEY_T, wl_keyboard::KeyState::Pressed, 0);
        assert_eq!(state.tool_override, Some(AnnotationTool::Text));
        assert_eq!(state.annotation_tool, AnnotationTool::Ellipse);
        assert_eq!(state.effective_annotation_tool(), AnnotationTool::Text);

        handle_key_event(&mut state, KEY_T, wl_keyboard::KeyState::Pressed, 0);
        assert_eq!(state.tool_override, None);
        assert_eq!(state.effective_annotation_tool(), AnnotationTool::Ellipse);
    }

    #[test]
    fn escape_leaves_text_mode_before_navigation() {
        let mut state = AppState::new(test_config());
        state.interaction_mode = InteractionMode::AnnotateZoomed;
        state.annotation_tool = AnnotationTool::Highlighter;
        state.tool_override = Some(AnnotationTool::Text);

        handle_key_event(&mut state, KEY_ESC, wl_keyboard::KeyState::Pressed, 0);

        assert_eq!(state.interaction_mode, InteractionMode::AnnotateZoomed);
        assert_eq!(state.tool_override, None);
        assert_eq!(
            state.effective_annotation_tool(),
            AnnotationTool::Highlighter
        );
    }

    #[test]
    fn explicit_escape_close_key_keeps_escape_backing_out_of_annotation() {
        let mut state = AppState::new(test_config_with_close_key(CloseKey::Escape));
        state.interaction_mode = InteractionMode::AnnotateZoomed;
        state.annotation_tool = AnnotationTool::Line;
        state.tool_override = Some(AnnotationTool::Move);

        handle_key_event(&mut state, KEY_ESC, wl_keyboard::KeyState::Pressed, 0);

        assert_eq!(state.interaction_mode, InteractionMode::AnnotateZoomed);
        assert_eq!(state.tool_override, None);

        handle_key_event(&mut state, KEY_ESC, wl_keyboard::KeyState::Pressed, 0);

        assert_eq!(state.interaction_mode, InteractionMode::Navigate);
        assert_eq!(state.tool_override, None);
    }

    #[test]
    fn line_tool_is_selectable() {
        let mut state = AppState::new(test_config());
        state.annotation_tool = AnnotationTool::Pen;

        handle_key_event(&mut state, KEY_L, wl_keyboard::KeyState::Pressed, 0);

        assert_eq!(state.annotation_tool, AnnotationTool::Line);
    }

    #[test]
    fn ctrl_modifier_is_detected_from_xkb_state() {
        let state = ctrl_pressed_state();

        assert!(ctrl_modifier_active(&state));
        assert!(is_copy_screenshot_shortcut(&state, KEY_C));
    }

    #[test]
    fn plain_c_does_not_become_copy_shortcut_without_ctrl() {
        let state = AppState::new(test_config());

        assert!(!ctrl_modifier_active(&state));
        assert!(!is_copy_screenshot_shortcut(&state, KEY_C));
    }

    #[test]
    fn palette_shortcuts_change_annotation_color_in_draw_mode() {
        let mut state = AppState::new(test_config());
        state.interaction_mode = InteractionMode::AnnotateZoomed;

        handle_key_event(&mut state, KEY_3, wl_keyboard::KeyState::Pressed, 0);

        assert_eq!(state.annotation_color_index, 2);
    }

    #[test]
    fn palette_updates_active_annotation_preview_color() {
        let mut annotation = ActiveAnnotation::new(
            AnnotationTool::Pen,
            AnnotationPoint { x: 10, y: 20 },
            ANNOTATION_COLOR_PALETTE[0].value,
        );
        let mut state = AppState::new(test_config());

        apply_annotation_palette_index(&mut state, 3);
        annotation.recolor(state.selected_palette_color());

        let ActiveAnnotation::Stroke(stroke) = annotation else {
            panic!("stroke annotation expected");
        };
        assert_eq!(stroke.color, ANNOTATION_COLOR_PALETTE[3].value);
    }

    #[test]
    fn current_text_input_accepts_digits() {
        let mut state = AppState::new(test_config());

        assert_eq!(current_text_input(&mut state, KEY_1).as_deref(), Some("1"));
    }

    #[test]
    fn text_size_shortcuts_adjust_default_text_scale_in_draw_mode() {
        let mut state = AppState::new(test_config());
        state.interaction_mode = InteractionMode::AnnotateZoomed;

        handle_key_event(
            &mut state,
            KEY_RIGHTBRACE,
            wl_keyboard::KeyState::Pressed,
            0,
        );

        assert_eq!(
            state.text_annotation_scale,
            DEFAULT_TEXT_ANNOTATION_SCALE + 1
        );
    }
}

fn install_keyboard_keymap(state: &mut AppState, fd: std::os::fd::OwnedFd, size: usize) {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = unsafe {
        xkb::Keymap::new_from_fd(
            &context,
            fd,
            size,
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
    };
    let Ok(Some(keymap)) = keymap else {
        state.keyboard_text = None;
        return;
    };
    let state_machine = xkb::State::new(&keymap);
    let compose = compose_state_for_locale(&context);

    state.keyboard_text = Some(KeyboardTextState {
        _context: context,
        _keymap: keymap,
        state: state_machine,
        compose,
    });
}

fn update_keyboard_modifiers(
    state: &mut AppState,
    mods_depressed: u32,
    mods_latched: u32,
    mods_locked: u32,
    group: u32,
) {
    if let Some(keyboard_text) = state.keyboard_text.as_mut() {
        keyboard_text
            .state
            .update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);
    }
}

fn compose_state_for_locale(context: &xkb::Context) -> Option<xkb::compose::State> {
    let locale = std::env::var_os("LC_ALL")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("LC_CTYPE").filter(|value| !value.is_empty()))
        .or_else(|| std::env::var_os("LANG").filter(|value| !value.is_empty()))
        .unwrap_or_else(|| OsString::from("C.UTF-8"));

    let table =
        xkb::compose::Table::new_from_locale(context, &locale, xkb::compose::COMPILE_NO_FLAGS)
            .ok()?;
    Some(xkb::compose::State::new(
        &table,
        xkb::compose::STATE_NO_FLAGS,
    ))
}

fn current_text_input(state: &mut AppState, key: u32) -> Option<String> {
    let keycode = xkb::Keycode::new(key + 8);

    if let Some(keyboard_text) = state.keyboard_text.as_mut() {
        if let Some(compose) = keyboard_text.compose.as_mut() {
            let keysym = keyboard_text.state.key_get_one_sym(keycode);
            let _ = compose.feed(keysym);
            match compose.status() {
                xkb::compose::Status::Composing => return None,
                xkb::compose::Status::Composed => {
                    let text = compose.utf8();
                    compose.reset();
                    return sanitize_text_input(text);
                }
                xkb::compose::Status::Cancelled => {
                    compose.reset();
                }
                xkb::compose::Status::Nothing => {}
            }
        }

        return sanitize_text_input(Some(keyboard_text.state.key_get_utf8(keycode)));
    }

    text_input_char(key).map(|ch| ch.to_string())
}

fn sanitize_text_input(input: Option<String>) -> Option<String> {
    let input = input?;
    let filtered = input
        .chars()
        .filter(|ch| !ch.is_control() && *ch != '\u{7f}')
        .collect::<String>();
    (!filtered.is_empty()).then_some(filtered)
}

fn text_input_char(key: u32) -> Option<char> {
    match key {
        KEY_A => Some('A'),
        KEY_B => Some('B'),
        KEY_C => Some('C'),
        KEY_D => Some('D'),
        KEY_E => Some('E'),
        KEY_F => Some('F'),
        KEY_G => Some('G'),
        KEY_H => Some('H'),
        KEY_I => Some('I'),
        KEY_J => Some('J'),
        KEY_K => Some('K'),
        KEY_L => Some('L'),
        KEY_M => Some('M'),
        KEY_N => Some('N'),
        KEY_O => Some('O'),
        KEY_P => Some('P'),
        KEY_Q => Some('Q'),
        KEY_R => Some('R'),
        KEY_S => Some('S'),
        KEY_T => Some('T'),
        KEY_U => Some('U'),
        KEY_V => Some('V'),
        KEY_W => Some('W'),
        KEY_X => Some('X'),
        KEY_Y => Some('Y'),
        KEY_Z => Some('Z'),
        KEY_0 => Some('0'),
        KEY_1 => Some('1'),
        KEY_2 => Some('2'),
        KEY_3 => Some('3'),
        KEY_4 => Some('4'),
        KEY_5 => Some('5'),
        KEY_6 => Some('6'),
        KEY_7 => Some('7'),
        KEY_8 => Some('8'),
        KEY_9 => Some('9'),
        KEY_SPACE => Some(' '),
        KEY_MINUS => Some('-'),
        KEY_DOT => Some('.'),
        KEY_SLASH => Some('/'),
        _ => None,
    }
}
