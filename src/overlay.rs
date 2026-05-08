use std::time::{Duration, Instant};

use wayland_client::{
    Dispatch, QueueHandle, delegate_noop,
    protocol::{wl_buffer, wl_callback, wl_region, wl_shm, wl_subsurface, wl_surface},
};

use crate::{
    error::{AppError, Result},
    render,
    shm::ShmBuffer,
    state::{
        ActiveAnnotation, AnnotationItem, AnnotationPoint, AppState, ShapeAnnotation,
        StrokeAnnotation, TextAnnotation, ToastState,
    },
    window::OverlayBufferSlot,
    zoom::ViewRect,
};

const ANNOTATION_BUFFER_COUNT: usize = 3;
const SPOTLIGHT_MOVE_THRESHOLD_SQ: f64 = 16.0;
pub const ZOOM_BADGE_WIDTH: i32 = 480;
pub const ZOOM_BADGE_HEIGHT: i32 = 136;
pub const ZOOM_BADGE_MARGIN: i32 = 24;
const TOAST_WIDTH: i32 = ZOOM_BADGE_WIDTH;
const TOAST_HEIGHT: i32 = ZOOM_BADGE_HEIGHT;
const TOAST_DURATION: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
struct AnnotationBufferKey {
    output_id: u32,
    slot: usize,
}

#[derive(Clone, Copy)]
struct AnnotationFrameKey {
    output_id: u32,
}

pub fn create_overlays_for_window(
    state: &mut AppState,
    output_id: u32,
    qh: &QueueHandle<AppState>,
) -> Result<()> {
    if state.globals.subcompositor.is_none() {
        return Ok(());
    }

    create_spotlight_overlay(state, output_id, qh)?;
    create_annotation_overlay(state, output_id, qh)?;
    create_zoom_badge_overlay(state, output_id, qh)?;
    create_toast_overlay(state, output_id, qh)?;
    update_window_overlays(state, output_id);

    Ok(())
}

pub fn update_window_overlays(state: &mut AppState, output_id: u32) {
    update_spotlight_overlay(state, output_id);
    update_annotation_overlay(state, output_id);
    update_zoom_badge_overlay(state, output_id);
    update_toast_overlay(state, output_id);
}

pub fn update_spotlight_overlays(state: &mut AppState) {
    let output_ids = state.windows.keys().copied().collect::<Vec<_>>();
    for output_id in output_ids {
        update_spotlight_overlay(state, output_id);
    }
}

pub fn refresh_visible_spotlight_overlays(state: &mut AppState) {
    let output_ids = state
        .windows
        .iter()
        .filter_map(|(output_id, window)| window.spotlight_visible.then_some(*output_id))
        .collect::<Vec<_>>();

    for output_id in output_ids {
        refresh_spotlight_overlay(state, output_id);
    }
}

pub fn refresh_annotation_overlay(state: &mut AppState, output_id: u32) {
    if let Some(window) = state.windows.get_mut(&output_id) {
        window.annotation_redraw_pending = true;
    }
    flush_annotation_overlay(state, output_id);
}

pub fn refresh_zoom_badge_overlays(state: &mut AppState) {
    let output_ids = state.windows.keys().copied().collect::<Vec<_>>();
    for output_id in output_ids {
        refresh_zoom_badge_overlay(state, output_id);
    }
}

pub fn show_toast(state: &mut AppState, output_id: u32, message: impl Into<String>) {
    let previous_output = state.toast.as_ref().map(|toast| toast.output_id);
    state.toast = Some(ToastState {
        output_id,
        message: message.into(),
        expires_at: Instant::now() + TOAST_DURATION,
    });

    if let Some(previous_output) =
        previous_output.filter(|previous_output| *previous_output != output_id)
    {
        set_toast_visible(state, previous_output, false);
    }

    update_toast_overlay(state, output_id);
}

