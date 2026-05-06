use wayland_client::{
    Dispatch, QueueHandle, WEnum, delegate_noop,
    protocol::{wl_buffer, wl_shm_pool},
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};

use crate::{
    error::{AppError, Result},
    shm::ShmBuffer,
    state::AppState,
    window,
};

pub fn begin_output_capture(state: &mut AppState, output_id: u32) -> Result<()> {
    if state
        .outputs
        .get(&output_id)
        .map(|output| output.capture_pending)
        .unwrap_or(false)
    {
        return Ok(());
    }

    let manager = state.globals.screencopy_manager()?;
    let qh = state
        .queue_handle
        .clone()
        .ok_or_else(|| AppError::runtime("Wayland queue handle is not available"))?;
    let wl_output = state
        .outputs
        .get(&output_id)
        .and_then(|output| output.wl_output.as_ref())
        .cloned()
        .ok_or_else(|| {
            AppError::runtime(format!(
                "output {output_id} is no longer available for screencopy"
            ))
        })?;

    let frame = manager.capture_output(0, &wl_output, &qh, output_id);
    tracing::info!(output_id, "requested screencopy");

    if let Some(output) = state.outputs.get_mut(&output_id) {
        output.capture_pending = true;
        output.screencopy_frame = Some(frame);
    }

    Ok(())
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, u32> for AppState {
    fn event(
        state: &mut Self,
        frame: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        output_id: &u32,
        _: &wayland_client::Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                tracing::info!(
                    output_id = *output_id,
                    width,
                    height,
                    stride,
                    "received screencopy buffer metadata"
                );
                let format = match format {
                    WEnum::Value(format) => format,
                    WEnum::Unknown(raw) => {
                        state.record_fatal(AppError::UnsupportedBufferFormat { raw });
                        return;
                    }
                };

                let shm = match state.globals.shm() {
                    Ok(shm) => shm,
                    Err(err) => {
                        state.record_fatal(err);
                        return;
                    }
                };

                let mut buffer = match ShmBuffer::create(
                    &shm,
                    qh,
                    format,
                    width as i32,
                    height as i32,
                    stride as i32,
                ) {
                    Ok(buffer) => buffer,
                    Err(err) => {
                        state.record_fatal(err);
                        return;
                    }
                };

                if state
                    .outputs
                    .get(output_id)
                    .map(|output| output.transform.is_quarter_turn())
                    .unwrap_or(false)
                {
                    std::mem::swap(&mut buffer.width, &mut buffer.height);
                }

                frame.copy(&buffer.wl_buffer);

                if let Some(output) = state.outputs.get_mut(output_id) {
                    output.buffer = Some(buffer);
                }
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                tracing::info!(output_id = *output_id, "screencopy frame ready");
                if let Some(output) = state.outputs.get_mut(output_id) {
                    output.capture_pending = false;
                    output.screencopy_frame = None;
                    if let Some(pending_buffer) = output.pending_buffer.take() {
                        output.buffer = Some(pending_buffer);
                    }
                }
                if state.windows.contains_key(output_id) {
                    window::attach_output_buffer(state, *output_id);
                    window::render_window(state, *output_id);
                }

                frame.destroy();
            }
            zwlr_screencopy_frame_v1::Event::Failed => {
                tracing::info!(output_id = *output_id, "screencopy frame failed");
                if let Some(output) = state.outputs.get_mut(output_id) {
                    output.capture_pending = false;
                    output.screencopy_frame = None;
                    output.pending_buffer = None;
                }

                let name = state
                    .outputs
                    .get(output_id)
                    .and_then(|output| output.name.clone())
                    .unwrap_or_else(|| format!("output-{output_id}"));
                state.record_fatal(AppError::CaptureFailed { output: name });
                frame.destroy();
            }
            zwlr_screencopy_frame_v1::Event::Flags { .. }
            | zwlr_screencopy_frame_v1::Event::Damage { .. }
            | zwlr_screencopy_frame_v1::Event::LinuxDmabuf { .. }
            | zwlr_screencopy_frame_v1::Event::BufferDone
            | _ => {}
        }
    }
}

delegate_noop!(AppState: ignore wl_buffer::WlBuffer);
delegate_noop!(AppState: ignore wl_shm_pool::WlShmPool);
delegate_noop!(AppState: ignore zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1);
