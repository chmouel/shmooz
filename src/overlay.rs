use std::time::{Duration, Instant};

use wayland_client::{
    Dispatch, QueueHandle, delegate_noop,
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_region, wl_shm, wl_subcompositor, wl_subsurface,
        wl_surface,
    },
};

use crate::{
    error::{AppError, Result},
    render::{self, AnnotationScene, CursorStyle, OverlayCursor},
    screenshot,
    shm::ShmBuffer,
    state::{AnnotationPoint, AnnotationTool, AppState, InteractionMode, ToastState},
    window::{OverlayBufferSlot, OverlayLayer, WindowState},
    zoom::{Size, ViewRect, source_to_screen},
};

const ANNOTATION_BUFFER_COUNT: usize = 3;
const SPOTLIGHT_MOVE_THRESHOLD_SQ: f64 = 16.0;
const ZOOM_BADGE_WIDTH: i32 = 640;
const ZOOM_BADGE_HEIGHT: i32 = 136;
const ZOOM_BADGE_MARGIN: i32 = 24;
const TOAST_WIDTH: i32 = ZOOM_BADGE_WIDTH;
const TOAST_HEIGHT: i32 = ZOOM_BADGE_HEIGHT;
const TOAST_DURATION: Duration = Duration::from_secs(2);
const COLOR_PICKER_WIDTH: i32 = 320;
const COLOR_PICKER_HEIGHT: i32 = 112;

#[derive(Clone, Copy)]
struct AnnotationBufferKey {
    output_id: u32,
    slot: usize,
}

#[derive(Clone, Copy)]
struct AnnotationFrameKey {
    output_id: u32,
}

#[derive(Clone, Copy)]
enum Layer {
    Spotlight,
    ZoomBadge,
    Toast,
    ColorPicker,
    Help,
}

fn layer(window: &WindowState, layer: Layer) -> Option<&OverlayLayer> {
    match layer {
        Layer::Spotlight => window.spotlight.as_ref(),
        Layer::ZoomBadge => window.zoom_badge.as_ref(),
        Layer::Toast => window.toast.as_ref(),
        Layer::ColorPicker => window.color_picker.as_ref(),
        Layer::Help => window.help.as_ref(),
    }
}

fn layer_mut(window: &mut WindowState, layer: Layer) -> Option<&mut OverlayLayer> {
    match layer {
        Layer::Spotlight => window.spotlight.as_mut(),
        Layer::ZoomBadge => window.zoom_badge.as_mut(),
        Layer::Toast => window.toast.as_mut(),
        Layer::ColorPicker => window.color_picker.as_mut(),
        Layer::Help => window.help.as_mut(),
    }
}

struct SubsurfaceFactory<'a> {
    compositor: wl_compositor::WlCompositor,
    subcompositor: wl_subcompositor::WlSubcompositor,
    shm: wl_shm::WlShm,
    parent: wl_surface::WlSurface,
    qh: &'a QueueHandle<AppState>,
}

impl SubsurfaceFactory<'_> {
    fn surface(&self, x: i32, y: i32) -> (wl_surface::WlSurface, wl_subsurface::WlSubsurface) {
        let surface = self.compositor.create_surface(self.qh, ());
        let subsurface = self
            .subcompositor
            .get_subsurface(&surface, &self.parent, self.qh, ());
        let region = self.compositor.create_region(self.qh, ());
        surface.set_input_region(Some(&region));
        region.destroy();
        subsurface.set_position(x, y);
        subsurface.set_desync();
        (surface, subsurface)
    }

    fn buffer<U: Send + Sync + 'static>(
        &self,
        width: i32,
        height: i32,
        user_data: U,
    ) -> Result<ShmBuffer>
    where
        AppState: Dispatch<wl_buffer::WlBuffer, U>,
    {
        ShmBuffer::create_with_data(
            &self.shm,
            self.qh,
            wl_shm::Format::Argb8888,
            width,
            height,
            width * 4,
            user_data,
        )
    }

    fn layer(&self, width: i32, height: i32, x: i32, y: i32) -> Result<OverlayLayer> {
        let buffer = self.buffer(width, height, ())?;
        let (surface, subsurface) = self.surface(x, y);
        Ok(OverlayLayer {
            surface,
            subsurface,
            buffer,
            visible: false,
        })
    }
}