pub fn expire_toast(state: &mut AppState) {
    let expired_output = state
        .toast
        .as_ref()
        .and_then(|toast| (Instant::now() >= toast.expires_at).then_some(toast.output_id));
    if let Some(output_id) = expired_output {
        state.toast = None;
        set_toast_visible(state, output_id, false);
    }
}

pub fn update_annotation_overlays(state: &mut AppState) {
    let output_ids = state.windows.keys().copied().collect::<Vec<_>>();
    for output_id in output_ids {
        update_annotation_overlay(state, output_id);
    }
}

pub fn refresh_spotlight_for_motion(
    state: &mut AppState,
    output_id: u32,
    delta_x: f64,
    delta_y: f64,
) {
    if delta_x * delta_x + delta_y * delta_y < SPOTLIGHT_MOVE_THRESHOLD_SQ {
        return;
    }

    let is_visible = state
        .windows
        .get(&output_id)
        .map(|window| window.spotlight_visible)
        .unwrap_or(false);
    if is_visible {
        refresh_spotlight_overlay(state, output_id);
    }
}

fn create_spotlight_overlay(
    state: &mut AppState,
    output_id: u32,
    qh: &QueueHandle<AppState>,
) -> Result<()> {
    let Some(subcompositor) = state.globals.subcompositor.clone() else {
        return Ok(());
    };
    let compositor = state.globals.compositor()?;
    let shm = state.globals.shm()?;
    let size = logical_size(state, output_id);
    if size.0 <= 0 || size.1 <= 0 {
        return Ok(());
    }

    let buffer = ShmBuffer::create(
        &shm,
        qh,
        wl_shm::Format::Argb8888,
        size.0,
        size.1,
        size.0 * 4,
    )?;
    let surface = compositor.create_surface(qh, ());
    let subsurface =
        subcompositor.get_subsurface(&surface, &window_surface(state, output_id)?, qh, ());
    make_surface_input_transparent(&compositor, &surface, qh);
    subsurface.set_position(0, 0);
    subsurface.set_desync();

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.spotlight_buffer = Some(buffer);
        window.spotlight_surface = Some(surface);
        window.spotlight_subsurface = Some(subsurface);
    }

    Ok(())
}

fn create_annotation_overlay(
    state: &mut AppState,
    output_id: u32,
    qh: &QueueHandle<AppState>,
) -> Result<()> {
    let Some(subcompositor) = state.globals.subcompositor.clone() else {
        return Ok(());
    };
    let compositor = state.globals.compositor()?;
    let shm = state.globals.shm()?;
    let size = logical_size(state, output_id);
    if size.0 <= 0 || size.1 <= 0 {
        return Ok(());
    }

    let surface = compositor.create_surface(qh, ());
    let subsurface =
        subcompositor.get_subsurface(&surface, &window_surface(state, output_id)?, qh, ());
    make_surface_input_transparent(&compositor, &surface, qh);
    subsurface.set_position(0, 0);
    subsurface.set_desync();

    let mut buffers = Vec::with_capacity(ANNOTATION_BUFFER_COUNT);
    for slot in 0..ANNOTATION_BUFFER_COUNT {
        buffers.push(OverlayBufferSlot {
            buffer: ShmBuffer::create_with_data(
                &shm,
                qh,
                wl_shm::Format::Argb8888,
                size.0,
                size.1,
                size.0 * 4,
                AnnotationBufferKey { output_id, slot },
            )?,
            busy: false,
        });
    }

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.annotation_buffers = buffers;
        window.annotation_surface = Some(surface);
        window.annotation_subsurface = Some(subsurface);
    }

    Ok(())
}

