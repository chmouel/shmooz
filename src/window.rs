use wayland_client::{
    Dispatch, QueueHandle, delegate_noop,
    protocol::{wl_subsurface, wl_surface},
};
use wayland_protocols::{
    wp::viewporter::client::wp_viewport,
    xdg::shell::client::{xdg_surface, xdg_toplevel},
};

use crate::{
    config::APP_ID,
    error::{AppError, Result},
    overlay,
    shm::ShmBuffer,
    state::AppState,
    zoom::{Size, ViewRect, apply_zoom, clamp_view},
};

pub const APP_TITLE: &str = "shmooz";
pub const APPLICATION_ID: &str = APP_ID;

#[allow(dead_code)]
pub struct WindowState {
    pub output_id: u32,
    pub surface: wl_surface::WlSurface,
    pub viewport: wp_viewport::WpViewport,
    pub xdg_surface: xdg_surface::XdgSurface,
    pub xdg_toplevel: xdg_toplevel::XdgToplevel,
    pub overlay_surface: Option<wl_surface::WlSurface>,
    pub overlay_subsurface: Option<wl_subsurface::WlSubsurface>,
    pub overlay_buffer: Option<ShmBuffer>,
    pub overlay_visible: bool,
    pub zoom_badge_surface: Option<wl_surface::WlSurface>,
    pub zoom_badge_subsurface: Option<wl_subsurface::WlSubsurface>,
    pub zoom_badge_buffer: Option<ShmBuffer>,
    pub zoom_badge_visible: bool,
    pub view_source: ViewRect,
    pub initial_view_source: ViewRect,
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

pub fn create_window_for_output(
    state: &mut AppState,
    output_id: u32,
    qh: &QueueHandle<AppState>,
) -> Result<()> {
    if state.windows.contains_key(&output_id) {
        return Ok(());
    }

    let compositor = state
        .globals
        .compositor
        .clone()
        .ok_or_else(|| AppError::missing_protocol("wl_compositor"))?;
    let shell = state
        .globals
        .shell
        .clone()
        .ok_or_else(|| AppError::missing_protocol("xdg_wm_base"))?;
    let viewporter = state
        .globals
        .viewporter
        .clone()
        .ok_or_else(|| AppError::missing_protocol("wp_viewporter"))?;
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
    let xdg_surface = shell.get_xdg_surface(&surface, qh, output_id);
    let xdg_toplevel = xdg_surface.get_toplevel(qh, output_id);
    let buffer_size = state
        .outputs
        .get(&output_id)
        .and_then(|output| output.buffer.as_ref())
        .map(|buffer| Size {
            width: buffer.width as f64,
            height: buffer.height as f64,
        })
        .ok_or_else(|| {
            AppError::runtime(format!(
                "output {output_id} has no captured buffer for window creation"
            ))
        })?;
    let initial_view_source = ViewRect::full(buffer_size);
    let logical_size = state
        .outputs
        .get(&output_id)
        .map(|output| Size {
            width: if output.logical_geometry.width > 0 {
                output.logical_geometry.width as f64
            } else {
                output.geometry.width as f64
            },
            height: if output.logical_geometry.height > 0 {
                output.logical_geometry.height as f64
            } else {
                output.geometry.height as f64
            },
        })
        .unwrap_or_default();

    xdg_toplevel.set_app_id(APPLICATION_ID.to_owned());
    xdg_toplevel.set_title(APP_TITLE.to_owned());
    xdg_toplevel.set_fullscreen(Some(&wl_output));

    tracing::info!(output_id, "creating fullscreen window");

    state.windows.insert(
        output_id,
        WindowState {
            output_id,
            surface,
            viewport,
            xdg_surface,
            xdg_toplevel,
            overlay_surface: None,
            overlay_subsurface: None,
            overlay_buffer: None,
            overlay_visible: false,
            zoom_badge_surface: None,
            zoom_badge_subsurface: None,
            zoom_badge_buffer: None,
            zoom_badge_visible: false,
            view_source: initial_view_source,
            initial_view_source,
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

impl Dispatch<xdg_surface::XdgSurface, u32> for AppState {
    fn event(
        state: &mut Self,
        xdg_surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        output_id: &u32,
        _: &wayland_client::Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            tracing::info!(
                output_id = *output_id,
                serial,
                "received xdg_surface configure"
            );
            xdg_surface.ack_configure(serial);
            let output_transform = state
                .outputs
                .get(output_id)
                .map(|output| output.transform.to_wayland());
            let configured_size = state
                .windows
                .get(output_id)
                .map(|window| (window.configured_width, window.configured_height));

            let zoom_setup = if state.config.initial_zoom > 0.0 {
                let zoom_pixels = state
                    .outputs
                    .get(output_id)
                    .map(|output| output.geometry.height as f64 * state.config.initial_zoom)
                    .unwrap_or(0.0);
                let logical_size = Size {
                    width: state
                        .outputs
                        .get(output_id)
                        .map(|output| {
                            if output.logical_geometry.width > 0 {
                                output.logical_geometry.width as f64
                            } else {
                                output.geometry.width as f64
                            }
                        })
                        .unwrap_or_default(),
                    height: state
                        .outputs
                        .get(output_id)
                        .map(|output| {
                            if output.logical_geometry.height > 0 {
                                output.logical_geometry.height as f64
                            } else {
                                output.geometry.height as f64
                            }
                        })
                        .unwrap_or_default(),
                };
                let buffer_size = state
                    .outputs
                    .get(output_id)
                    .and_then(|output| output.buffer.as_ref())
                    .map(|buffer| Size {
                        width: buffer.width as f64,
                        height: buffer.height as f64,
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
            if let Some((configured_width, configured_height)) = configured_size {
                if configured_width != 0 && configured_height != 0 {
                    window
                        .viewport
                        .set_destination(configured_width, configured_height);
                }
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
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, u32> for AppState {
    fn event(
        state: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        output_id: &u32,
        _: &wayland_client::Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { width, height, .. } => {
                if let Some(window) = state.windows.get_mut(output_id) {
                    window.configured_width = width;
                    window.configured_height = height;
                }
            }
            xdg_toplevel::Event::Close => {
                tracing::info!(output_id = *output_id, "received compositor close request");
                if let Some(loop_signal) = &state.loop_signal {
                    loop_signal.stop();
                }
            }
            _ => {}
        }
    }
}

delegate_noop!(AppState: ignore wl_surface::WlSurface);
delegate_noop!(AppState: ignore wp_viewport::WpViewport);

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

        let logical_size = Size {
            width: if output.logical_geometry.width > 0 {
                output.logical_geometry.width as f64
            } else {
                output.geometry.width as f64
            },
            height: if output.logical_geometry.height > 0 {
                output.logical_geometry.height as f64
            } else {
                output.geometry.height as f64
            },
        };
        let buffer_size = Size {
            width: buffer.width as f64,
            height: buffer.height as f64,
        };
        let ratio = if logical_size.width > 0.0 && logical_size.height > 0.0 {
            logical_size.width / logical_size.height
        } else if buffer_size.width > 0.0 && buffer_size.height > 0.0 {
            buffer_size.width / buffer_size.height
        } else {
            1.0
        };
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

pub fn is_zoomed(window: &WindowState) -> bool {
    window.view_source.width < window.initial_view_source.width - 0.5
        || window.view_source.height < window.initial_view_source.height - 0.5
}
