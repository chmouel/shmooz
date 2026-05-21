use std::{
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{Seek, Write},
    os::{fd::OwnedFd, unix::fs::PermissionsExt},
    path::Path,
    process::{Command, Stdio},
};

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
pub const TEXT_MIME_TYPES: &[&str] = &["text/plain;charset=utf-8", "text/plain"];
const TEXT_WL_COPY_MIME_TYPE: &str = "text/plain";
const WL_COPY_BINARY: &str = "wl-copy";

pub fn is_available(state: &AppState) -> bool {
    native_clipboard_available(state)
        || native_primary_selection_available(state)
        || wl_copy_available()
}

pub fn set_png_selection(state: &mut AppState, serial: u32, png: Vec<u8>) -> Result<()> {
    set_selection(state, serial, png, PNG_MIME_TYPE, &[PNG_MIME_TYPE])
}

pub fn set_text_selection(state: &mut AppState, serial: u32, text: String) -> Result<()> {
    set_selection(
        state,
        serial,
        text.into_bytes(),
        TEXT_WL_COPY_MIME_TYPE,
        TEXT_MIME_TYPES,
    )
}

fn set_selection(
    state: &mut AppState,
    serial: u32,
    data: Vec<u8>,
    wl_copy_mime_type: &str,
    mime_types: &[&'static str],
) -> Result<()> {
    clear_clipboard_selection(state);
    clear_primary_selection(state);

    let mut regular_selection_set = false;
    let queue_handle = state.queue_handle.clone();
    if native_clipboard_available(state) {
        let queue_handle = queue_handle
            .as_ref()
            .ok_or_else(|| AppError::runtime("Wayland queue handle is not available"))?;
        regular_selection_set =
            set_native_clipboard_selection(state, serial, &data, mime_types, queue_handle);
    }

    let mut primary_selection_set = false;
    if native_primary_selection_available(state) {
        let queue_handle = queue_handle
            .as_ref()
            .ok_or_else(|| AppError::runtime("Wayland queue handle is not available"))?;
        primary_selection_set =
            set_native_primary_selection(state, serial, &data, mime_types, queue_handle);
    }

    let mut wl_copy_error = None;
    if wl_copy_available() {
        match copy_with_wl_copy(wl_copy_mime_type, &data) {
            Ok(()) => regular_selection_set = true,
            Err(err) => {
                tracing::warn!(error = %err, "failed to start wl-copy");
                wl_copy_error = Some(err);
            }
        }
    }

    if regular_selection_set || primary_selection_set {
        Ok(())
    } else if let Some(err) = wl_copy_error {
        Err(err)
    } else {
        Err(AppError::runtime(
            "clipboard data device is not available and wl-copy is not installed",
        ))
    }
}

fn native_clipboard_available(state: &AppState) -> bool {
    state.globals.data_device_manager.is_some() && state.globals.data_device.is_some()
}

fn native_primary_selection_available(state: &AppState) -> bool {
    state.globals.primary_selection_device_manager.is_some()
        && state.globals.primary_selection_device.is_some()
}

fn set_native_clipboard_selection(
    state: &mut AppState,
    serial: u32,
    data: &[u8],
    mime_types: &[&'static str],
    queue_handle: &QueueHandle<AppState>,
) -> bool {
    let (Some(manager), Some(data_device)) = (
        state.globals.data_device_manager.clone(),
        state.globals.data_device.clone(),
    ) else {
        return false;
    };

    let source = manager.create_data_source(queue_handle, ());
    for mime_type in mime_types {
        source.offer((*mime_type).to_owned());
    }
    data_device.set_selection(Some(&source), serial);
    state.clipboard_selection = Some(ClipboardSelection {
        source,
        mime_types: mime_types.to_vec(),
        data: data.to_vec(),
    });
    true
}

fn set_native_primary_selection(
    state: &mut AppState,
    serial: u32,
    data: &[u8],
    mime_types: &[&'static str],
    queue_handle: &QueueHandle<AppState>,
) -> bool {
    let (Some(manager), Some(primary_device)) = (
        state.globals.primary_selection_device_manager.clone(),
        state.globals.primary_selection_device.clone(),
    ) else {
        return false;
    };

    let source = manager.create_source(queue_handle, ());
    for mime_type in mime_types {
        source.offer((*mime_type).to_owned());
    }
    primary_device.set_selection(Some(&source), serial);
    state.primary_selection = Some(PrimarySelection {
        source,
        mime_types: mime_types.to_vec(),
        data: data.to_vec(),
    });
    true
}

#[derive(Debug, PartialEq, Eq)]
struct WlCopyInvocation<'a> {
    binary: &'static str,
    args: [&'a str; 2],
}

fn wl_copy_invocation(mime_type: &str) -> WlCopyInvocation<'_> {
    WlCopyInvocation {
        binary: WL_COPY_BINARY,
        args: ["--type", mime_type],
    }
}

fn wl_copy_available() -> bool {
    env::var_os("PATH").is_some_and(|path| path_has_executable(&path, WL_COPY_BINARY))
}

fn path_has_executable(path: &OsStr, binary: &str) -> bool {
    env::split_paths(path).any(|directory| is_executable_file(&directory.join(binary)))
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn copy_with_wl_copy(mime_type: &str, data: &[u8]) -> Result<()> {
    let invocation = wl_copy_invocation(mime_type);
    let mut input = tempfile::tempfile()
        .map_err(|err| AppError::runtime(format!("failed to create wl-copy input file: {err}")))?;
    input.write_all(data).map_err(|err| {
        AppError::runtime(format!("failed to write clipboard data for wl-copy: {err}"))
    })?;
    input
        .rewind()
        .map_err(|err| AppError::runtime(format!("failed to rewind wl-copy input file: {err}")))?;

    Command::new(invocation.binary)
        .args(invocation.args)
        .stdin(Stdio::from(input))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| AppError::runtime(format!("failed to run wl-copy: {err}")))
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

    if !selection.mime_types.contains(&mime_type) {
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

    if !selection.mime_types.contains(&mime_type) {
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

#[cfg(test)]
mod tests {
    use std::{env, fs, os::unix::fs::PermissionsExt};

    use tempfile::tempdir;

    use super::{
        PNG_MIME_TYPE, TEXT_WL_COPY_MIME_TYPE, WL_COPY_BINARY, path_has_executable,
        wl_copy_invocation,
    };

    #[test]
    fn wl_copy_invocation_uses_explicit_png_mime_type() {
        let invocation = wl_copy_invocation(PNG_MIME_TYPE);

        assert_eq!(invocation.binary, WL_COPY_BINARY);
        assert_eq!(invocation.args, ["--type", "image/png"]);
    }

    #[test]
    fn wl_copy_invocation_uses_plain_text_for_text_copy() {
        let invocation = wl_copy_invocation(TEXT_WL_COPY_MIME_TYPE);

        assert_eq!(invocation.binary, WL_COPY_BINARY);
        assert_eq!(invocation.args, ["--type", "text/plain"]);
    }

    #[test]
    fn path_has_executable_finds_wl_copy_in_later_path_entry() {
        let missing_dir = tempdir().unwrap();
        let bin_dir = tempdir().unwrap();
        let wl_copy = bin_dir.path().join(WL_COPY_BINARY);
        fs::write(&wl_copy, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&wl_copy, fs::Permissions::from_mode(0o755)).unwrap();

        let path = env::join_paths([missing_dir.path(), bin_dir.path()]).unwrap();

        assert!(path_has_executable(&path, WL_COPY_BINARY));
    }

    #[test]
    fn path_has_executable_ignores_non_executable_files() {
        let bin_dir = tempdir().unwrap();
        let wl_copy = bin_dir.path().join(WL_COPY_BINARY);
        fs::write(&wl_copy, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&wl_copy, fs::Permissions::from_mode(0o644)).unwrap();

        let path = env::join_paths([bin_dir.path()]).unwrap();

        assert!(!path_has_executable(&path, WL_COPY_BINARY));
    }
}

delegate_noop!(AppState: ignore wl_data_device_manager::WlDataDeviceManager);
delegate_noop!(AppState: ignore wl_data_offer::WlDataOffer);
delegate_noop!(AppState: ignore zwp_primary_selection_device_manager_v1::ZwpPrimarySelectionDeviceManagerV1);
delegate_noop!(AppState: ignore zwp_primary_selection_offer_v1::ZwpPrimarySelectionOfferV1);
