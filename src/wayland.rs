use std::cmp;

use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, WEnum, delegate_noop,
    protocol::{wl_compositor, wl_output, wl_registry, wl_seat, wl_shm, wl_subcompositor},
};
use wayland_protocols::{
    wp::viewporter::client::wp_viewporter,
    xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1},
};
use wayland_protocols_wlr::{
    layer_shell::v1::client::zwlr_layer_shell_v1,
    screencopy::v1::client::zwlr_screencopy_manager_v1,
};

use crate::{
    config::Config,
    error::{AppError, Result},
    output::{OutputState, OutputTransform, guess_logical_geometry},
    state::AppState,
};

pub struct WaylandContext {
    pub connection: Connection,
    pub event_queue: EventQueue<AppState>,
    pub state: AppState,
}

pub struct StartupSummary {
    pub total_outputs: usize,
    pub selected_outputs: Vec<OutputDetails>,
    pub used_xdg_output: bool,
}

pub struct OutputDetails {
    pub name: Option<String>,
    pub logical_geometry: crate::output::Rect,
    pub logical_scale: f64,
}

impl WaylandContext {
    pub fn connect(config: Config) -> Result<Self> {
        let connection = Connection::connect_to_env().map_err(AppError::connect)?;
        let mut event_queue = connection.new_event_queue();
        let qh = event_queue.handle();

        connection.display().get_registry(&qh, ());

        let mut state = AppState::new(config);

        event_queue
            .roundtrip(&mut state)
            .map_err(|err| AppError::dispatch("registry roundtrip", err))?;

        state.globals.validate()?;

        if state.outputs.is_empty() {
            return Err(AppError::NoOutputs);
        }

        if let Some(manager) = state.globals.xdg_output_manager.clone() {
            for output in state.outputs.values_mut() {
                let Some(wl_output) = output.wl_output.as_ref() else {
                    continue;
                };

                let xdg_output = manager.get_xdg_output(wl_output, &qh, output.registry_name);
                output.xdg_output = Some(xdg_output);
            }

            event_queue
                .roundtrip(&mut state)
                .map_err(|err| AppError::dispatch("xdg-output roundtrip", err))?;
        } else {
            tracing::warn!("zxdg_output_manager_v1 isn't available, guessing the output layout");

            for output in state.outputs.values_mut() {
                guess_logical_geometry(output);
            }
        }

        Ok(Self {
            connection,
            event_queue,
            state,
        })
    }

