use wayland_client::protocol::wl_output;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1;
use wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1;

use crate::shm::ShmBuffer;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn is_quarter_turn(transform: wl_output::Transform) -> bool {
    matches!(
        transform,
        wl_output::Transform::_90
            | wl_output::Transform::_270
            | wl_output::Transform::Flipped90
            | wl_output::Transform::Flipped270
    )
}

pub struct OutputState {
    pub registry_name: u32,
    pub wl_output: Option<wl_output::WlOutput>,
    pub xdg_output: Option<zxdg_output_v1::ZxdgOutputV1>,
    pub geometry: Rect,
    pub logical_geometry: Rect,
    pub scale: i32,
    pub logical_scale: f64,
    pub transform: wl_output::Transform,
    pub name: Option<String>,
    pub buffer: Option<ShmBuffer>,
    pub pending_buffer: Option<ShmBuffer>,
    pub screencopy_frame: Option<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1>,
    pub capture_pending: bool,
}

impl OutputState {
    pub fn new(registry_name: u32, wl_output: Option<wl_output::WlOutput>) -> Self {
        Self {
            registry_name,
            wl_output,
            xdg_output: None,
            geometry: Rect::default(),
            logical_geometry: Rect::default(),
            scale: 1,
            logical_scale: 1.0,
            transform: wl_output::Transform::Normal,
            name: None,
            buffer: None,
            pending_buffer: None,
            screencopy_frame: None,
            capture_pending: false,
        }
    }

    pub fn update_current_mode(&mut self, width: i32, height: i32) {
        if is_quarter_turn(self.transform) {
            self.geometry.width = height;
            self.geometry.height = width;
        } else {
            self.geometry.width = width;
            self.geometry.height = height;
        }
    }

    pub fn finalize_logical_details(&mut self) {
        if self.logical_geometry.width > 0 {
            self.logical_scale = self.geometry.width as f64 / self.logical_geometry.width as f64;
        }
    }

    pub fn logical_size(&self) -> (i32, i32) {
        let width = if self.logical_geometry.width > 0 {
            self.logical_geometry.width
        } else {
            self.geometry.width
        };
        let height = if self.logical_geometry.height > 0 {
            self.logical_geometry.height
        } else {
            self.geometry.height
        };
        (width, height)
    }

    pub fn buffer_dimensions(&self) -> Option<(i32, i32)> {
        self.buffer.as_ref().map(|b| (b.width, b.height))
    }
}

pub fn output_matches_filter(output: &OutputState, filter: Option<&str>) -> bool {
    match filter {
        None => true,
        Some(filter) => output.name.as_deref() == Some(filter),
    }
}

pub fn guess_logical_geometry(output: &mut OutputState) {
    output.logical_geometry.x = output.geometry.x;
    output.logical_geometry.y = output.geometry.y;

    let (width, height) = if is_quarter_turn(output.transform) {
        (output.geometry.height, output.geometry.width)
    } else {
        (output.geometry.width, output.geometry.height)
    };

    output.logical_geometry.width = width / output.scale.max(1);
    output.logical_geometry.height = height / output.scale.max(1);
    output.logical_scale = output.scale.max(1) as f64;
}

#[cfg(test)]
mod tests {
    use wayland_client::protocol::wl_output;

    use super::{OutputState, guess_logical_geometry, output_matches_filter};

    #[test]
    fn output_filter_requires_exact_name_match() {
        let mut output = OutputState::new(1, None);
        output.name = Some("DP-1".to_owned());

        assert!(output_matches_filter(&output, None));
        assert!(output_matches_filter(&output, Some("DP-1")));
        assert!(!output_matches_filter(&output, Some("dp-1")));
        assert!(!output_matches_filter(&output, Some("DP")));
    }

    #[test]
    fn output_filter_rejects_unnamed_output_when_filter_is_set() {
        let output = OutputState::new(1, None);
        assert!(!output_matches_filter(&output, Some("DP-1")));
    }

    #[test]
    fn guessed_logical_geometry_swaps_dimensions_for_rotated_outputs() {
        let mut output = OutputState::new(7, None);
        output.geometry.x = 10;
        output.geometry.y = 20;
        output.geometry.width = 3840;
        output.geometry.height = 2160;
        output.scale = 2;
        output.transform = wl_output::Transform::_90;

        guess_logical_geometry(&mut output);

        assert_eq!(output.logical_geometry.x, 10);
        assert_eq!(output.logical_geometry.y, 20);
        assert_eq!(output.logical_geometry.width, 1080);
        assert_eq!(output.logical_geometry.height, 1920);
        assert_eq!(output.logical_scale, 2.0);
    }
}