fn create_zoom_badge_overlay(
    state: &mut AppState,
    output_id: u32,
    qh: &QueueHandle<AppState>,
) -> Result<()> {
    if !state.config.show_indicator {
        return Ok(());
    }
    let Some(subcompositor) = state.globals.subcompositor.clone() else {
        return Ok(());
    };

    let compositor = state.globals.compositor()?;
    let shm = state.globals.shm()?;

    let buffer = ShmBuffer::create(
        &shm,
        qh,
        wl_shm::Format::Argb8888,
        ZOOM_BADGE_WIDTH,
        ZOOM_BADGE_HEIGHT,
        ZOOM_BADGE_WIDTH * 4,
    )?;

    let surface = compositor.create_surface(qh, ());
    let subsurface =
        subcompositor.get_subsurface(&surface, &window_surface(state, output_id)?, qh, ());
    make_surface_input_transparent(&compositor, &surface, qh);
    subsurface.set_position(ZOOM_BADGE_MARGIN, ZOOM_BADGE_MARGIN);
    subsurface.set_desync();

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.zoom_badge_buffer = Some(buffer);
        window.zoom_badge_surface = Some(surface);
        window.zoom_badge_subsurface = Some(subsurface);
    }

    Ok(())
}

fn create_toast_overlay(
    state: &mut AppState,
    output_id: u32,
    qh: &QueueHandle<AppState>,
) -> Result<()> {
    let Some(subcompositor) = state.globals.subcompositor.clone() else {
        return Ok(());
    };

    let compositor = state.globals.compositor()?;
    let shm = state.globals.shm()?;

    let buffer = ShmBuffer::create(
        &shm,
        qh,
        wl_shm::Format::Argb8888,
        TOAST_WIDTH,
        TOAST_HEIGHT,
        TOAST_WIDTH * 4,
    )?;

    let surface = compositor.create_surface(qh, ());
    let subsurface =
        subcompositor.get_subsurface(&surface, &window_surface(state, output_id)?, qh, ());
    make_surface_input_transparent(&compositor, &surface, qh);
    subsurface.set_desync();

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.toast_buffer = Some(buffer);
        window.toast_surface = Some(surface);
        window.toast_subsurface = Some(subsurface);
    }
    position_toast_subsurface(state, output_id);

    Ok(())
}

fn make_surface_input_transparent(
    compositor: &wayland_client::protocol::wl_compositor::WlCompositor,
    surface: &wl_surface::WlSurface,
    qh: &QueueHandle<AppState>,
) {
    let region = compositor.create_region(qh, ());
    surface.set_input_region(Some(&region));
    region.destroy();
}

type AnnotationRenderInputs = (
    Vec<AnnotationItem>,
    Option<ActiveAnnotation>,
    Option<TextAnnotation>,
    Option<render::OverlayCursor>,
);

fn build_annotation_render_inputs(
    state: &AppState,
    output_id: u32,
    width: f64,
    height: f64,
) -> Option<AnnotationRenderInputs> {
    let effective_tool = state.effective_annotation_tool();
    let show_cursor = cursor_visible_for_output(state, output_id);
    let window = state.windows.get(&output_id)?;
    let cursor = show_cursor.then_some(render::OverlayCursor {
        position: AnnotationPoint::new(window.pointer_x, window.pointer_y),
        style: if effective_tool == crate::state::AnnotationTool::Move {
            render::CursorStyle::Hand
        } else {
            render::CursorStyle::Crosshair
        },
    });
    let projected_annotations =
        project_annotations(&window.annotations, window.view_source, width, height);
    let projected_active = window
        .active_annotation
        .as_ref()
        .map(|a| project_active_annotation(a, window.view_source, width, height));
    let projected_text = window
        .active_text
        .as_ref()
        .map(|t| project_text_annotation(t, window.view_source, width, height));
    Some((
        projected_annotations,
        projected_active,
        projected_text,
        cursor,
    ))
}

