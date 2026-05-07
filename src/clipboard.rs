use std::{fs::File, io::Write, os::fd::OwnedFd};

use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    protocol::{wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source},
};

use crate::{
    error::{AppError, Result},
    state::{AppState, ClipboardSelection},
};

pub const PNG_MIME_TYPE: &str = "image/png";

pub fn is_available(state: &AppState) -> bool {
    state.globals.data_device_manager.is_some() && state.globals.data_device.is_some()
}

pub fn set_png_selection(state: &mut AppState, serial: u32, png: Vec<u8>) -> Result<()> {
    clear_selection(state);

    let manager = state.globals.data_device_manager()?;
    let data_device = state
        .globals
        .data_device
        .clone()
        .ok_or_else(|| AppError::runtime("clipboard data device is not available"))?;
    let queue_handle = state
        .queue_handle
        .clone()
        .ok_or_else(|| AppError::runtime("Wayland queue handle is not available"))?;
    let source = manager.create_data_source(&queue_handle, ());
    source.offer(PNG_MIME_TYPE.to_owned());
    data_device.set_selection(Some(&source), serial);
    state.clipboard_selection = Some(ClipboardSelection {
        source,
        mime_type: PNG_MIME_TYPE,
        data: png,
    });
    Ok(())
}

fn clear_selection(state: &mut AppState) {
    if let Some(selection) = state.clipboard_selection.take() {
        selection.source.destroy();
    }
}

impl Dispatch<wl_data_source::WlDataSource, ()> for AppState {
    fn event(
        state: &mut Self,
        source: &wl_data_source::WlDataSource,
        event: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_source::Event::Send { mime_type, fd } => {
                send_selection(state, source, &mime_type, fd);
            }
            wl_data_source::Event::Cancelled
                if state
                    .clipboard_selection
                    .as_ref()
                    .is_some_and(|selection| selection.source == *source) =>
            {
                clear_selection(state);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_data_device::WlDataDevice, ()> for AppState {
    wayland_client::event_created_child!(AppState, wl_data_device::WlDataDevice, [
        0 => (wl_data_offer::WlDataOffer, ())
    ]);

    fn event(
        _: &mut Self,
        _: &wl_data_device::WlDataDevice,
        event: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_device::Event::Selection { id: Some(offer) } = event {
            offer.destroy();
        }
    }
}

fn send_selection(
    state: &mut AppState,
    source: &wl_data_source::WlDataSource,
    mime_type: &str,
    fd: OwnedFd,
) {
    let Some(selection) = state
        .clipboard_selection
        .as_ref()
        .filter(|selection| selection.source == *source)
    else {
        return;
    };

    if mime_type != selection.mime_type {
        return;
    }

    let mut file = File::from(fd);
    if let Err(err) = file.write_all(&selection.data).and_then(|_| file.flush()) {
        tracing::warn!(mime_type, error = %err, "failed to serve clipboard data");
    }
}

delegate_noop!(AppState: ignore wl_data_device_manager::WlDataDeviceManager);
delegate_noop!(AppState: ignore wl_data_offer::WlDataOffer);
