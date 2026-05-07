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

    pub fn badge_hints(self, tool: AnnotationTool) -> [&'static str; 3] {
        match self {
            Self::Navigate => [
                "D DRAW  W DRAW NO ZOOM",
                "+ - ZOOM  ARROWS PAN",
                "S SPOT  ESC CLOSE",
            ],
            Self::AnnotateZoomed | Self::AnnotateUnzoomed => match tool {
                AnnotationTool::Move => [
                    "M EXIT MOVE  P PEN  H HILITE",
                    "T TEXT  L LINE  R RECT  E ELL",
                    "U UNDO  C CLEAR  ESC BACK",
                ],
                _ => [
                    "P PEN  H HILITE  M MOVE",
                    "T TEXT  L LINE  R RECT  E ELL",
                    "U UNDO  C CLEAR  ENTER COMMIT",
                ],
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnnotationTool {
    #[default]
    Pen,
    Highlighter,
    Move,
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
            Self::Move => "MOVE",
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
            Self::Move => 0xFFFF_C83D,
            Self::Text => 0xFFFF_FFFF,
            Self::Rectangle => 0xFF48_C78E,
            Self::Ellipse => 0xFF5B_8DEF,
        }
    }

    pub fn stroke_width(self) -> usize {
        match self {
            Self::Pen | Self::Line => 4,
            Self::Highlighter => 18,
            Self::Move => 1,
            Self::Text => 4,
            Self::Rectangle | Self::Ellipse => 5,
        }
    }

    pub fn shape_kind(self) -> Option<AnnotationShapeKind> {
        match self {
            Self::Line => Some(AnnotationShapeKind::Line),
            Self::Rectangle => Some(AnnotationShapeKind::Rectangle),
            Self::Ellipse => Some(AnnotationShapeKind::Ellipse),
            Self::Pen | Self::Highlighter | Self::Move | Self::Text => None,
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

    pub fn offset(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
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

#[derive(Debug, Clone, Copy)]
pub struct ActiveMove {
    pub annotation_index: usize,
    pub last_point: AnnotationPoint,
}

impl AnnotationItem {
    pub fn translate(&mut self, dx: i32, dy: i32) {
        match self {
            Self::Stroke(stroke) => {
                for point in &mut stroke.points {
                    *point = point.offset(dx, dy);
                }
            }
            Self::Shape(shape) => {
                shape.start = shape.start.offset(dx, dy);
                shape.end = shape.end.offset(dx, dy);
            }
            Self::Text(text) => {
                text.position = text.position.offset(dx, dy);
            }
        }
    }

    pub fn hit_test(&self, point: AnnotationPoint, tolerance: f64) -> bool {
        match self {
            Self::Stroke(stroke) => stroke_hit_test(stroke, point, tolerance),
            Self::Shape(shape) => shape_hit_test(shape, point, tolerance),
            Self::Text(text) => text_hit_test(text, point, tolerance),
        }
    }
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

fn stroke_hit_test(stroke: &StrokeAnnotation, point: AnnotationPoint, tolerance: f64) -> bool {
    if stroke.points.len() <= 1 {
        return point_distance(point, stroke.points.first().copied().unwrap_or_default())
            <= tolerance + stroke.width as f64;
    }

    let half_width = stroke.width.max(1) as f64 / 2.0;
    stroke
        .points
        .windows(2)
        .any(|segment| point_near_segment(point, segment[0], segment[1], tolerance + half_width))
}

fn shape_hit_test(shape: &ShapeAnnotation, point: AnnotationPoint, tolerance: f64) -> bool {
    let tolerance = tolerance + shape.width.max(1) as f64 / 2.0;
    match shape.kind {
        AnnotationShapeKind::Line => point_near_segment(point, shape.start, shape.end, tolerance),
        AnnotationShapeKind::Rectangle => {
            point_near_rectangle(point, shape.start, shape.end, tolerance)
        }
        AnnotationShapeKind::Ellipse => {
            point_near_ellipse(point, shape.start, shape.end, tolerance)
        }
    }
}

fn text_hit_test(text: &TextAnnotation, point: AnnotationPoint, tolerance: f64) -> bool {
    let scale = text.scale.max(1) as i32;
    let width = if text.text.is_empty() {
        2
    } else {
        (text.text.chars().count() as i32 * 6 * scale).max(2)
    };
    let height = 7 * scale;
    let tolerance = tolerance.ceil() as i32;
    let left = text.position.x - tolerance;
    let top = text.position.y - tolerance;
    let right = text.position.x + width + tolerance;
    let bottom = text.position.y + height + tolerance;

    point.x >= left && point.x <= right && point.y >= top && point.y <= bottom
}

fn point_distance(a: AnnotationPoint, b: AnnotationPoint) -> f64 {
    let dx = f64::from(a.x - b.x);
    let dy = f64::from(a.y - b.y);
    (dx * dx + dy * dy).sqrt()
}

fn point_near_segment(
    point: AnnotationPoint,
    start: AnnotationPoint,
    end: AnnotationPoint,
    tolerance: f64,
) -> bool {
    let px = f64::from(point.x);
    let py = f64::from(point.y);
    let x1 = f64::from(start.x);
    let y1 = f64::from(start.y);
    let x2 = f64::from(end.x);
    let y2 = f64::from(end.y);
    let dx = x2 - x1;
    let dy = y2 - y1;

    if dx == 0.0 && dy == 0.0 {
        return point_distance(point, start) <= tolerance;
    }

    let projection = (((px - x1) * dx) + ((py - y1) * dy)) / (dx * dx + dy * dy);
    let projection = projection.clamp(0.0, 1.0);
    let closest_x = x1 + projection * dx;
    let closest_y = y1 + projection * dy;
    let dist_x = px - closest_x;
    let dist_y = py - closest_y;

    dist_x * dist_x + dist_y * dist_y <= tolerance * tolerance
}

fn point_near_rectangle(
    point: AnnotationPoint,
    start: AnnotationPoint,
    end: AnnotationPoint,
    tolerance: f64,
) -> bool {
    let left = start.x.min(end.x);
    let right = start.x.max(end.x);
    let top = start.y.min(end.y);
    let bottom = start.y.max(end.y);

    point_near_segment(
        point,
        AnnotationPoint { x: left, y: top },
        AnnotationPoint { x: right, y: top },
        tolerance,
    ) || point_near_segment(
        point,
        AnnotationPoint { x: right, y: top },
        AnnotationPoint {
            x: right,
            y: bottom,
        },
        tolerance,
    ) || point_near_segment(
        point,
        AnnotationPoint {
            x: right,
            y: bottom,
        },
        AnnotationPoint { x: left, y: bottom },
        tolerance,
    ) || point_near_segment(
        point,
        AnnotationPoint { x: left, y: bottom },
        AnnotationPoint { x: left, y: top },
        tolerance,
    )
}

fn point_near_ellipse(
    point: AnnotationPoint,
    start: AnnotationPoint,
    end: AnnotationPoint,
    tolerance: f64,
) -> bool {
    let left = f64::from(start.x.min(end.x));
    let right = f64::from(start.x.max(end.x));
    let top = f64::from(start.y.min(end.y));
    let bottom = f64::from(start.y.max(end.y));
    let rx = ((right - left) / 2.0).max(1.0);
    let ry = ((bottom - top) / 2.0).max(1.0);
    let cx = left + rx;
    let cy = top + ry;
    let dx = f64::from(point.x) - cx;
    let dy = f64::from(point.y) - cy;
    let angle = dy.atan2(dx);
    let edge_x = cx + rx * angle.cos();
    let edge_y = cy + ry * angle.sin();
    let dist_x = f64::from(point.x) - edge_x;
    let dist_y = f64::from(point.y) - edge_y;

    dist_x * dist_x + dist_y * dist_y <= tolerance * tolerance
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
    pub move_mode: bool,
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
            move_mode: false,
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

    pub fn effective_annotation_tool(&self) -> AnnotationTool {
        if self.move_mode {
            AnnotationTool::Move
        } else {
            self.annotation_tool
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{APP_ID, Config};

    use super::{
        AnnotationItem, AnnotationPoint, AnnotationShapeKind, AnnotationTool, AppState,
        ShapeAnnotation, StrokeAnnotation, TextAnnotation,
    };

    fn test_config() -> Config {
        Config {
            app_id: APP_ID,
            close_key: None,
            mouse_track: false,
            initial_zoom: 0.0,
            output_filter: None,
            invert_scroll: false,
            spotlight: false,
            show_indicator: true,
        }
    }

    #[test]
    fn annotation_item_translate_moves_shape_points() {
        let mut annotation = AnnotationItem::Shape(ShapeAnnotation {
            kind: AnnotationShapeKind::Rectangle,
            start: AnnotationPoint { x: 10, y: 20 },
            end: AnnotationPoint { x: 40, y: 60 },
            color: 0,
            width: 4,
        });

        annotation.translate(5, -3);

        let AnnotationItem::Shape(shape) = annotation else {
            panic!("shape annotation expected");
        };
        assert_eq!(shape.start, AnnotationPoint { x: 15, y: 17 });
        assert_eq!(shape.end, AnnotationPoint { x: 45, y: 57 });
    }

    #[test]
    fn annotation_item_hit_test_matches_top_level_shapes() {
        let stroke = AnnotationItem::Stroke(StrokeAnnotation {
            points: vec![
                AnnotationPoint { x: 10, y: 10 },
                AnnotationPoint { x: 40, y: 10 },
            ],
            color: 0,
            width: 4,
        });
        assert!(stroke.hit_test(AnnotationPoint { x: 25, y: 12 }, 8.0));
        assert!(!stroke.hit_test(AnnotationPoint { x: 25, y: 30 }, 4.0));

        let text = AnnotationItem::Text(TextAnnotation {
            position: AnnotationPoint { x: 50, y: 50 },
            text: "move".to_owned(),
            color: 0,
            scale: 4,
        });
        assert!(text.hit_test(AnnotationPoint { x: 60, y: 60 }, 4.0));
        assert!(!text.hit_test(AnnotationPoint { x: 10, y: 10 }, 4.0));
    }

    #[test]
    fn effective_annotation_tool_prefers_move_mode_without_losing_base_tool() {
        let mut state = AppState::new(test_config());
        state.annotation_tool = AnnotationTool::Rectangle;
        assert_eq!(state.effective_annotation_tool(), AnnotationTool::Rectangle);

        state.move_mode = true;

        assert_eq!(state.effective_annotation_tool(), AnnotationTool::Move);
        assert_eq!(state.annotation_tool, AnnotationTool::Rectangle);
    }
}
