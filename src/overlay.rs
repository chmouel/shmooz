use wayland_client::{
    QueueHandle, delegate_noop,
    protocol::{wl_region, wl_shm, wl_subsurface, wl_surface},
};

use crate::{
    error::{AppError, Result},
    render,
    shm::ShmBuffer,
    state::AppState,
    window,
};

const SPOTLIGHT_MOVE_THRESHOLD_SQ: f64 = 16.0;
const ZOOM_BADGE_WIDTH: i32 = 320;
const ZOOM_BADGE_HEIGHT: i32 = 72;
const ZOOM_BADGE_MARGIN: i32 = 24;

pub fn create_overlays_for_window(
    state: &mut AppState,
    output_id: u32,
    qh: &QueueHandle<AppState>,
) -> Result<()> {
    if state.globals.subcompositor.is_none() {
        return Ok(());
    }

    create_spotlight_overlay(state, output_id, qh)?;
    create_zoom_badge_overlay(state, output_id, qh)?;
    update_window_overlays(state, output_id);

    Ok(())
}

pub fn update_window_overlays(state: &mut AppState, output_id: u32) {
    update_spotlight_overlay(state, output_id);
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
        .filter_map(|(output_id, window)| window.overlay_visible.then_some(*output_id))
        .collect::<Vec<_>>();

    for output_id in output_ids {
        refresh_spotlight_overlay(state, output_id);
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
        .map(|window| window.overlay_visible)
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
        window.overlay_buffer = Some(buffer);
        window.overlay_surface = Some(surface);
        window.overlay_subsurface = Some(subsurface);
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

    let mut buffer = ShmBuffer::create(
        &shm,
        qh,
        wl_shm::Format::Argb8888,
        ZOOM_BADGE_WIDTH,
        ZOOM_BADGE_HEIGHT,
        ZOOM_BADGE_WIDTH * 4,
    )?;
    render::paint_zoom_badge(
        buffer.data.as_mut(),
        ZOOM_BADGE_WIDTH as usize,
        ZOOM_BADGE_HEIGHT as usize,
    );

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

fn refresh_spotlight_overlay(state: &mut AppState, output_id: u32) {
    let (width, height) = logical_size(state, output_id);
    let Some(window) = state.windows.get_mut(&output_id) else {
        return;
    };
    let Some(buffer) = window.overlay_buffer.as_mut() else {
        return;
    };
    let Some(surface) = window.overlay_surface.as_ref() else {
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
    let Some(window) = state.windows.get_mut(&output_id) else {
        return;
    };
    let Some(surface) = window.zoom_badge_surface.as_ref() else {
        return;
    };
    let Some(buffer) = window.zoom_badge_buffer.as_ref() else {
        return;
    };

    surface.attach(Some(&buffer.wl_buffer), 0, 0);
    surface.damage(0, 0, ZOOM_BADGE_WIDTH, ZOOM_BADGE_HEIGHT);
    surface.commit();
}

fn update_spotlight_overlay(state: &mut AppState, output_id: u32) {
    let should_show = state
        .windows
        .get(&output_id)
        .map(|window| {
            window.overlay_surface.is_some() && window::is_zoomed(window) && state.spotlight_enabled
        })
        .unwrap_or(false);

    set_spotlight_overlay_visible(state, output_id, should_show);
}

fn update_zoom_badge_overlay(state: &mut AppState, output_id: u32) {
    let should_show = state
        .windows
        .get(&output_id)
        .map(|window| state.config.show_indicator && window.zoom_badge_surface.is_some())
        .unwrap_or(false);

    set_zoom_badge_visible(state, output_id, should_show);
}

fn set_spotlight_overlay_visible(state: &mut AppState, output_id: u32, show: bool) {
    let Some((was_visible, surface)) = state.windows.get(&output_id).map(|window| {
        (
            window.overlay_visible,
            window.overlay_surface.as_ref().cloned(),
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
        window.overlay_visible = show;
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

    if show {
        refresh_zoom_badge_overlay(state, output_id);
    } else {
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

fn window_surface(state: &AppState, output_id: u32) -> Result<wl_surface::WlSurface> {
    state
        .windows
        .get(&output_id)
        .map(|window| window.surface.clone())
        .ok_or_else(|| {
            AppError::runtime(format!("window {output_id} is not available for overlays"))
        })
}

delegate_noop!(AppState: ignore wl_region::WlRegion);
delegate_noop!(AppState: ignore wl_subsurface::WlSubsurface);
