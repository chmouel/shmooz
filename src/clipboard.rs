use std::{fs::File, io::Write, os::fd::OwnedFd};

use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    protocol::{wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source},
};
use wayland_protocols::wp::primary_selection::zv1::client::{
    zwp_primary_selection_device_manager_v1, zwp_primary_selection_device_v1,
    zwp_primary_selection_offer_v1, zwp_primary_selection_source_v1,
};

use crate::{
    error::{AppError, Result},
    state::{AppState, ClipboardSelection, PrimarySelection},
};

pub const PNG_MIME_TYPE: &str = "image/png";

pub fn is_available(state: &AppState) -> bool {
    (state.globals.data_device_manager.is_some() && state.globals.data_device.is_some())
        || (state.globals.primary_selection_device_manager.is_some()
            && state.globals.primary_selection_device.is_some())
}

pub fn set_png_selection(state: &mut AppState, serial: u32, png: Vec<u8>) -> Result<()> {
    clear_clipboard_selection(state);
    clear_primary_selection(state);

    let queue_handle = state
        .queue_handle
        .clone()
        .ok_or_else(|| AppError::runtime("Wayland queue handle is not available"))?;
    if let (Some(manager), Some(data_device)) = (
        state.globals.data_device_manager.clone(),
        state.globals.data_device.clone(),
    ) {
        let source = manager.create_data_source(&queue_handle, ());
        source.offer(PNG_MIME_TYPE.to_owned());
        data_device.set_selection(Some(&source), serial);
        state.clipboard_selection = Some(ClipboardSelection {
            source,
            data: png.clone(),
        });
    }

    if let (Some(manager), Some(primary_device)) = (
        state.globals.primary_selection_device_manager.clone(),
        state.globals.primary_selection_device.clone(),
    ) {
        let source = manager.create_source(&queue_handle, ());
        source.offer(PNG_MIME_TYPE.to_owned());
        primary_device.set_selection(Some(&source), serial);
        state.primary_selection = Some(PrimarySelection { source, data: png });
    }

    if state.clipboard_selection.is_some() || state.primary_selection.is_some() {
        Ok(())
    } else {
        Err(AppError::runtime(
            "clipboard or primary selection data device is not available",
        ))
    }
}

fn clear_clipboard_selection(state: &mut AppState) {
    if let Some(selection) = state.clipboard_selection.take() {
        selection.source.destroy();
    }
}

fn clear_primary_selection(state: &mut AppState) {
    if let Some(selection) = state.primary_selection.take() {
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
                clear_clipboard_selection(state);
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

impl Dispatch<zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1, ()> for AppState {
    fn event(
        state: &mut Self,
        source: &zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1,
        event: zwp_primary_selection_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_primary_selection_source_v1::Event::Send { mime_type, fd } => {
                send_primary_selection(state, source, &mime_type, fd);
            }
            zwp_primary_selection_source_v1::Event::Cancelled
                if state
                    .primary_selection
                    .as_ref()
                    .is_some_and(|selection| selection.source == *source) =>
            {
                clear_primary_selection(state);
            }
            _ => {}
        }
    }
}

impl Dispatch<zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1, ()> for AppState {
    wayland_client::event_created_child!(
        AppState,
        zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1,
        [0 => (zwp_primary_selection_offer_v1::ZwpPrimarySelectionOfferV1, ())]
    );

    fn event(
        _: &mut Self,
        _: &zwp_primary_selection_device_v1::ZwpPrimarySelectionDeviceV1,
        event: zwp_primary_selection_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_primary_selection_device_v1::Event::Selection { id: Some(offer) } = event {
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

    if mime_type != PNG_MIME_TYPE {
        return;
    }

    let data = selection.data.clone();
    write_selection_data(fd, data, "failed to serve clipboard data");
}

fn send_primary_selection(
    state: &mut AppState,
    source: &zwp_primary_selection_source_v1::ZwpPrimarySelectionSourceV1,
    mime_type: &str,
    fd: OwnedFd,
) {
    let Some(selection) = state
        .primary_selection
        .as_ref()
        .filter(|selection| selection.source == *source)
    else {
        return;
    };

    if mime_type != PNG_MIME_TYPE {
        return;
    }

    let data = selection.data.clone();
    write_selection_data(fd, data, "failed to serve primary selection data");
}

fn write_selection_data(fd: OwnedFd, data: Vec<u8>, message: &'static str) {
    std::thread::spawn(move || {
        let mut file = File::from(fd);
        if let Err(err) = file.write_all(&data) {
            tracing::warn!(error = %err, "{message}");
        }
    });
}

delegate_noop!(AppState: ignore wl_data_device_manager::WlDataDeviceManager);
delegate_noop!(AppState: ignore wl_data_offer::WlDataOffer);
delegate_noop!(AppState: ignore zwp_primary_selection_device_manager_v1::ZwpPrimarySelectionDeviceManagerV1);
delegate_noop!(AppState: ignore zwp_primary_selection_offer_v1::ZwpPrimarySelectionOfferV1);