    pub fn summary(&self) -> Result<StartupSummary> {
        let selected_outputs = self
            .state
            .selected_outputs()?
            .into_iter()
            .map(|output| OutputDetails {
                name: output.name.clone(),
                logical_geometry: output.logical_geometry,
                logical_scale: output.logical_scale,
            })
            .collect::<Vec<_>>();

        Ok(StartupSummary {
            total_outputs: self.state.outputs.len(),
            selected_outputs,
            used_xdg_output: self.state.globals.xdg_output_manager.is_some(),
        })
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for AppState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_compositor" => {
                    let compositor = registry.bind::<wl_compositor::WlCompositor, _, _>(
                        name,
                        cmp::min(version, 5),
                        qh,
                        (),
                    );
                    state.globals.compositor = Some(compositor);
                }
                "wl_subcompositor" => {
                    let subcompositor =
                        registry.bind::<wl_subcompositor::WlSubcompositor, _, _>(name, 1, qh, ());
                    state.globals.subcompositor = Some(subcompositor);
                }
                "wl_shm" => {
                    let shm = registry.bind::<wl_shm::WlShm, _, _>(name, 1, qh, ());
                    state.globals.shm = Some(shm);
                }
                "zxdg_output_manager_v1" => {
                    let manager = registry
                        .bind::<zxdg_output_manager_v1::ZxdgOutputManagerV1, _, _>(
                            name,
                            cmp::min(version, 2),
                            qh,
                            (),
                        );
                    state.globals.xdg_output_manager = Some(manager);
                }
                "wl_output" => {
                    let wl_output = registry.bind::<wl_output::WlOutput, _, _>(
                        name,
                        cmp::min(version, 3),
                        qh,
                        name,
                    );
                    state
                        .outputs
                        .insert(name, OutputState::new(name, wl_output));
                }
                "zwlr_screencopy_manager_v1" => {
                    let manager = registry
                        .bind::<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, _, _>(
                            name,
                            1,
                            qh,
                            (),
                        );
                    state.globals.screencopy_manager = Some(manager);
                }
                "zwlr_layer_shell_v1" => {
                    let layer_shell = registry.bind::<zwlr_layer_shell_v1::ZwlrLayerShellV1, _, _>(
                        name,
                        cmp::min(version, 4),
                        qh,
                        (),
                    );
                    state.globals.layer_shell = Some(layer_shell);
                }
                "wp_viewporter" => {
                    let viewporter =
                        registry.bind::<wp_viewporter::WpViewporter, _, _>(name, 1, qh, ());
                    state.globals.viewporter = Some(viewporter);
                }
                "wl_seat" => {
                    let seat = registry.bind::<wl_seat::WlSeat, _, _>(name, 1, qh, ());
                    state.globals.seat = Some(seat);
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                state.outputs.remove(&name);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, u32> for AppState {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        output_name: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(output_name) else {
            return;
        };

        match event {
            wl_output::Event::Geometry {
                x, y, transform, ..
            } => {
                output.geometry.x = x;
                output.geometry.y = y;
                output.transform = map_output_transform(transform);
            }
            wl_output::Event::Mode {
                flags,
                width,
                height,
                ..
            } if is_current_mode(flags) => {
                output.update_current_mode(width, height);
            }
            wl_output::Event::Scale { factor } => {
                output.scale = factor;
            }
            _ => {}
        }
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, u32> for AppState {
    fn event(
        state: &mut Self,
        _: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        output_name: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(output_name) else {
            return;
        };

        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                output.logical_geometry.x = x;
                output.logical_geometry.y = y;
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                output.logical_geometry.width = width;
                output.logical_geometry.height = height;
            }
            zxdg_output_v1::Event::Done => {
                output.finalize_logical_details();
            }
            zxdg_output_v1::Event::Name { name } => {
                output.name = Some(name);
            }
            zxdg_output_v1::Event::Description { description: _ } => {}
            _ => {}
        }
    }
}

delegate_noop!(AppState: ignore wl_compositor::WlCompositor);
delegate_noop!(AppState: ignore zwlr_layer_shell_v1::ZwlrLayerShellV1);
delegate_noop!(AppState: ignore wl_subcompositor::WlSubcompositor);
delegate_noop!(AppState: ignore wl_shm::WlShm);
delegate_noop!(AppState: ignore wp_viewporter::WpViewporter);
delegate_noop!(AppState: ignore zxdg_output_manager_v1::ZxdgOutputManagerV1);

fn is_current_mode(flags: WEnum<wl_output::Mode>) -> bool {
    matches!(
        flags,
        WEnum::Value(value) if value.contains(wl_output::Mode::Current)
    )
}

fn map_output_transform(transform: WEnum<wl_output::Transform>) -> OutputTransform {
    match transform {
        WEnum::Value(wl_output::Transform::Normal) => OutputTransform::Normal,
        WEnum::Value(wl_output::Transform::_90) => OutputTransform::Rot90,
        WEnum::Value(wl_output::Transform::_180) => OutputTransform::Rot180,
        WEnum::Value(wl_output::Transform::_270) => OutputTransform::Rot270,
        WEnum::Value(wl_output::Transform::Flipped) => OutputTransform::Flipped,
        WEnum::Value(wl_output::Transform::Flipped90) => OutputTransform::Flipped90,
        WEnum::Value(wl_output::Transform::Flipped180) => OutputTransform::Flipped180,
        WEnum::Value(wl_output::Transform::Flipped270) => OutputTransform::Flipped270,
        WEnum::Value(_) => OutputTransform::Unknown,
        WEnum::Unknown(_) => OutputTransform::Unknown,
    }
}
