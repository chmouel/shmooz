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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputTransform {
    #[default]
    Normal,
    Rot90,
    Rot180,
    Rot270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
    Unknown,
}

impl OutputTransform {
    pub fn is_quarter_turn(self) -> bool {
        matches!(
            self,
            Self::Rot90 | Self::Rot270 | Self::Flipped90 | Self::Flipped270
        )
    }

    pub fn to_wayland(self) -> wl_output::Transform {
        match self {
            Self::Normal => wl_output::Transform::Normal,
            Self::Rot90 => wl_output::Transform::_90,
            Self::Rot180 => wl_output::Transform::_180,
            Self::Rot270 => wl_output::Transform::_270,
            Self::Flipped => wl_output::Transform::Flipped,
            Self::Flipped90 => wl_output::Transform::Flipped90,
            Self::Flipped180 => wl_output::Transform::Flipped180,
            Self::Flipped270 => wl_output::Transform::Flipped270,
            Self::Unknown => wl_output::Transform::Normal,
        }
    }
}

pub struct OutputState {
    pub registry_name: u32,
    pub wl_output: Option<wl_output::WlOutput>,
    pub xdg_output: Option<zxdg_output_v1::ZxdgOutputV1>,
    pub geometry: Rect,
    pub logical_geometry: Rect,
    pub scale: i32,
    pub logical_scale: f64,
    pub transform: OutputTransform,
    pub name: Option<String>,
    pub buffer: Option<ShmBuffer>,
    pub pending_buffer: Option<ShmBuffer>,
    pub screencopy_frame: Option<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1>,
    pub capture_pending: bool,
}

impl OutputState {
    pub fn new(registry_name: u32, wl_output: wl_output::WlOutput) -> Self {
        Self {
            registry_name,
            wl_output: Some(wl_output),
            xdg_output: None,
            geometry: Rect::default(),
            logical_geometry: Rect::default(),
            scale: 1,
            logical_scale: 1.0,
            transform: OutputTransform::Normal,
            name: None,
            buffer: None,
            pending_buffer: None,
            screencopy_frame: None,
            capture_pending: false,
        }
    }

    #[cfg(test)]
    pub fn placeholder(registry_name: u32) -> Self {
        Self {
            registry_name,
            wl_output: None,
            xdg_output: None,
            geometry: Rect::default(),
            logical_geometry: Rect::default(),
            scale: 1,
            logical_scale: 1.0,
            transform: OutputTransform::Normal,
            name: None,
            buffer: None,
            pending_buffer: None,
            screencopy_frame: None,
            capture_pending: false,
        }
    }

    pub fn update_current_mode(&mut self, width: i32, height: i32) {
        if self.transform.is_quarter_turn() {
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

    let (width, height) = if output.transform.is_quarter_turn() {
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
    use super::{OutputState, OutputTransform, guess_logical_geometry, output_matches_filter};

    #[test]
    fn output_filter_requires_exact_name_match() {
        let mut output = OutputState::placeholder(1);
        output.name = Some("DP-1".to_owned());

        assert!(output_matches_filter(&output, None));
        assert!(output_matches_filter(&output, Some("DP-1")));
        assert!(!output_matches_filter(&output, Some("dp-1")));
        assert!(!output_matches_filter(&output, Some("DP")));
    }

    #[test]
    fn output_filter_rejects_unnamed_output_when_filter_is_set() {
        let output = OutputState::placeholder(1);
        assert!(!output_matches_filter(&output, Some("DP-1")));
    }

    #[test]
    fn guessed_logical_geometry_swaps_dimensions_for_rotated_outputs() {
        let mut output = OutputState::placeholder(7);
        output.geometry.x = 10;
        output.geometry.y = 20;
        output.geometry.width = 3840;
        output.geometry.height = 2160;
        output.scale = 2;
        output.transform = OutputTransform::Rot90;

        guess_logical_geometry(&mut output);

        assert_eq!(output.logical_geometry.x, 10);
        assert_eq!(output.logical_geometry.y, 20);
        assert_eq!(output.logical_geometry.width, 1080);
        assert_eq!(output.logical_geometry.height, 1920);
        assert_eq!(output.logical_scale, 2.0);
    }
}
