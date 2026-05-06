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
    state::AppState,
    wayland::{StartupSummary, WaylandContext},
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
    let summary = context.summary()?;
    log_summary(&summary);

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
    install_repeat_timer(&event_loop, &context.state)?;
    install_live_timer(&event_loop, &context.state)?;

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

fn log_summary(summary: &StartupSummary) {
    tracing::info!(
        total_outputs = summary.total_outputs,
        selected_outputs = summary.selected_outputs.len(),
        used_xdg_output = summary.used_xdg_output,
        "startup discovery completed"
    );

    for output in &summary.selected_outputs {
        tracing::info!(
            name = output.name.as_deref().unwrap_or("<unnamed>"),
            geometry = %format!(
                "{}x{}+{}+{}",
                output.logical_geometry.width,
                output.logical_geometry.height,
                output.logical_geometry.x,
                output.logical_geometry.y
            ),
            scale = output.logical_scale,
            "selected output"
        );
    }
}

fn wait_for_initial_captures(
    context: &mut WaylandContext,
    selected_outputs: &[u32],
) -> Result<()> {
    while !selected_outputs.iter().all(|output_id| {
        context
            .state
            .outputs
            .get(output_id)
            .map(|output| output.buffer.is_some() && !output.capture_pending)
            .unwrap_or(false)
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

fn install_repeat_timer(event_loop: &EventLoop<'_, AppState>, state: &AppState) -> Result<()> {
    let interval = state.repeat_interval;

    event_loop
        .handle()
        .insert_source(Timer::from_duration(interval), |_, _, state| {
            input::repeat_timer_tick(state);
            TimeoutAction::ToDuration(state.repeat_interval)
        })
        .map_err(|err| AppError::event_loop("insert repeat timer", err))?;

    Ok(())
}

fn install_live_timer(event_loop: &EventLoop<'_, AppState>, state: &AppState) -> Result<()> {
    if !state.config.live_zoom {
        return Ok(());
    }

    let interval = Duration::from_millis(u64::from(state.config.live_refresh_ms));
    event_loop
        .handle()
        .insert_source(Timer::from_duration(interval), |_, _, state| {
            capture::live_timer_tick(state);
            TimeoutAction::ToDuration(Duration::from_millis(u64::from(
                state.config.live_refresh_ms,
            )))
        })
        .map_err(|err| AppError::event_loop("insert live timer", err))?;

    Ok(())
}
