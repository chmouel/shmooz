use std::time::{Duration, Instant};

use wayland_client::{
    Dispatch, QueueHandle, delegate_noop,
    protocol::{wl_callback, wl_subsurface, wl_surface},
};
use wayland_protocols::wp::viewporter::client::wp_viewport;
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

use crate::{
    error::{AppError, Result},
    overlay,
    shm::ShmBuffer,
    state::AppState,
    state::{ActiveAnnotation, ActiveMove, AnnotationItem, AnnotationPoint, TextAnnotation},
    zoom::{
        Size, ViewRect, apply_zoom, aspect_ratio, clamp_view, ease_out_cubic, interpolate_view,
        view_rect_nearly_equal,
    },
};

pub const APP_TITLE: &str = "shmooz";
pub const ZOOM_ANIMATION_DURATION: Duration = Duration::from_millis(140);

pub struct OverlayBufferSlot {
    pub buffer: ShmBuffer,
    pub busy: bool,
}

pub struct WindowState {
    #[allow(dead_code)]
    pub output_id: u32,
    pub surface: wl_surface::WlSurface,
    pub viewport: wp_viewport::WpViewport,
    #[allow(dead_code)]
    pub layer_surface: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    pub spotlight_surface: Option<wl_surface::WlSurface>,
    pub spotlight_subsurface: Option<wl_subsurface::WlSubsurface>,
    pub spotlight_buffer: Option<ShmBuffer>,
    pub spotlight_visible: bool,
    pub annotation_surface: Option<wl_surface::WlSurface>,
    pub annotation_subsurface: Option<wl_subsurface::WlSubsurface>,
    pub annotation_buffers: Vec<OverlayBufferSlot>,
    pub annotation_frame_callback: Option<wl_callback::WlCallback>,
    pub annotation_redraw_pending: bool,
    pub annotation_visible: bool,
    pub zoom_badge_surface: Option<wl_surface::WlSurface>,
    pub zoom_badge_subsurface: Option<wl_subsurface::WlSubsurface>,
    pub zoom_badge_buffer: Option<ShmBuffer>,
    pub zoom_badge_visible: bool,
    pub toast_surface: Option<wl_surface::WlSurface>,
    pub toast_subsurface: Option<wl_subsurface::WlSubsurface>,
    pub toast_buffer: Option<ShmBuffer>,
    pub toast_visible: bool,
    pub color_picker_surface: Option<wl_surface::WlSurface>,
    pub color_picker_subsurface: Option<wl_subsurface::WlSubsurface>,
    pub color_picker_buffer: Option<ShmBuffer>,
    pub color_picker_visible: bool,
    pub help_surface: Option<wl_surface::WlSurface>,
    pub help_subsurface: Option<wl_subsurface::WlSubsurface>,
    pub help_buffer: Option<ShmBuffer>,
    pub help_visible: bool,
    pub annotations: Vec<AnnotationItem>,
    pub active_annotation: Option<ActiveAnnotation>,
    pub active_move: Option<ActiveMove>,
    pub active_text: Option<TextAnnotation>,
    pub shift_select_start: Option<AnnotationPoint>,
    pub view_source: ViewRect,
    pub initial_view_source: ViewRect,
    pub zoom_animation: Option<ZoomAnimation>,
    pub pointer_x: f64,
    pub pointer_y: f64,
    pub pointer_pressed: bool,
    pub last_click_time: u32,
    pub last_click_button: u32,
    pub configured_width: i32,
    pub configured_height: i32,
    pub is_configured: bool,
    pub initial_zoom_applied: bool,
}

pub struct ZoomAnimation {
    pub start: ViewRect,
    pub target: ViewRect,
    pub started_at: Instant,
    pub duration: Duration,
}

