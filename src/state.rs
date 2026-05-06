use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use calloop::LoopSignal;
use wayland_client::{
    QueueHandle,
    protocol::{wl_compositor, wl_keyboard, wl_pointer, wl_seat, wl_shm, wl_subcompositor},
};
use wayland_protocols::{
    wp::viewporter::client::wp_viewporter, xdg::xdg_output::zv1::client::zxdg_output_manager_v1,
};
use wayland_protocols_wlr::{
    layer_shell::v1::client::zwlr_layer_shell_v1,
    screencopy::v1::client::zwlr_screencopy_manager_v1,
};

use crate::{
    config::Config,
    error::{AppError, Result},
    output::{OutputState, output_matches_filter},
    window::WindowState,
};

#[derive(Default)]
pub struct BoundGlobals {
    pub compositor: Option<wl_compositor::WlCompositor>,
    pub subcompositor: Option<wl_subcompositor::WlSubcompositor>,
    pub layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    pub shm: Option<wl_shm::WlShm>,
    pub xdg_output_manager: Option<zxdg_output_manager_v1::ZxdgOutputManagerV1>,
    pub screencopy_manager: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    pub viewporter: Option<wp_viewporter::WpViewporter>,
    pub seat: Option<wl_seat::WlSeat>,
    pub pointer: Option<wl_pointer::WlPointer>,
    pub keyboard: Option<wl_keyboard::WlKeyboard>,
}

impl BoundGlobals {
    pub fn validate(&self) -> Result<()> {
        if self.compositor.is_none() {
            return Err(AppError::missing_protocol("wl_compositor"));
        }
        if self.layer_shell.is_none() {
            return Err(AppError::missing_protocol("zwlr_layer_shell_v1"));
        }
        if self.shm.is_none() {
            return Err(AppError::missing_protocol("wl_shm"));
        }
        if self.screencopy_manager.is_none() {
            return Err(AppError::missing_protocol("zwlr_screencopy_manager_v1"));
        }
        if self.viewporter.is_none() {
            return Err(AppError::missing_protocol("wp_viewporter"));
        }
        if self.seat.is_none() {
            return Err(AppError::missing_protocol("wl_seat"));
        }

        Ok(())
    }
}

pub struct AppState {
    pub config: Config,
    pub globals: BoundGlobals,
    pub queue_handle: Option<QueueHandle<AppState>>,
    pub outputs: BTreeMap<u32, OutputState>,
    pub windows: BTreeMap<u32, WindowState>,
    #[allow(dead_code)]
    pub focused_window: Option<u32>,
    pub loop_signal: Option<LoopSignal>,
    pub fatal_error: Option<AppError>,
    pub spotlight_enabled: bool,
    pub spotlight_radius_frac: f64,
    pub repeat_key: Option<u32>,
    pub repeat_deadline: Option<Instant>,
    pub repeat_interval: Duration,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let spotlight_enabled = config.spotlight;
        Self {
            config,
            globals: BoundGlobals::default(),
            queue_handle: None,
            outputs: BTreeMap::new(),
            windows: BTreeMap::new(),
            focused_window: None,
            loop_signal: None,
            fatal_error: None,
            spotlight_enabled,
            spotlight_radius_frac: 0.25,
            repeat_key: None,
            repeat_deadline: None,
            repeat_interval: Duration::from_millis(50),
        }
    }

    pub fn selected_outputs(&self) -> Result<Vec<&OutputState>> {
        if self.outputs.is_empty() {
            return Err(AppError::NoOutputs);
        }

        let selected = self
            .outputs
            .values()
            .filter(|output| output_matches_filter(output, self.config.output_filter.as_deref()))
            .collect::<Vec<_>>();

        if !selected.is_empty() {
            return Ok(selected);
        }

        if let Some(filter) = &self.config.output_filter {
            return Err(AppError::OutputNotFound {
                name: filter.clone(),
            });
        }

        Err(AppError::NoSelectedOutputs)
    }

    pub fn selected_output_ids(&self) -> Result<Vec<u32>> {
        Ok(self
            .selected_outputs()?
            .into_iter()
            .map(|output| output.registry_name)
            .collect())
    }

    pub fn record_fatal(&mut self, error: AppError) {
        if self.fatal_error.is_none() {
            self.fatal_error = Some(error);
        }

        if let Some(loop_signal) = &self.loop_signal {
            loop_signal.stop();
        }
    }

    pub fn take_fatal_error(&mut self) -> Option<AppError> {
        self.fatal_error.take()
    }

    pub fn stop_repeat(&mut self) {
        self.repeat_key = None;
        self.repeat_deadline = None;
    }
}