fn flush_annotation_overlay(state: &mut AppState, output_id: u32) {
    let qh = match state.queue_handle.clone() {
        Some(qh) => qh,
        None => return,
    };
    let (width, height) = logical_size(state, output_id);
    if width <= 0 || height <= 0 {
        return;
    }

    let ready = state.windows.get(&output_id).is_some_and(|w| {
        w.annotation_visible && w.annotation_frame_callback.is_none() && w.annotation_redraw_pending
    });
    if !ready {
        return;
    }

    let surface = state
        .windows
        .get(&output_id)
        .and_then(|w| w.annotation_surface.as_ref().cloned());
    let Some(surface) = surface else {
        return;
    };

    let slot_index = state
        .windows
        .get(&output_id)
        .and_then(|w| w.annotation_buffers.iter().position(|s| !s.busy));
    let Some(slot_index) = slot_index else {
        return;
    };

    let Some((projected_annotations, projected_active, projected_text, cursor)) =
        build_annotation_render_inputs(state, output_id, width as f64, height as f64)
    else {
        return;
    };

    let Some(window) = state.windows.get_mut(&output_id) else {
        return;
    };
    let slot = &mut window.annotation_buffers[slot_index];

    render::draw_annotation_overlay(
        slot.buffer.data.as_mut(),
        width as usize,
        height as usize,
        &projected_annotations,
        projected_active.as_ref(),
        projected_text.as_ref(),
        cursor,
    );
    slot.busy = true;
    window.annotation_redraw_pending = false;

    let callback = surface.frame(&qh, AnnotationFrameKey { output_id });
    surface.attach(Some(&slot.buffer.wl_buffer), 0, 0);
    surface.damage(0, 0, width, height);
    surface.commit();

    tracing::trace!(output_id, slot_index, "committed annotation overlay frame");
    window.annotation_frame_callback = Some(callback);
}

fn refresh_spotlight_overlay(state: &mut AppState, output_id: u32) {
    let (width, height) = logical_size(state, output_id);
    let Some(window) = state.windows.get_mut(&output_id) else {
        return;
    };
    let Some(buffer) = window.spotlight_buffer.as_mut() else {
        return;
    };
    let Some(surface) = window.spotlight_surface.as_ref() else {
        return;
    };

    let radius = f64::from(width.min(height)) * state.spotlight_radius_frac;
    render::draw_spotlight_overlay(
        buffer.data.as_mut(),
        width as usize,
        height as usize,
        window.pointer_x.max(0.0) as usize,
        window.pointer_y.max(0.0) as usize,
        radius,
    );
    surface.attach(Some(&buffer.wl_buffer), 0, 0);
    surface.damage(0, 0, width, height);
    surface.commit();
}

fn refresh_zoom_badge_overlay(state: &mut AppState, output_id: u32) {
    let effective_tool = state.effective_annotation_tool();
    let badge = state.interaction_mode.badge(
        effective_tool,
        state.selected_palette_color(),
        state.text_annotation_scale,
        state.config.close_key,
    );
    let Some(window) = state.windows.get_mut(&output_id) else {
        return;
    };
    let Some(surface) = window.zoom_badge_surface.as_ref() else {
        return;
    };
    let Some(buffer) = window.zoom_badge_buffer.as_mut() else {
        return;
    };

    render::paint_zoom_badge(
        buffer.data.as_mut(),
        ZOOM_BADGE_WIDTH as usize,
        ZOOM_BADGE_HEIGHT as usize,
        &badge,
    );
    surface.attach(Some(&buffer.wl_buffer), 0, 0);
    surface.damage(0, 0, ZOOM_BADGE_WIDTH, ZOOM_BADGE_HEIGHT);
    surface.commit();
}

fn refresh_toast_overlay(state: &mut AppState, output_id: u32) {
    let message = state
        .toast
        .as_ref()
        .filter(|toast| toast.output_id == output_id)
        .map(|toast| toast.message.clone());
    let Some(message) = message else {
        return;
    };

    position_toast_subsurface(state, output_id);

    let Some(window) = state.windows.get_mut(&output_id) else {
        return;
    };
    let Some(surface) = window.toast_surface.as_ref() else {
        return;
    };
    let Some(buffer) = window.toast_buffer.as_mut() else {
        return;
    };

    render::paint_toast(
        buffer.data.as_mut(),
        TOAST_WIDTH as usize,
        TOAST_HEIGHT as usize,
        "SCREENSHOT SAVED",
        &message,
    );
    surface.attach(Some(&buffer.wl_buffer), 0, 0);
    surface.damage(0, 0, TOAST_WIDTH, TOAST_HEIGHT);
    surface.commit();
}