pub fn create_window_for_output(
    state: &mut AppState,
    output_id: u32,
    qh: &QueueHandle<AppState>,
) -> Result<()> {
    if state.windows.contains_key(&output_id) {
        return Ok(());
    }

    let compositor = state.globals.compositor()?;
    let layer_shell = state.globals.layer_shell()?;
    let viewporter = state.globals.viewporter()?;
    let wl_output = state
        .outputs
        .get(&output_id)
        .and_then(|output| output.wl_output.as_ref())
        .cloned()
        .ok_or_else(|| {
            AppError::runtime(format!(
                "output {output_id} is no longer available for window creation"
            ))
        })?;

    let surface = compositor.create_surface(qh, ());
    let viewport = viewporter.get_viewport(&surface, qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        Some(&wl_output),
        zwlr_layer_shell_v1::Layer::Overlay,
        APP_TITLE.to_owned(),
        qh,
        output_id,
    );
    let (bw, bh) = state
        .outputs
        .get(&output_id)
        .and_then(|output| output.buffer_dimensions())
        .ok_or_else(|| {
            AppError::runtime(format!(
                "output {output_id} has no captured buffer for window creation"
            ))
        })?;
    let buffer_size = Size {
        width: bw as f64,
        height: bh as f64,
    };
    let initial_view_source = ViewRect::full(buffer_size);
    let logical_size = state
        .outputs
        .get(&output_id)
        .map(|output| {
            let (w, h) = output.logical_size();
            Size {
                width: w as f64,
                height: h as f64,
            }
        })
        .unwrap_or_default();

    layer_surface.set_anchor(
        zwlr_layer_surface_v1::Anchor::Top
            | zwlr_layer_surface_v1::Anchor::Bottom
            | zwlr_layer_surface_v1::Anchor::Left
            | zwlr_layer_surface_v1::Anchor::Right,
    );
    layer_surface.set_size(0, 0);
    layer_surface.set_exclusive_zone(-1);
    layer_surface
        .set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive);

    tracing::info!(output_id, "creating layer shell overlay window");

    state.windows.insert(
        output_id,
        WindowState {
            output_id,
            surface,
            viewport,
            layer_surface,
            spotlight_surface: None,
            spotlight_subsurface: None,
            spotlight_buffer: None,
            spotlight_visible: false,
            annotation_surface: None,
            annotation_subsurface: None,
            annotation_buffers: Vec::new(),
            annotation_frame_callback: None,
            annotation_redraw_pending: false,
            annotation_visible: false,
            zoom_badge_surface: None,
            zoom_badge_subsurface: None,
            zoom_badge_buffer: None,
            zoom_badge_visible: false,
            toast_surface: None,
            toast_subsurface: None,
            toast_buffer: None,
            toast_visible: false,
            color_picker_surface: None,
            color_picker_subsurface: None,
            color_picker_buffer: None,
            color_picker_visible: false,
            help_surface: None,
            help_subsurface: None,
            help_buffer: None,
            help_visible: false,
            annotations: Vec::new(),
            active_annotation: None,
            active_move: None,
            active_text: None,
            shift_select_start: None,
            view_source: initial_view_source,
            initial_view_source,
            zoom_animation: None,
            pointer_x: logical_size.width / 2.0,
            pointer_y: logical_size.height / 2.0,
            pointer_pressed: false,
            last_click_time: 0,
            last_click_button: 0,
            configured_width: 0,
            configured_height: 0,
            is_configured: false,
            initial_zoom_applied: false,
        },
    );
    if state.focused_window.is_none() && state.windows.len() == 1 {
        state.focused_window = Some(output_id);
    }
    overlay::create_overlays_for_window(state, output_id, qh)?;
    if let Some(window) = state.windows.get(&output_id) {
        window.surface.commit();
    }

    Ok(())
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, u32> for AppState {
    fn event(
        state: &mut Self,
        layer_surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        output_id: &u32,
        _: &wayland_client::Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                tracing::info!(
                    output_id = *output_id,
                    serial,
                    width,
                    height,
                    "received layer_surface configure"
                );

                if let Some(window) = state.windows.get_mut(output_id) {
                    window.configured_width = width as i32;
                    window.configured_height = height as i32;
                }

                layer_surface.ack_configure(serial);

                let output_transform = state
                    .outputs
                    .get(output_id)
                    .map(|output| output.transform.to_wayland());

                let zoom_setup = if state.config.initial_zoom > 0.0 {
                    let output = state.outputs.get(output_id);
                    let zoom_pixels = output
                        .map(|o| o.geometry.height as f64 * state.config.initial_zoom)
                        .unwrap_or(0.0);
                    let logical_size = output
                        .map(|o| {
                            let (w, h) = o.logical_size();
                            Size {
                                width: w as f64,
                                height: h as f64,
                            }
                        })
                        .unwrap_or_default();
                    let buffer_size = output
                        .and_then(|o| o.buffer_dimensions())
                        .map(|(w, h)| Size {
                            width: w as f64,
                            height: h as f64,
                        })
                        .unwrap_or_default();
                    Some((zoom_pixels, logical_size, buffer_size))
                } else {
                    None
                };

                let Some(window) = state.windows.get_mut(output_id) else {
                    return;
                };
                window.is_configured = true;
                if let Some(transform) = output_transform {
                    window.surface.set_buffer_transform(transform);
                }
                if width != 0 && height != 0 {
                    window.viewport.set_destination(width as i32, height as i32);
                }
                if let Some((zoom_pixels, logical_size, buffer_size)) =
                    zoom_setup.filter(|_| !window.initial_zoom_applied)
                {
                    apply_zoom(
                        &mut window.view_source,
                        zoom_pixels,
                        crate::zoom::screen_center(logical_size),
                        logical_size,
                        buffer_size,
                    );
                    window.initial_zoom_applied = true;
                }
                attach_output_buffer(state, *output_id);
                render_window(state, *output_id);
            }
            zwlr_layer_surface_v1::Event::Closed => {
                tracing::info!(output_id = *output_id, "received compositor close request");
                state.request_exit();
            }
            _ => {}
        }
    }
}

