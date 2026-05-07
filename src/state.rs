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
use xkbcommon::xkb;

use crate::{
    config::Config,
    error::{AppError, Result},
    output::{OutputState, output_matches_filter},
    window::WindowState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InteractionMode {
    #[default]
    Navigate,
    AnnotateZoomed,
    AnnotateUnzoomed,
}

impl InteractionMode {
    pub fn is_annotating(self) -> bool {
        !matches!(self, Self::Navigate)
    }

    pub fn badge_title(self, tool: AnnotationTool) -> String {
        match self {
            Self::Navigate => "NAVIGATE".to_owned(),
            Self::AnnotateZoomed => format!("DRAW {}", tool.label()),
            Self::AnnotateUnzoomed => format!("DRAW NO ZOOM {}", tool.label()),
        }
    }

    pub fn badge_hints(self) -> [&'static str; 3] {
        match self {
            Self::Navigate => [
                "D DRAW  W DRAW NO ZOOM",
                "+ - ZOOM  ARROWS PAN",
                "S SPOT  ESC CLOSE",
            ],
            Self::AnnotateZoomed | Self::AnnotateUnzoomed => [
                "P PEN  H HILITE  T TEXT",
                "L LINE  R RECT  E ELLIPSE",
                "U UNDO  C CLEAR  ENTER COMMIT",
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnnotationTool {
    #[default]
    Pen,
    Highlighter,
    Text,
    Line,
    Rectangle,
    Ellipse,
}

impl AnnotationTool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pen => "PEN",
            Self::Highlighter => "HILITE",
            Self::Text => "TEXT",
            Self::Line => "LINE",
            Self::Rectangle => "RECT",
            Self::Ellipse => "ELLIPSE",
        }
    }

    pub fn color(self) -> u32 {
        match self {
            Self::Pen | Self::Line => 0xFFFF_4F5E,
            Self::Highlighter => 0x8888_7829,
            Self::Text => 0xFFFF_FFFF,
            Self::Rectangle => 0xFF48_C78E,
            Self::Ellipse => 0xFF5B_8DEF,
        }
    }

    pub fn stroke_width(self) -> usize {
        match self {
            Self::Pen | Self::Line => 4,
            Self::Highlighter => 18,
            Self::Text => 4,
            Self::Rectangle | Self::Ellipse => 5,
        }
    }

    pub fn shape_kind(self) -> Option<AnnotationShapeKind> {
        match self {
            Self::Line => Some(AnnotationShapeKind::Line),
            Self::Rectangle => Some(AnnotationShapeKind::Rectangle),
            Self::Ellipse => Some(AnnotationShapeKind::Ellipse),
            Self::Pen | Self::Highlighter | Self::Text => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnnotationPoint {
    pub x: i32,
    pub y: i32,
}

impl AnnotationPoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            x: x.round() as i32,
            y: y.round() as i32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationShapeKind {
    Line,
    Rectangle,
    Ellipse,
}

#[derive(Debug, Clone)]
pub struct StrokeAnnotation {
    pub points: Vec<AnnotationPoint>,
    pub color: u32,
    pub width: usize,
}

#[derive(Debug, Clone)]
pub struct ShapeAnnotation {
    pub kind: AnnotationShapeKind,
    pub start: AnnotationPoint,
    pub end: AnnotationPoint,
    pub color: u32,
    pub width: usize,
}

#[derive(Debug, Clone)]
pub struct TextAnnotation {
    pub position: AnnotationPoint,
    pub text: String,
    pub color: u32,
    pub scale: usize,
}

#[derive(Debug, Clone)]
pub enum AnnotationItem {
    Stroke(StrokeAnnotation),
    Shape(ShapeAnnotation),
    Text(TextAnnotation),
}

#[derive(Debug, Clone)]
pub enum ActiveAnnotation {
    Stroke(StrokeAnnotation),
    Shape(ShapeAnnotation),
}

impl ActiveAnnotation {
    pub fn new(tool: AnnotationTool, point: AnnotationPoint) -> Self {
        match tool.shape_kind() {
            Some(kind) => Self::Shape(ShapeAnnotation {
                kind,
                start: point,
                end: point,
                color: tool.color(),
                width: tool.stroke_width(),
            }),
            None => Self::Stroke(StrokeAnnotation {
                points: vec![point],
                color: tool.color(),
                width: tool.stroke_width(),
            }),
        }
    }

    pub fn update(&mut self, point: AnnotationPoint) {
        match self {
            Self::Stroke(stroke) => {
                if stroke.points.last().copied() != Some(point) {
                    stroke.points.push(point);
                }
            }
            Self::Shape(shape) => {
                shape.end = point;
            }
        }
    }

    pub fn finish(self) -> Option<AnnotationItem> {
        match self {
            Self::Stroke(stroke) if stroke.points.len() >= 2 => {
                Some(AnnotationItem::Stroke(stroke))
            }
            Self::Shape(shape) if shape.start != shape.end => Some(AnnotationItem::Shape(shape)),
            _ => None,
        }
    }
}

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
        self.compositor()?;
        self.layer_shell()?;
        self.shm()?;
        self.screencopy_manager()?;
        self.viewporter()?;
        if self.seat.is_none() {
            return Err(AppError::missing_protocol("wl_seat"));
        }
        Ok(())
    }

    pub fn compositor(&self) -> Result<wl_compositor::WlCompositor> {
        self.compositor
            .clone()
            .ok_or_else(|| AppError::missing_protocol("wl_compositor"))
    }

    pub fn shm(&self) -> Result<wl_shm::WlShm> {
        self.shm
            .clone()
            .ok_or_else(|| AppError::missing_protocol("wl_shm"))
    }

    pub fn layer_shell(&self) -> Result<zwlr_layer_shell_v1::ZwlrLayerShellV1> {
        self.layer_shell
            .clone()
            .ok_or_else(|| AppError::missing_protocol("zwlr_layer_shell_v1"))
    }

    pub fn viewporter(&self) -> Result<wp_viewporter::WpViewporter> {
        self.viewporter
            .clone()
            .ok_or_else(|| AppError::missing_protocol("wp_viewporter"))
    }

    pub fn screencopy_manager(
        &self,
    ) -> Result<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1> {
        self.screencopy_manager
            .clone()
            .ok_or_else(|| AppError::missing_protocol("zwlr_screencopy_manager_v1"))
    }
}

pub struct AppState {
    pub config: Config,
    pub globals: BoundGlobals,
    pub queue_handle: Option<QueueHandle<AppState>>,
    pub outputs: BTreeMap<u32, OutputState>,
    pub windows: BTreeMap<u32, WindowState>,
    pub focused_window: Option<u32>,
    pub loop_signal: Option<LoopSignal>,
    pub fatal_error: Option<AppError>,
    pub spotlight_enabled: bool,
    pub spotlight_radius_frac: f64,
    pub interaction_mode: InteractionMode,
    pub annotation_tool: AnnotationTool,
    pub keyboard_text: Option<KeyboardTextState>,
    pub repeat_key: Option<u32>,
    pub repeat_deadline: Option<Instant>,
    pub repeat_interval: Duration,
}

pub struct KeyboardTextState {
    pub _context: xkb::Context,
    pub _keymap: xkb::Keymap,
    pub state: xkb::State,
    pub compose: Option<xkb::compose::State>,
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
            interaction_mode: InteractionMode::default(),
            annotation_tool: AnnotationTool::default(),
            keyboard_text: None,
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

    pub fn request_exit(&self) {
        if let Some(loop_signal) = &self.loop_signal {
            loop_signal.stop();
        }
    }

    pub fn record_fatal(&mut self, error: AppError) {
        if self.fatal_error.is_none() {
            self.fatal_error = Some(error);
        }
        self.request_exit();
    }

    pub fn take_fatal_error(&mut self) -> Option<AppError> {
        self.fatal_error.take()
    }

    pub fn stop_repeat(&mut self) {
        self.repeat_key = None;
        self.repeat_deadline = None;
    }
}