fn update_spotlight_overlay(state: &mut AppState, output_id: u32) {
    let should_show = state
        .windows
        .get(&output_id)
        .map(|window| {
            spotlight_overlay_should_show(
                window.spotlight_surface.is_some(),
                state.spotlight_enabled,
                state.interaction_mode.is_annotating(),
            )
        })
        .unwrap_or(false);

    set_spotlight_overlay_visible(state, output_id, should_show);
}

fn spotlight_overlay_should_show(
    has_surface: bool,
    spotlight_enabled: bool,
    annotating: bool,
) -> bool {
    has_surface && spotlight_enabled && !annotating
}

fn update_annotation_overlay(state: &mut AppState, output_id: u32) {
    let should_show = state
        .windows
        .get(&output_id)
        .map(|window| {
            window.annotation_surface.is_some()
                && (cursor_visible_for_output(state, output_id)
                    || state.interaction_mode.is_annotating()
                    || !window.annotations.is_empty()
                    || window.active_annotation.is_some()
                    || window.active_text.is_some())
        })
        .unwrap_or(false);

    set_annotation_overlay_visible(state, output_id, should_show);
    if should_show {
        refresh_annotation_overlay(state, output_id);
    }
}

fn update_zoom_badge_overlay(state: &mut AppState, output_id: u32) {
    let should_show = state
        .windows
        .get(&output_id)
        .map(|window| state.config.show_indicator && window.zoom_badge_surface.is_some())
        .unwrap_or(false);

    set_zoom_badge_visible(state, output_id, should_show);
    if should_show {
        refresh_zoom_badge_overlay(state, output_id);
    }
}

fn update_toast_overlay(state: &mut AppState, output_id: u32) {
    let should_show = state
        .windows
        .get(&output_id)
        .map(|window| {
            window.toast_surface.is_some()
                && state
                    .toast
                    .as_ref()
                    .is_some_and(|toast| toast.output_id == output_id)
        })
        .unwrap_or(false);

    set_toast_visible(state, output_id, should_show);
    if should_show {
        refresh_toast_overlay(state, output_id);
    }
}

fn set_spotlight_overlay_visible(state: &mut AppState, output_id: u32, show: bool) {
    let Some((was_visible, surface)) = state.windows.get(&output_id).map(|window| {
        (
            window.spotlight_visible,
            window.spotlight_surface.as_ref().cloned(),
        )
    }) else {
        return;
    };
    let Some(surface) = surface else {
        return;
    };

    if was_visible == show {
        return;
    }

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.spotlight_visible = show;
    }

    if show {
        refresh_spotlight_overlay(state, output_id);
    } else {
        let (width, height) = logical_size(state, output_id);
        commit_surface_hide(&surface, width, height);
    }
}

fn set_annotation_overlay_visible(state: &mut AppState, output_id: u32, show: bool) {
    let Some((was_visible, surface)) = state.windows.get(&output_id).map(|window| {
        (
            window.annotation_visible,
            window.annotation_surface.as_ref().cloned(),
        )
    }) else {
        return;
    };
    let Some(surface) = surface else {
        return;
    };

    if was_visible == show {
        return;
    }

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.annotation_visible = show;
        if !show {
            window.annotation_frame_callback = None;
            window.annotation_redraw_pending = false;
        }
    }

    if !show {
        let (width, height) = logical_size(state, output_id);
        commit_surface_hide(&surface, width, height);
    }
}

fn set_zoom_badge_visible(state: &mut AppState, output_id: u32, show: bool) {
    let Some((was_visible, surface)) = state.windows.get(&output_id).map(|window| {
        (
            window.zoom_badge_visible,
            window.zoom_badge_surface.as_ref().cloned(),
        )
    }) else {
        return;
    };
    let Some(surface) = surface else {
        return;
    };

    if was_visible == show {
        return;
    }

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.zoom_badge_visible = show;
    }

    if !show {
        commit_surface_hide(&surface, ZOOM_BADGE_WIDTH, ZOOM_BADGE_HEIGHT);
    }
}