delegate_noop!(AppState: ignore wl_surface::WlSurface);
delegate_noop!(AppState: ignore wp_viewport::WpViewport);

pub fn animate_window_to_view(
    state: &mut AppState,
    output_id: u32,
    target: ViewRect,
    duration: Duration,
) {
    let Some(window) = state.windows.get_mut(&output_id) else {
        return;
    };

    if view_rect_nearly_equal(window.view_source, target) {
        window.view_source = target;
        window.zoom_animation = None;
        return;
    }

    window.zoom_animation = Some(ZoomAnimation {
        start: window.view_source,
        target,
        started_at: Instant::now(),
        duration,
    });
}

pub fn cancel_zoom_animation(state: &mut AppState, output_id: u32) {
    if let Some(window) = state.windows.get_mut(&output_id) {
        window.zoom_animation = None;
    }
}

pub fn tick_zoom_animations(state: &mut AppState) {
    let now = Instant::now();
    let output_ids = state.windows.keys().copied().collect::<Vec<_>>();

    for output_id in output_ids {
        if advance_zoom_animation(state, output_id, now) {
            render_window(state, output_id);
        }
    }
}

fn advance_zoom_animation(state: &mut AppState, output_id: u32, now: Instant) -> bool {
    let Some(window) = state.windows.get_mut(&output_id) else {
        return false;
    };
    let Some(animation) = window.zoom_animation.as_ref() else {
        return false;
    };

    let elapsed = now.saturating_duration_since(animation.started_at);
    let progress = if animation.duration.is_zero() {
        1.0
    } else {
        elapsed.as_secs_f64() / animation.duration.as_secs_f64()
    };

    if progress >= 1.0 {
        window.view_source = animation.target;
        window.zoom_animation = None;
        return true;
    }

    window.view_source =
        interpolate_view(animation.start, animation.target, ease_out_cubic(progress));
    true
}

pub fn render_window(state: &mut AppState, output_id: u32) {
    let (surface, viewport, view_source, damage_width, damage_height) = {
        let Some(window) = state.windows.get_mut(&output_id) else {
            return;
        };
        if !window.is_configured {
            return;
        }

        let Some(output) = state.outputs.get(&output_id) else {
            return;
        };
        let Some(buffer) = output.buffer.as_ref() else {
            return;
        };

        let (lw, lh) = output.logical_size();
        let logical_size = Size {
            width: lw as f64,
            height: lh as f64,
        };
        let buffer_size = Size {
            width: buffer.width as f64,
            height: buffer.height as f64,
        };
        let ratio = aspect_ratio(logical_size, buffer_size);
        clamp_view(&mut window.view_source, buffer_size, ratio);

        (
            window.surface.clone(),
            window.viewport.clone(),
            window.view_source,
            buffer.width,
            buffer.height,
        )
    };
    viewport.set_source(
        view_source.x,
        view_source.y,
        view_source.width,
        view_source.height,
    );
    surface.damage_buffer(0, 0, damage_width, damage_height);
    surface.commit();
    overlay::update_window_overlays(state, output_id);
}

pub fn attach_output_buffer(state: &AppState, output_id: u32) {
    let Some(window) = state.windows.get(&output_id) else {
        return;
    };
    let Some(output) = state.outputs.get(&output_id) else {
        return;
    };
    let Some(buffer) = output.buffer.as_ref() else {
        return;
    };

    window.surface.attach(Some(&buffer.wl_buffer), 0, 0);
}
