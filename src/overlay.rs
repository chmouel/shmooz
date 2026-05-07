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
        StrokeAnnotation, TextAnnotation,
    },
    window::{self, OverlayBufferSlot},
    zoom::ViewRect,
};

const ANNOTATION_BUFFER_COUNT: usize = 3;
const SPOTLIGHT_MOVE_THRESHOLD_SQ: f64 = 16.0;
const ZOOM_BADGE_WIDTH: i32 = 480;
const ZOOM_BADGE_HEIGHT: i32 = 136;
const ZOOM_BADGE_MARGIN: i32 = 24;

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
    update_window_overlays(state, output_id);

    Ok(())
}

pub fn update_window_overlays(state: &mut AppState, output_id: u32) {
    update_spotlight_overlay(state, output_id);
    update_annotation_overlay(state, output_id);
    update_zoom_badge_overlay(state, output_id);
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
    if !state.config.show_indicator || state.globals.subcompositor.is_none() {
        return Ok(());
    }

    let compositor = state.globals.compositor()?;
    let shm = state.globals.shm()?;
    let subcompositor = state
        .globals
        .subcompositor
        .clone()
        .ok_or_else(|| AppError::missing_protocol("wl_subcompositor"))?;

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

fn make_surface_input_transparent(
    compositor: &wayland_client::protocol::wl_compositor::WlCompositor,
    surface: &wl_surface::WlSurface,
    qh: &QueueHandle<AppState>,
) {
    let region = compositor.create_region(qh, ());
    surface.set_input_region(Some(&region));
    region.destroy();
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
    let show_cursor = state.interaction_mode.is_annotating();

    let Some(window) = state.windows.get_mut(&output_id) else {
        return;
    };
    if !window.annotation_visible
        || window.annotation_frame_callback.is_some()
        || !window.annotation_redraw_pending
    {
        return;
    }

    let Some(surface) = window.annotation_surface.as_ref().cloned() else {
        return;
    };
    let Some((slot_index, slot)) = window
        .annotation_buffers
        .iter_mut()
        .enumerate()
        .find(|(_, slot)| !slot.busy)
    else {
        return;
    };
    let cursor = show_cursor.then_some(AnnotationPoint::new(window.pointer_x, window.pointer_y));
    let projected_annotations = project_annotations(
        &window.annotations,
        window.view_source,
        width as f64,
        height as f64,
    );
    let projected_active = window.active_annotation.as_ref().map(|annotation| {
        project_active_annotation(annotation, window.view_source, width as f64, height as f64)
    });
    let projected_text = window
        .active_text
        .as_ref()
        .map(|text| project_text_annotation(text, window.view_source, width as f64, height as f64));

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
    let title = state.interaction_mode.badge_title(state.annotation_tool);
    let hints = state.interaction_mode.badge_hints();
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
        &title,
        hints,
    );
    surface.attach(Some(&buffer.wl_buffer), 0, 0);
    surface.damage(0, 0, ZOOM_BADGE_WIDTH, ZOOM_BADGE_HEIGHT);
    surface.commit();
}

fn update_spotlight_overlay(state: &mut AppState, output_id: u32) {
    let should_show = state
        .windows
        .get(&output_id)
        .map(|window| {
            window.spotlight_surface.is_some()
                && window::is_zoomed(window)
                && state.spotlight_enabled
                && !state.interaction_mode.is_annotating()
        })
        .unwrap_or(false);

    set_spotlight_overlay_visible(state, output_id, should_show);
}

fn update_annotation_overlay(state: &mut AppState, output_id: u32) {
    let should_show = state
        .windows
        .get(&output_id)
        .map(|window| {
            window.annotation_surface.is_some()
                && (state.interaction_mode.is_annotating()
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
        surface.attach(None, 0, 0);
        let (width, height) = logical_size(state, output_id);
        surface.damage(0, 0, width, height);
        surface.commit();
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
        surface.attach(None, 0, 0);
        let (width, height) = logical_size(state, output_id);
        surface.damage(0, 0, width, height);
        surface.commit();
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
        surface.attach(None, 0, 0);
        surface.damage(0, 0, ZOOM_BADGE_WIDTH, ZOOM_BADGE_HEIGHT);
        surface.commit();
    }
}

fn logical_size(state: &AppState, output_id: u32) -> (i32, i32) {
    state
        .outputs
        .get(&output_id)
        .map(|output| output.logical_size())
        .unwrap_or((0, 0))
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
                && let Some(slot) = window.annotation_buffers.get_mut(key.slot) {
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

    use super::project_point;

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
}

delegate_noop!(AppState: ignore wl_region::WlRegion);
delegate_noop!(AppState: ignore wl_subsurface::WlSubsurface);