/// Creates the overlay subsurfaces. Creation order is the stacking order.
pub fn create_overlays_for_window(
    state: &mut AppState,
    output_id: u32,
    qh: &QueueHandle<AppState>,
) -> Result<()> {
    let Some(subcompositor) = state.globals.subcompositor.clone() else {
        return Ok(());
    };
    let factory = SubsurfaceFactory {
        compositor: state.globals.compositor()?,
        subcompositor,
        shm: state.globals.shm()?,
        parent: window_surface(state, output_id)?,
        qh,
    };
    let (width, height) = logical_size(state, output_id);
    let has_size = width > 0 && height > 0;

    let spotlight = has_size
        .then(|| factory.layer(width, height, 0, 0))
        .transpose()?;
    let annotation = if has_size {
        let (surface, subsurface) = factory.surface(0, 0);
        let buffers = (0..ANNOTATION_BUFFER_COUNT)
            .map(|slot| {
                Ok(OverlayBufferSlot {
                    buffer: factory.buffer(
                        width,
                        height,
                        AnnotationBufferKey { output_id, slot },
                    )?,
                    busy: false,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Some((surface, subsurface, buffers))
    } else {
        None
    };
    let zoom_badge = state
        .config
        .show_indicator
        .then(|| {
            factory.layer(
                ZOOM_BADGE_WIDTH,
                ZOOM_BADGE_HEIGHT,
                ZOOM_BADGE_MARGIN,
                ZOOM_BADGE_MARGIN,
            )
        })
        .transpose()?;
    let toast = factory.layer(TOAST_WIDTH, TOAST_HEIGHT, 0, 0)?;
    let color_picker = factory.layer(COLOR_PICKER_WIDTH, COLOR_PICKER_HEIGHT, 0, 0)?;
    let help = has_size
        .then(|| factory.layer(width, height, 0, 0))
        .transpose()?;

    if let Some(window) = state.windows.get_mut(&output_id) {
        window.spotlight = spotlight;
        if let Some((surface, subsurface, buffers)) = annotation {
            window.annotation_surface = Some(surface);
            window.annotation_subsurface = Some(subsurface);
            window.annotation_buffers = buffers;
        }
        window.zoom_badge = zoom_badge;
        window.toast = Some(toast);
        window.color_picker = Some(color_picker);
        window.help = help;
    }
    position_toast_subsurface(state, output_id);
    position_color_picker_subsurface(state, output_id);
    update_window_overlays(state, output_id);

    Ok(())
}

pub fn update_window_overlays(state: &mut AppState, output_id: u32) {
    update_spotlight_overlay(state, output_id);
    update_annotation_overlay(state, output_id);
    update_zoom_badge_overlay(state, output_id);
    update_toast_overlay(state, output_id);
    update_color_picker_overlay(state, output_id);
    update_help_overlay(state, output_id);
}

fn for_each_window(state: &mut AppState, f: impl Fn(&mut AppState, u32)) {
    for output_id in state.window_ids() {
        f(state, output_id);
    }
}

pub fn update_spotlight_overlays(state: &mut AppState) {
    for_each_window(state, update_spotlight_overlay);
}

pub fn update_annotation_overlays(state: &mut AppState) {
    for_each_window(state, update_annotation_overlay);
}

pub fn update_color_picker_overlays(state: &mut AppState) {
    for_each_window(state, update_color_picker_overlay);
}

pub fn update_help_overlays(state: &mut AppState) {
    for_each_window(state, update_help_overlay);
}

pub fn refresh_zoom_badge_overlays(state: &mut AppState) {
    for_each_window(state, refresh_zoom_badge_overlay);
}

pub fn refresh_visible_spotlight_overlays(state: &mut AppState) {
    for_each_window(state, |state, output_id| {
        if layer_visible(state, output_id, Layer::Spotlight) {
            refresh_spotlight_overlay(state, output_id);
        }
    });
}

pub fn refresh_spotlight_for_motion(
    state: &mut AppState,
    output_id: u32,
    delta_x: f64,
    delta_y: f64,
) {
    if delta_x * delta_x + delta_y * delta_y >= SPOTLIGHT_MOVE_THRESHOLD_SQ
        && layer_visible(state, output_id, Layer::Spotlight)
    {
        refresh_spotlight_overlay(state, output_id);
    }
}

pub fn refresh_annotation_overlay(state: &mut AppState, output_id: u32) {
    if let Some(window) = state.windows.get_mut(&output_id) {
        window.annotation_redraw_pending = true;
    }
    flush_annotation_overlay(state, output_id);
}

pub fn show_toast(state: &mut AppState, output_id: u32, message: impl Into<String>) {
    let previous_output = state.toast.as_ref().map(|toast| toast.output_id);
    state.toast = Some(ToastState {
        output_id,
        message: message.into(),
        expires_at: Instant::now() + TOAST_DURATION,
    });

    if let Some(previous_output) = previous_output.filter(|previous| *previous != output_id) {
        set_layer_visible(state, previous_output, Layer::Toast, false);
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
        set_layer_visible(state, output_id, Layer::Toast, false);
    }
}

fn layer_visible(state: &AppState, output_id: u32, kind: Layer) -> bool {
    state
        .windows
        .get(&output_id)
        .and_then(|window| layer(window, kind))
        .is_some_and(|layer| layer.visible)
}

/// Returns whether the visibility changed. Hiding commits a null buffer.
fn set_layer_visible(state: &mut AppState, output_id: u32, kind: Layer, show: bool) -> bool {
    let Some(layer) = state
        .windows
        .get_mut(&output_id)
        .and_then(|window| layer_mut(window, kind))
    else {
        return false;
    };
    if layer.visible == show {
        return false;
    }

    layer.visible = show;
    if !show {
        commit_surface_hide(&layer.surface, layer.buffer.width, layer.buffer.height);
    }
    true
}

fn paint_layer(
    state: &mut AppState,
    output_id: u32,
    kind: Layer,
    paint: impl FnOnce(&mut [u8], usize, usize),
) {
    let Some(layer) = state
        .windows
        .get_mut(&output_id)
        .and_then(|window| layer_mut(window, kind))
    else {
        return;
    };
    let (width, height) = (layer.buffer.width, layer.buffer.height);

    paint(layer.buffer.data.as_mut(), width as usize, height as usize);
    layer.surface.attach(Some(&layer.buffer.wl_buffer), 0, 0);
    layer.surface.damage(0, 0, width, height);
    layer.surface.commit();
}

fn update_spotlight_overlay(state: &mut AppState, output_id: u32) {
    let show = spotlight_overlay_should_show(
        state.spotlight_enabled,
        state.interaction_mode.is_annotating(),
    );
    if set_layer_visible(state, output_id, Layer::Spotlight, show) && show {
        refresh_spotlight_overlay(state, output_id);
    }
}

fn spotlight_overlay_should_show(spotlight_enabled: bool, annotating: bool) -> bool {
    spotlight_enabled && !annotating
}

fn refresh_spotlight_overlay(state: &mut AppState, output_id: u32) {
    let Some(window) = state.windows.get(&output_id) else {
        return;
    };
    let center_x = window.pointer_x.max(0.0) as usize;
    let center_y = window.pointer_y.max(0.0) as usize;
    let radius_frac = state.spotlight_radius_frac;

    paint_layer(
        state,
        output_id,
        Layer::Spotlight,
        |pixels, width, height| {
            let radius = width.min(height) as f64 * radius_frac;
            render::draw_spotlight_overlay(pixels, width, height, center_x, center_y, radius);
        },
    );
}

fn update_zoom_badge_overlay(state: &mut AppState, output_id: u32) {
    let show = state.config.show_indicator;
    set_layer_visible(state, output_id, Layer::ZoomBadge, show);
    if show {
        refresh_zoom_badge_overlay(state, output_id);
    }
}

fn refresh_zoom_badge_overlay(state: &mut AppState, output_id: u32) {
    let spotlight_radius_pct = state
        .spotlight_enabled
        .then(|| (state.spotlight_radius_frac * 100.0).round() as u32);
    let badge = state.interaction_mode.badge(
        state.effective_annotation_tool(),
        state.selected_palette_color(),
        state.text_annotation_scale,
        state.config.close_key,
        spotlight_radius_pct,
    );

    paint_layer(
        state,
        output_id,
        Layer::ZoomBadge,
        |pixels, width, height| {
            render::paint_zoom_badge(pixels, width, height, &badge);
        },
    );
}

fn update_toast_overlay(state: &mut AppState, output_id: u32) {
    let show = state
        .toast
        .as_ref()
        .is_some_and(|toast| toast.output_id == output_id);
    set_layer_visible(state, output_id, Layer::Toast, show);
    if show {
        refresh_toast_overlay(state, output_id);
    }
}

fn refresh_toast_overlay(state: &mut AppState, output_id: u32) {
    let Some(message) = state
        .toast
        .as_ref()
        .filter(|toast| toast.output_id == output_id)
        .map(|toast| toast.message.clone())
    else {
        return;
    };

    position_toast_subsurface(state, output_id);
    paint_layer(state, output_id, Layer::Toast, |pixels, width, height| {
        render::paint_toast(pixels, width, height, "SCREENSHOT SAVED", &message);
    });
}

fn update_color_picker_overlay(state: &mut AppState, output_id: u32) {
    let show = state.interaction_mode == InteractionMode::ColorPicker
        && cursor_visible_for_output(state, output_id);
    set_layer_visible(state, output_id, Layer::ColorPicker, show);
    if show {
        refresh_color_picker_overlay(state, output_id);
    }
}

pub fn refresh_color_picker_overlay(state: &mut AppState, output_id: u32) {
    let Ok(color) = screenshot::output_color_value_at_pointer(state, output_id) else {
        return;
    };
    let hex = screenshot::rgb_hex(color);
    let copied = state.color_picker_copied;

    position_color_picker_subsurface(state, output_id);
    paint_layer(
        state,
        output_id,
        Layer::ColorPicker,
        |pixels, width, height| {
            render::paint_color_picker(pixels, width, height, color, &hex, copied);
        },
    );
}

fn update_help_overlay(state: &mut AppState, output_id: u32) {
    let show = state.help_visible;
    set_layer_visible(state, output_id, Layer::Help, show);
    if show {
        let close_key = state.config.close_key;
        paint_layer(state, output_id, Layer::Help, |pixels, width, height| {
            render::paint_help_overlay(pixels, width, height, close_key);
        });
    }
}

fn update_annotation_overlay(state: &mut AppState, output_id: u32) {
    let should_show = state.windows.get(&output_id).is_some_and(|window| {
        window.annotation_surface.is_some()
            && (cursor_visible_for_output(state, output_id)
                || state.interaction_mode.is_annotating()
                || !window.annotations.is_empty()
                || window.active_annotation.is_some()
                || window.active_text.is_some()
                || window.shift_select_start.is_some())
    });

    set_annotation_overlay_visible(state, output_id, should_show);
    if should_show {
        refresh_annotation_overlay(state, output_id);
    }
}

fn set_annotation_overlay_visible(state: &mut AppState, output_id: u32, show: bool) {
    let (width, height) = logical_size(state, output_id);
    let Some(window) = state.windows.get_mut(&output_id) else {
        return;
    };
    let Some(surface) = window.annotation_surface.as_ref() else {
        return;
    };
    if window.annotation_visible == show {
        return;
    }

    window.annotation_visible = show;
    if !show {
        window.annotation_frame_callback = None;
        window.annotation_redraw_pending = false;
        commit_surface_hide(surface, width, height);
    }
}

fn annotation_scene(
    state: &AppState,
    output_id: u32,
    width: f64,
    height: f64,
) -> Option<AnnotationScene> {
    let window = state.windows.get(&output_id)?;
    let screen = Size { width, height };
    let project = |point| project_point(point, window.view_source, screen);
    let pointer = AnnotationPoint::new(window.pointer_x, window.pointer_y);
    let cursor = cursor_visible_for_output(state, output_id).then(|| OverlayCursor {
        position: pointer,
        style: if state.effective_annotation_tool() == AnnotationTool::Move {
            CursorStyle::Hand
        } else {
            CursorStyle::Crosshair
        },
    });

    Some(AnnotationScene {
        annotations: window
            .annotations
            .iter()
            .map(|annotation| {
                let mut annotation = annotation.clone();
                annotation.map_points(project);
                annotation
            })
            .collect(),
        active_annotation: window.active_annotation.clone().map(|mut annotation| {
            annotation.map_points(project);
            annotation
        }),
        active_text: window.active_text.clone().map(|mut text| {
            text.map_points(project);
            text
        }),
        cursor,
        shift_select_rect: window
            .shift_select_start
            .map(|start| (project(start), pointer)),
    })
}

fn flush_annotation_overlay(state: &mut AppState, output_id: u32) {
    let Some(qh) = state.queue_handle.clone() else {
        return;
    };
    let (width, height) = logical_size(state, output_id);
    if width <= 0 || height <= 0 {
        return;
    }

    let Some(window) = state.windows.get(&output_id) else {
        return;
    };
    if !window.annotation_visible
        || window.annotation_frame_callback.is_some()
        || !window.annotation_redraw_pending
    {
        return;
    }
    let Some(surface) = window.annotation_surface.clone() else {
        return;
    };
    let Some(slot_index) = window.annotation_buffers.iter().position(|s| !s.busy) else {
        return;
    };
    let Some(scene) = annotation_scene(state, output_id, f64::from(width), f64::from(height))
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
        &scene,
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

pub fn draw_annotation_overlay_snapshot(
    state: &AppState,
    output_id: u32,
    pixels: &mut [u8],
    width: usize,
    height: usize,
) {
    match annotation_scene(state, output_id, width as f64, height as f64) {
        Some(scene) => render::draw_annotation_overlay(pixels, width, height, &scene),
        None => pixels.fill(0),
    }
}

fn logical_size(state: &AppState, output_id: u32) -> (i32, i32) {
    state
        .outputs
        .get(&output_id)
        .map_or((0, 0), |output| output.logical_size())
}

fn commit_surface_hide(surface: &wl_surface::WlSurface, width: i32, height: i32) {
    surface.attach(None, 0, 0);
    surface.damage(0, 0, width, height);
    surface.commit();
}

fn position_toast_subsurface(state: &AppState, output_id: u32) {
    let (width, _) = logical_size(state, output_id);
    let x = (width - TOAST_WIDTH - ZOOM_BADGE_MARGIN).max(0);
    let y = if state.interaction_mode == InteractionMode::ColorPicker {
        ZOOM_BADGE_MARGIN + COLOR_PICKER_HEIGHT + 8
    } else {
        ZOOM_BADGE_MARGIN
    };
    if let Some(toast) = state
        .windows
        .get(&output_id)
        .and_then(|window| window.toast.as_ref())
    {
        toast.subsurface.set_position(x, y);
    }
}

fn position_color_picker_subsurface(state: &AppState, output_id: u32) {
    let (width, _) = logical_size(state, output_id);
    let x = (width - COLOR_PICKER_WIDTH - ZOOM_BADGE_MARGIN).max(0);
    if let Some(color_picker) = state
        .windows
        .get(&output_id)
        .and_then(|window| window.color_picker.as_ref())
    {
        color_picker.subsurface.set_position(x, ZOOM_BADGE_MARGIN);
    }
}

fn cursor_visible_for_output(state: &AppState, output_id: u32) -> bool {
    state.focused_window == Some(output_id)
        || (state.focused_window.is_none() && state.windows.len() == 1)
}

fn project_point(point: AnnotationPoint, view_source: ViewRect, screen: Size) -> AnnotationPoint {
    let point = source_to_screen(point.into(), view_source, screen);
    AnnotationPoint::new(point.x, point.y)
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
    use crate::{
        state::AnnotationPoint,
        zoom::{Size, ViewRect},
    };

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
        let screen = Size {
            width: 800.0,
            height: 400.0,
        };
        let stored = AnnotationPoint { x: 300, y: 150 };

        let zoomed_screen = project_point(stored, zoomed, screen);
        let full_screen = project_point(stored, full, screen);

        assert_eq!(zoomed_screen, AnnotationPoint { x: 400, y: 200 });
        assert_eq!(full_screen, AnnotationPoint { x: 300, y: 150 });
    }

    #[test]
    fn spotlight_overlay_can_show_without_zoom() {
        assert!(spotlight_overlay_should_show(true, false));
    }

    #[test]
    fn spotlight_overlay_requires_navigation_mode() {
        assert!(!spotlight_overlay_should_show(false, false));
        assert!(!spotlight_overlay_should_show(true, true));
    }
}

delegate_noop!(AppState: ignore wl_region::WlRegion);
delegate_noop!(AppState: ignore wl_subsurface::WlSubsurface);
