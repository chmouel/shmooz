use std::time::Duration;

use calloop::{
    EventLoop,
    timer::{TimeoutAction, Timer},
};
use calloop_wayland_source::WaylandSource;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::{
    capture,
    cli::Cli,
    config::Config,
    error::{AppError, Result},
    input,
    state::{AppState, REPEAT_INTERVAL},
    wayland::WaylandContext,
    window,
};

pub fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .without_time()
        .try_init();
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::try_from(cli)?;
    let mut context = WaylandContext::connect(config)?;
    log_selected_outputs(&context.state)?;

    let qh = context.event_queue.handle();
    context.state.queue_handle = Some(qh.clone());
    let selected_outputs = context.state.selected_output_ids()?;
    tracing::info!(
        outputs = selected_outputs.len(),
        "starting initial screencopy"
    );
    for output_id in &selected_outputs {
        capture::begin_output_capture(&mut context.state, *output_id)?;
    }
    wait_for_initial_captures(&mut context, &selected_outputs)?;

    for output_id in selected_outputs {
        window::create_window_for_output(&mut context.state, output_id, &qh)?;
    }

    let mut event_loop: EventLoop<AppState> =
        EventLoop::try_new().map_err(|err| AppError::event_loop("create event loop", err))?;
    context.state.loop_signal = Some(event_loop.get_signal());
    install_repeat_timer(&event_loop)?;
    install_animation_timer(&event_loop)?;

    WaylandSource::new(context.connection, context.event_queue)
        .insert(event_loop.handle())
        .map_err(|err| AppError::event_loop("insert Wayland source", err))?;

    tracing::info!("entering event loop");
    event_loop
        .run(None::<Duration>, &mut context.state, |_| {})
        .map_err(|err| AppError::event_loop("run event loop", err))?;
    tracing::info!("event loop exited");

    if let Some(err) = context.state.take_fatal_error() {
        return Err(err);
    }

    Ok(())
}

fn log_selected_outputs(state: &AppState) -> Result<()> {
    let selected = state.selected_outputs()?;
    tracing::info!(
        total_outputs = state.outputs.len(),
        selected_outputs = selected.len(),
        used_xdg_output = state.globals.xdg_output_manager.is_some(),
        "startup discovery completed"
    );

    for output in selected {
        let geometry = output.logical_geometry;
        tracing::info!(
            name = output.name.as_deref().unwrap_or("<unnamed>"),
            geometry = %format!(
                "{}x{}+{}+{}",
                geometry.width, geometry.height, geometry.x, geometry.y
            ),
            scale = output.logical_scale,
            "selected output"
        );
    }

    Ok(())
}

fn wait_for_initial_captures(context: &mut WaylandContext, selected_outputs: &[u32]) -> Result<()> {
    while !selected_outputs.iter().all(|output_id| {
        context
            .state
            .outputs
            .get(output_id)
            .is_some_and(|output| output.buffer.is_some() && !output.capture_pending)
    }) {
        context
            .event_queue
            .blocking_dispatch(&mut context.state)
            .map_err(|err| AppError::dispatch("initial screencopy dispatch", err))?;

        if let Some(err) = context.state.take_fatal_error() {
            return Err(err);
        }
    }

    Ok(())
}

fn install_repeat_timer(event_loop: &EventLoop<'_, AppState>) -> Result<()> {
    event_loop
        .handle()
        .insert_source(Timer::from_duration(REPEAT_INTERVAL), |_, _, state| {
            input::repeat_timer_tick(state);
            TimeoutAction::ToDuration(REPEAT_INTERVAL)
        })
        .map_err(|err| AppError::event_loop("insert repeat timer", err))?;

    Ok(())
}

fn install_animation_timer(event_loop: &EventLoop<'_, AppState>) -> Result<()> {
    let interval = Duration::from_millis(16);

    event_loop
        .handle()
        .insert_source(Timer::from_duration(interval), move |_, _, state| {
            window::tick_zoom_animations(state);
            TimeoutAction::ToDuration(interval)
        })
        .map_err(|err| AppError::event_loop("insert animation timer", err))?;

    Ok(())
}