fn set_toast_visible(state: &mut AppState, output_id: u32, show: bool) {
    let Some((was_visible, surface)) = state
        .windows
        .get(&output_id)
        .map(|window| (window.toast_visible, window.toast_surface.as_ref().cloned()))
    else {
        return;
    };
    let Some(surface) = surface else {
        return;
    };

    if was_visible == show {
        return;
    }

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.toast_visible = show;
    }

    if show {
        refresh_toast_overlay(state, output_id);
    } else {
        commit_surface_hide(&surface, TOAST_WIDTH, TOAST_HEIGHT);
    }
}

fn logical_size(state: &AppState, output_id: u32) -> (i32, i32) {
    state
        .outputs
        .get(&output_id)
        .map(|output| output.logical_size())
        .unwrap_or((0, 0))
}

fn commit_surface_hide(surface: &wl_surface::WlSurface, width: i32, height: i32) {
    surface.attach(None, 0, 0);
    surface.damage(0, 0, width, height);
    surface.commit();
}

fn position_toast_subsurface(state: &AppState, output_id: u32) {
    let (width, _) = logical_size(state, output_id);
    let x = (width - TOAST_WIDTH - ZOOM_BADGE_MARGIN).max(0);
    if let Some(window) = state.windows.get(&output_id)
        && let Some(subsurface) = window.toast_subsurface.as_ref()
    {
        subsurface.set_position(x, ZOOM_BADGE_MARGIN);
    }
}

fn cursor_visible_for_output(state: &AppState, output_id: u32) -> bool {
    state.focused_window == Some(output_id)
        || (state.focused_window.is_none() && state.windows.len() == 1)
}

pub fn draw_annotation_overlay_snapshot(
    state: &AppState,
    output_id: u32,
    pixels: &mut [u8],
    width: usize,
    height: usize,
) {
    let Some((projected_annotations, projected_active, projected_text, cursor)) =
        build_annotation_render_inputs(state, output_id, width as f64, height as f64)
    else {
        pixels.fill(0);
        return;
    };

    render::draw_annotation_overlay(
        pixels,
        width,
        height,
        &projected_annotations,
        projected_active.as_ref(),
        projected_text.as_ref(),
        cursor,
    );
}

fn project_annotations(
    annotations: &[AnnotationItem],
    view_source: ViewRect,
    logical_width: f64,
    logical_height: f64,
) -> Vec<AnnotationItem> {
    annotations
        .iter()
        .map(|annotation| match annotation {
            AnnotationItem::Stroke(stroke) => AnnotationItem::Stroke(project_stroke(
                stroke,
                view_source,
                logical_width,
                logical_height,
            )),
            AnnotationItem::Shape(shape) => AnnotationItem::Shape(project_shape(
                shape,
                view_source,
                logical_width,
                logical_height,
            )),
            AnnotationItem::Text(text) => AnnotationItem::Text(project_text_annotation(
                text,
                view_source,
                logical_width,
                logical_height,
            )),
        })
        .collect()
}

fn project_active_annotation(
    annotation: &ActiveAnnotation,
    view_source: ViewRect,
    logical_width: f64,
    logical_height: f64,
) -> ActiveAnnotation {
    match annotation {
        ActiveAnnotation::Stroke(stroke) => ActiveAnnotation::Stroke(project_stroke(
            stroke,
            view_source,
            logical_width,
            logical_height,
        )),
        ActiveAnnotation::Shape(shape) => ActiveAnnotation::Shape(project_shape(
            shape,
            view_source,
            logical_width,
            logical_height,
        )),
    }
}

fn project_stroke(
    stroke: &StrokeAnnotation,
    view_source: ViewRect,
    logical_width: f64,
    logical_height: f64,
) -> StrokeAnnotation {
    StrokeAnnotation {
        points: stroke
            .points
            .iter()
            .copied()
            .map(|point| project_point(point, view_source, logical_width, logical_height))
            .collect(),
        color: stroke.color,
        width: stroke.width,
    }
}

fn project_shape(
    shape: &ShapeAnnotation,
    view_source: ViewRect,
    logical_width: f64,
    logical_height: f64,
) -> ShapeAnnotation {
    ShapeAnnotation {
        kind: shape.kind,
        start: project_point(shape.start, view_source, logical_width, logical_height),
        end: project_point(shape.end, view_source, logical_width, logical_height),
        color: shape.color,
        width: shape.width,
    }
}

fn project_text_annotation(
    text: &TextAnnotation,
    view_source: ViewRect,
    logical_width: f64,
    logical_height: f64,
) -> TextAnnotation {
    TextAnnotation {
        position: project_point(text.position, view_source, logical_width, logical_height),
        text: text.text.clone(),
        color: text.color,
        scale: text.scale,
    }
}

fn project_point(
    point: AnnotationPoint,
    view_source: ViewRect,
    logical_width: f64,
    logical_height: f64,
) -> AnnotationPoint {
    if view_source.width <= 0.0
        || view_source.height <= 0.0
        || logical_width <= 0.0
        || logical_height <= 0.0
    {
        return point;
    }

    AnnotationPoint::new(
        ((point.x as f64 - view_source.x) / view_source.width) * logical_width,
        ((point.y as f64 - view_source.y) / view_source.height) * logical_height,
    )
}

fn window_surface(state: &AppState, output_id: u32) -> Result<wl_surface::WlSurface> {
    state
        .windows
        .get(&output_id)
        .map(|window| window.surface.clone())
        .ok_or_else(|| {
            AppError::runtime(format!("window {output_id} is not available for overlays"))
        })
}

impl Dispatch<wl_buffer::WlBuffer, AnnotationBufferKey> for AppState {
    fn event(
        state: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        key: &AnnotationBufferKey,
        _: &wayland_client::Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            if let Some(window) = state.windows.get_mut(&key.output_id)
                && let Some(slot) = window.annotation_buffers.get_mut(key.slot)
            {
                slot.busy = false;
            }
            flush_annotation_overlay(state, key.output_id);
        }
    }
}

impl Dispatch<wl_callback::WlCallback, AnnotationFrameKey> for AppState {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        key: &AnnotationFrameKey,
        _: &wayland_client::Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            if let Some(window) = state.windows.get_mut(&key.output_id) {
                window.annotation_frame_callback = None;
            }
            flush_annotation_overlay(state, key.output_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{state::AnnotationPoint, zoom::ViewRect};

    use super::{project_point, spotlight_overlay_should_show};

    #[test]
    fn project_point_tracks_view_source_changes() {
        let zoomed = ViewRect {
            x: 100.0,
            y: 50.0,
            width: 400.0,
            height: 200.0,
        };
        let full = ViewRect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 400.0,
        };
        let stored = AnnotationPoint { x: 300, y: 150 };

        let zoomed_screen = project_point(stored, zoomed, 800.0, 400.0);
        let full_screen = project_point(stored, full, 800.0, 400.0);

        assert_eq!(zoomed_screen, AnnotationPoint { x: 400, y: 200 });
        assert_eq!(full_screen, AnnotationPoint { x: 300, y: 150 });
    }

    #[test]
    fn spotlight_overlay_can_show_without_zoom() {
        assert!(spotlight_overlay_should_show(true, true, false));
    }

    #[test]
    fn spotlight_overlay_requires_surface_and_navigation_mode() {
        assert!(!spotlight_overlay_should_show(false, true, false));
        assert!(!spotlight_overlay_should_show(true, false, false));
        assert!(!spotlight_overlay_should_show(true, true, true));
    }
}

delegate_noop!(AppState: ignore wl_region::WlRegion);
delegate_noop!(AppState: ignore wl_subsurface::WlSubsurface);
