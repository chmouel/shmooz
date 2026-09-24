use bytemuck::cast_slice_mut;

use crate::config::{CloseKey, close_key_label};
use crate::state::{
    ActiveAnnotation, AnnotationItem, AnnotationPoint, AnnotationShapeKind, BadgeModel,
    ShapeAnnotation, StrokeAnnotation, TextAnnotation,
};

const BADGE_TITLE_SCALE: usize = 3;
const BADGE_SUBTITLE_SCALE: usize = 2;
const BADGE_HINT_SCALE: usize = 2;
const TOAST_TITLE_SCALE: usize = 2;
const TOAST_TEXT_SCALE: usize = 1;
pub(crate) const TEXT_GLYPH_HEIGHT: usize = 7;
pub(crate) const TEXT_GLYPH_ADVANCE: usize = 6;

const CARD_RADIUS: f32 = 12.0;
const CARD_BACKGROUND: u32 = 0xEB13_1316;
const CARD_BORDER: u32 = 0x2CFF_FFFF;
const CARD_DIVIDER: u32 = 0x1AFF_FFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    Crosshair,
    Hand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayCursor {
    pub position: AnnotationPoint,
    pub style: CursorStyle,
}

/// Annotation overlay content, already projected into screen coordinates.
#[derive(Debug, Default)]
pub struct AnnotationScene {
    pub annotations: Vec<AnnotationItem>,
    pub active_annotation: Option<ActiveAnnotation>,
    pub active_text: Option<TextAnnotation>,
    pub cursor: Option<OverlayCursor>,
    pub shift_select_rect: Option<(AnnotationPoint, AnnotationPoint)>,
}

struct Canvas<'a> {
    pixels: &'a mut [u32],
    width: usize,
    height: usize,
}

pub fn draw_spotlight_overlay(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    center_x: usize,
    center_y: usize,
    radius: f64,
) {
    let pixels: &mut [u32] = cast_slice_mut(pixels);
    let radius_sq = radius * radius;
    let center_x = center_x.min(width.saturating_sub(1)) as i32;
    let center_y = center_y.min(height.saturating_sub(1)) as i32;
    let dim = 0xAA00_0000;

    for y in 0..height {
        let row = &mut pixels[y * width..(y + 1) * width];
        let dy = y as i32 - center_y;
        let dy_sq = f64::from(dy * dy);

        if dy_sq >= radius_sq {
            row.fill(dim);
            continue;
        }

        let half_width = (radius_sq - dy_sq).sqrt() as i32;
        let x0 = (center_x - half_width).clamp(0, width.saturating_sub(1) as i32) as usize;
        let x1 = (center_x + half_width + 1).clamp(0, width as i32) as usize;
        row[..x0].fill(dim);
        row[x0..x1].fill(0x0000_0000);
        row[x1..].fill(dim);
    }
}

pub fn draw_annotation_overlay(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    scene: &AnnotationScene,
) {
    let mut canvas = Canvas::new(pixels, width, height);
    canvas.pixels.fill(0);

    for annotation in &scene.annotations {
        match annotation {
            AnnotationItem::Stroke(stroke) => canvas.draw_stroke(stroke),
            AnnotationItem::Shape(shape) => canvas.draw_shape(shape),
            AnnotationItem::Text(text) => canvas.draw_text_annotation(text),
        }
    }

    match &scene.active_annotation {
        Some(ActiveAnnotation::Stroke(stroke)) => canvas.draw_stroke(stroke),
        Some(ActiveAnnotation::Shape(shape)) => canvas.draw_shape(shape),
        None => {}
    }

    if let Some(active_text) = &scene.active_text {
        canvas.draw_text_annotation(active_text);
    }

    if let Some((start, end)) = scene.shift_select_rect {
        canvas.blend_rect(start.x, start.y, end.x, end.y, 0x253B_82F6);
        canvas.draw_rectangle(start, end, 0xCC3B_82F6, 1);
    }

    if let Some(cursor) = scene.cursor {
        match cursor.style {
            CursorStyle::Crosshair => canvas.draw_crosshair_cursor(cursor.position),
            CursorStyle::Hand => canvas.draw_hand_cursor(cursor.position),
        }
    }
}

pub fn paint_zoom_badge(pixels: &mut [u8], width: usize, height: usize, badge: &BadgeModel) {
    let mut canvas = Canvas::new(pixels, width, height);
    canvas.paint_card(Some(0xFFFF_C83D));
    for y in [52, 80, 104] {
        canvas.draw_divider(y);
    }

    canvas.draw_label(18, 12, &badge.title, BADGE_TITLE_SCALE, 0xFFFF_FFFF);
    canvas.draw_label(18, 34, &badge.subtitle, BADGE_SUBTITLE_SCALE, 0xFF9C_A3AF);
    for (line, y) in badge.lines.iter().zip([60, 84, 108]) {
        canvas.draw_label(18, y, line.label, BADGE_HINT_SCALE, 0xFFFF_C83D);
        canvas.draw_label(88, y, &line.text, BADGE_HINT_SCALE, line.color);
    }
}

pub fn paint_toast(pixels: &mut [u8], width: usize, height: usize, title: &str, message: &str) {
    let mut canvas = Canvas::new(pixels, width, height);
    canvas.paint_card(Some(0xFFFF_C83D));
    canvas.draw_divider(54);

    canvas.draw_label(18, 16, title, TOAST_TITLE_SCALE, 0xFFFF_FFFF);
    canvas.draw_label(18, 40, "NOTIFICATION", TOAST_TEXT_SCALE, 0xFF9C_A3AF);
    canvas.draw_label(18, 68, message, TOAST_TEXT_SCALE, 0xFFFF_FFFF);
}

pub fn paint_color_picker(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    color: u32,
    hex: &str,
    copied: bool,
) {
    let mut canvas = Canvas::new(pixels, width, height);
    let color = 0xFF00_0000 | (color & 0x00FF_FFFF);
    canvas.paint_card(Some(color));

    canvas.fill_rect(18, 32, 86, 62, 0xFFFF_FFFF);
    canvas.fill_rect(21, 35, 80, 56, color);

    canvas.draw_label(122, 26, "COLOR", 2, 0xFF9C_A3AF);
    canvas.draw_label(122, 50, hex, 3, 0xFFFF_FFFF);
    if copied {
        canvas.draw_label(122, 82, "COPIED", 1, 0xFFFF_C83D);
    }
}

/// Color of the rounded card at (x, y) for a card of the given size, or None
/// outside its corners. `accent` tints the top three rows.
fn card_pixel(x: usize, y: usize, width: usize, height: usize, accent: Option<u32>) -> Option<u32> {
    let r = CARD_RADIUS;
    let cx = (x as f32).clamp(r, width as f32 - 1.0 - r);
    let cy = (y as f32).clamp(r, height as f32 - 1.0 - r);
    let dx = x as f32 - cx;
    let dy = y as f32 - cy;
    let d = (dx * dx + dy * dy).sqrt();
    if d > r + 0.5 {
        return None;
    }

    let alpha_scale = (r + 0.5 - d).clamp(0.0, 1.0);
    let border_alpha = (1.0 - (d - (r - 0.5)).abs()).clamp(0.0, 1.0);

    let mut color = if border_alpha > 0.0 {
        let b_alpha = ((CARD_BORDER >> 24) & 0xFF) as f32 * border_alpha;
        let b_color = (CARD_BORDER & 0x00FF_FFFF) | (((b_alpha as u32) & 0xFF) << 24);
        alpha_over(CARD_BACKGROUND, b_color)
    } else {
        CARD_BACKGROUND
    };

    if let Some(accent) = accent.filter(|_| y < 3) {
        color = alpha_over(color, accent);
    }

    let final_a = (((color >> 24) & 0xFF) as f32 * alpha_scale) as u32;
    Some((color & 0x00FF_FFFF) | (final_a << 24))
}

impl<'a> Canvas<'a> {
    fn new(pixels: &'a mut [u8], width: usize, height: usize) -> Self {
        Self {
            pixels: cast_slice_mut(pixels),
            width,
            height,
        }
    }

    /// Fills the whole buffer with a rounded card.
    fn paint_card(&mut self, accent: Option<u32>) {
        let (width, height) = (self.width, self.height);
        for y in 0..height {
            for x in 0..width {
                self.pixels[y * width + x] = card_pixel(x, y, width, height, accent).unwrap_or(0);
            }
        }
    }

    /// Blends a rounded card onto the buffer. The card must fit inside it.
    fn blend_card(&mut self, card_x: usize, card_y: usize, card_width: usize, card_height: usize) {
        for y in 0..card_height {
            for x in 0..card_width {
                if let Some(color) = card_pixel(x, y, card_width, card_height, None) {
                    let index = (card_y + y) * self.width + card_x + x;
                    self.pixels[index] = alpha_over(self.pixels[index], color);
                }
            }
        }
    }

    fn draw_divider(&mut self, y: i32) {
        let right = self.width.saturating_sub(18) as i32;
        self.blend_rect(18, y, right, y + 1, CARD_DIVIDER);
    }

    fn fill_rect(&mut self, x: i32, y: i32, rect_width: usize, rect_height: usize, color: u32) {
        let clip = |start: i32, len: usize, max: usize| {
            let start_clamped = start.clamp(0, max as i32) as usize;
            let end_clamped = start.saturating_add(len as i32).clamp(0, max as i32) as usize;
            start_clamped..end_clamped
        };
        let columns = clip(x, rect_width, self.width);
        for row in clip(y, rect_height, self.height) {
            let offset = row * self.width;
            self.pixels[offset + columns.start..offset + columns.end].fill(color);
        }
    }

    fn blend_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
        let left = x0.min(x1).max(0) as usize;
        let right = x0.max(x1).min(self.width as i32) as usize;
        let top = y0.min(y1).max(0) as usize;
        let bottom = y0.max(y1).min(self.height as i32) as usize;

        for yy in top..bottom {
            let row_offset = yy * self.width;
            for xx in left..right {
                let index = row_offset + xx;
                self.pixels[index] = alpha_over(self.pixels[index], color);
            }
        }
    }

    fn blend_pixel(&mut self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }

        let index = y as usize * self.width + x as usize;
        self.pixels[index] = alpha_over(self.pixels[index], color);
    }

    fn draw_stroke(&mut self, stroke: &StrokeAnnotation) {
        if let [point] = stroke.points[..] {
            self.draw_disc(point, (stroke.width.max(1) as i32) / 2, stroke.color);
            return;
        }

        for segment in stroke.points.windows(2) {
            self.draw_line(
                segment[0],
                segment[1],
                stroke.color,
                stroke.width.max(1) as i32,
            );
        }
    }

    fn draw_shape(&mut self, shape: &ShapeAnnotation) {
        let thickness = shape.width.max(1) as i32;
        match shape.kind {
            AnnotationShapeKind::Line => {
                self.draw_line(shape.start, shape.end, shape.color, thickness)
            }
            AnnotationShapeKind::Rectangle => {
                self.draw_rectangle(shape.start, shape.end, shape.color, thickness)
            }
            AnnotationShapeKind::Ellipse => {
                self.draw_ellipse(shape.start, shape.end, shape.color, thickness)
            }
        }
    }

    fn draw_crosshair_cursor(&mut self, cursor: AnnotationPoint) {
        for (color, thickness) in [(0xFF00_0000, 5), (0xFFFF_FFFF, 2)] {
            self.draw_line(
                cursor.offset(-12, 0),
                cursor.offset(12, 0),
                color,
                thickness,
            );
            self.draw_line(
                cursor.offset(0, -12),
                cursor.offset(0, 12),
                color,
                thickness,
            );
        }
        self.draw_disc(cursor, 4, 0xFFFF_C83D);
    }

    fn draw_hand_cursor(&mut self, cursor: AnnotationPoint) {
        // (dx, dy, width, height) of each outlined box, from the index finger down to the thumb.
        const BOXES: [(i32, i32, usize, usize); 7] = [
            (-2, 0, 5, 13),
            (2, 3, 4, 11),
            (5, 5, 4, 10),
            (8, 7, 4, 8),
            (-2, 12, 14, 10),
            (-8, 12, 7, 5),
            (-10, 15, 8, 5),
        ];

        for (dx, dy, box_width, box_height) in BOXES {
            let (x, y) = (cursor.x + dx, cursor.y + dy);
            self.fill_rect(x, y, box_width, box_height, 0xFF00_0000);
            if box_width > 2 && box_height > 2 {
                self.fill_rect(x + 1, y + 1, box_width - 2, box_height - 2, 0xFFFF_FFFF);
            }
        }
        self.fill_rect(cursor.x + 1, cursor.y + 15, 6, 3, 0xFFFF_C83D);
    }

    fn draw_text_annotation(&mut self, text: &TextAnnotation) {
        let scale = text.scale.max(1);
        let AnnotationPoint { x, y } = text.position;
        if text.text.is_empty() {
            self.fill_rect(
                x.max(0),
                y.max(0),
                2,
                TEXT_GLYPH_HEIGHT * scale,
                0xFFFF_C83D,
            );
            return;
        }

        self.draw_label(x + 2, y + 2, &text.text, scale, 0xC000_0000);
        self.draw_label(x, y, &text.text, scale, text.color);
    }

    fn draw_line(
        &mut self,
        start: AnnotationPoint,
        end: AnnotationPoint,
        color: u32,
        thickness: i32,
    ) {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let steps = dx.abs().max(dy.abs()).max(1);
        let radius = thickness.max(1) / 2;

        for step in 0..=steps {
            let t = step as f64 / steps as f64;
            let point = AnnotationPoint {
                x: (start.x as f64 + dx as f64 * t).round() as i32,
                y: (start.y as f64 + dy as f64 * t).round() as i32,
            };
            self.draw_disc(point, radius, color);
        }
    }

    fn draw_rectangle(
        &mut self,
        start: AnnotationPoint,
        end: AnnotationPoint,
        color: u32,
        thickness: i32,
    ) {
        let corner = |x, y| AnnotationPoint { x, y };
        let top_left = corner(start.x.min(end.x), start.y.min(end.y));
        let top_right = corner(start.x.max(end.x), start.y.min(end.y));
        let bottom_right = corner(start.x.max(end.x), start.y.max(end.y));
        let bottom_left = corner(start.x.min(end.x), start.y.max(end.y));

        self.draw_line(top_left, top_right, color, thickness);
        self.draw_line(top_right, bottom_right, color, thickness);
        self.draw_line(bottom_right, bottom_left, color, thickness);
        self.draw_line(bottom_left, top_left, color, thickness);
    }

    fn draw_ellipse(
        &mut self,
        start: AnnotationPoint,
        end: AnnotationPoint,
        color: u32,
        thickness: i32,
    ) {
        let left = start.x.min(end.x) as f64;
        let right = start.x.max(end.x) as f64;
        let top = start.y.min(end.y) as f64;
        let bottom = start.y.max(end.y) as f64;
        let rx = ((right - left) / 2.0).max(1.0);
        let ry = ((bottom - top) / 2.0).max(1.0);
        let cx = left + rx;
        let cy = top + ry;
        let step_count = (((rx + ry) * 3.0).round() as i32).max(24);

        for step in 0..=step_count {
            let theta = std::f64::consts::TAU * step as f64 / step_count as f64;
            let point = AnnotationPoint {
                x: (cx + rx * theta.cos()).round() as i32,
                y: (cy + ry * theta.sin()).round() as i32,
            };
            self.draw_disc(point, thickness.max(1) / 2, color);
        }
    }

    fn draw_disc(&mut self, center: AnnotationPoint, radius: i32, color: u32) {
        let radius = radius.max(1);
        let radius_sq = radius * radius;

        for y in (center.y - radius)..=(center.y + radius) {
            for x in (center.x - radius)..=(center.x + radius) {
                let dx = x - center.x;
                let dy = y - center.y;
                if dx * dx + dy * dy <= radius_sq {
                    self.blend_pixel(x, y, color);
                }
            }
        }
    }

    fn draw_label(&mut self, mut x: i32, y: i32, label: &str, scale: usize, color: u32) {
        for ch in label.chars() {
            x = self.draw_text_glyph(x, y, ch, scale, color);
        }
    }

    /// Draws one character and returns the x position of the next one.
    /// Characters without a glyph are drawn as their `U+XXXX` code point.
    fn draw_text_glyph(&mut self, x: i32, y: i32, ch: char, scale: usize, color: u32) -> i32 {
        let advance = (TEXT_GLYPH_ADVANCE * scale) as i32;
        if ch == ' ' {
            return x + advance;
        }

        let Some(glyph) = lookup_glyph(ch) else {
            let fallback = format!("U+{:04X}", ch as u32);
            return fallback
                .chars()
                .fold(x, |x, ch| self.draw_text_glyph(x, y, ch, scale, color));
        };

        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) == 0 {
                    continue;
                }

                let pixel_x = x + (col * scale) as i32;
                let pixel_y = y + (row * scale) as i32;
                // Glyph pixels starting left of or above the buffer are skipped whole.
                if pixel_x < 0 || pixel_y < 0 {
                    continue;
                }
                self.fill_rect(pixel_x, pixel_y, scale, scale, color);
            }
        }

        x + advance
    }

    fn draw_help_section(&mut self, x: usize, y: usize, section: &HelpSection) {
        let (heading, entries) = *section;
        self.draw_label(
            x as i32,
            y as i32,
            heading,
            HELP_HEADING_SCALE,
            HELP_HEADING_COLOR,
        );

        let mut row_y = y + HELP_HEADING_HEIGHT + HELP_ROW_GAP * 2;
        for (key, desc) in entries {
            self.draw_label(x as i32, row_y as i32, key, HELP_TEXT_SCALE, HELP_KEY_COLOR);
            self.draw_label(
                (x + HELP_KEY_COLUMN_WIDTH) as i32,
                row_y as i32,
                desc,
                HELP_TEXT_SCALE,
                HELP_TEXT_COLOR,
            );
            row_y += HELP_ROW_HEIGHT + HELP_ROW_GAP;
        }
    }
}

pub(crate) fn alpha_over(dst: u32, src: u32) -> u32 {
    let src_a = (src >> 24) & 0xFF;
    if src_a == 0 {
        return dst;
    }
    if src_a == 0xFF {
        return src;
    }

    let dst_a = (dst >> 24) & 0xFF;
    let inverse_src_a = 0xFF - src_a;

    let blend_channel = |src_shift: u32| -> u32 {
        let src_channel = (src >> src_shift) & 0xFF;
        let dst_channel = (dst >> src_shift) & 0xFF;
        src_channel + (dst_channel * inverse_src_a + 0x7F) / 0xFF
    };

    let out_a = src_a + (dst_a * inverse_src_a + 0x7F) / 0xFF;
    let out_r = blend_channel(16);
    let out_g = blend_channel(8);
    let out_b = blend_channel(0);

    (out_a << 24) | (out_r << 16) | (out_g << 8) | out_b
}

const HELP_TITLE_SCALE: usize = 3;
const HELP_HEADING_SCALE: usize = 2;
const HELP_TEXT_SCALE: usize = 2;
const HELP_KEY_COLOR: u32 = 0xFFFF_C83D;
const HELP_TEXT_COLOR: u32 = 0xFFE0_E0E0;
const HELP_HEADING_COLOR: u32 = 0xFFFF_FFFF;
const HELP_DIM_COLOR: u32 = 0xFF7A_7A7A;
const HELP_HEADING_HEIGHT: usize = TEXT_GLYPH_HEIGHT * HELP_HEADING_SCALE;
const HELP_ROW_HEIGHT: usize = TEXT_GLYPH_HEIGHT * HELP_TEXT_SCALE;
const HELP_ROW_GAP: usize = HELP_TEXT_SCALE * 3;
const HELP_KEY_COLUMN_WIDTH: usize = 14 * TEXT_GLYPH_ADVANCE * HELP_TEXT_SCALE;
const HELP_SECTION_WIDTH: usize = HELP_KEY_COLUMN_WIDTH + 18 * TEXT_GLYPH_ADVANCE * HELP_TEXT_SCALE;
const HELP_COLUMN_GAP: usize = 4 * TEXT_GLYPH_ADVANCE * HELP_TEXT_SCALE;
const HELP_PADDING: usize = 36;

type HelpSection = (&'static str, &'static [(&'static str, &'static str)]);

const HELP_NAVIGATION: HelpSection = (
    "NAVIGATION",
    &[
        ("Scroll", "Zoom at pointer"),
        ("Shift+Drag", "Box zoom area"),
        ("Drag", "Pan"),
        ("+/-", "Zoom center"),
        ("Arrows", "Pan"),
        ("0", "Reset view"),
        ("f", "Spotlight"),
        ("[ ]", "Spotlight size"),
        ("Dbl click", "Reset view"),
        ("Right click", "Exit"),
    ],
);

const HELP_COLOR_PICKER: HelpSection = (
    "COLOR PICKER",
    &[
        ("i", "Toggle picker"),
        ("Move", "Update swatch"),
        ("Left click", "Copy hex"),
        ("Esc", "Leave picker"),
    ],
);

const HELP_ANNOTATION: HelpSection = (
    "ANNOTATION",
    &[
        ("d", "Draw zoomed"),
        ("w", "Draw full view"),
        ("p/h/l/r/e", "Tools"),
        ("t", "Text mode"),
        ("m", "Move mode"),
        ("Drag", "Draw or move"),
        ("Click", "Place text"),
        ("1-0 - =", "Select color"),
        ("[ ]", "Text size"),
        ("Enter", "Commit text"),
        ("Backspace", "Delete char"),
        ("u", "Undo"),
        ("c", "Clear"),
        ("Esc", "Back"),
    ],
);

const HELP_GLOBAL: HelpSection = (
    "GLOBAL",
    &[
        ("s", "Save screenshot"),
        ("Ctrl+C", "Copy to clipboard"),
        ("?", "This help"),
    ],
);

const HELP_THREE_COLUMNS: &[&[HelpSection]] = &[
    &[HELP_NAVIGATION, HELP_GLOBAL],
    &[HELP_COLOR_PICKER],
    &[HELP_ANNOTATION],
];

const HELP_TWO_COLUMNS: &[&[HelpSection]] = &[
    &[HELP_NAVIGATION, HELP_COLOR_PICKER],
    &[HELP_ANNOTATION, HELP_GLOBAL],
];

fn help_section_height((_, entries): &HelpSection) -> usize {
    HELP_HEADING_HEIGHT + HELP_ROW_GAP * 2 + entries.len() * (HELP_ROW_HEIGHT + HELP_ROW_GAP)
}

pub fn paint_help_overlay(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    close_key: Option<CloseKey>,
) {
    let mut canvas = Canvas::new(pixels, width, height);
    canvas.pixels.fill(0xCC10_1018);

    let pad = HELP_PADDING;
    let columns_width =
        |count: usize| HELP_SECTION_WIDTH * count + HELP_COLUMN_GAP * (count - 1) + pad * 2;
    let columns = if width >= columns_width(3) + 80 {
        HELP_THREE_COLUMNS
    } else {
        HELP_TWO_COLUMNS
    };

    let title_h = TEXT_GLYPH_HEIGHT * HELP_TITLE_SCALE;
    let footer_h = TEXT_GLYPH_HEIGHT * HELP_TEXT_SCALE + pad;
    let body_h = columns
        .iter()
        .map(|column| {
            column
                .iter()
                .map(|s| help_section_height(s) + pad)
                .sum::<usize>()
                - pad
        })
        .max()
        .unwrap_or(0);

    let card_w = columns_width(columns.len()).min(width.saturating_sub(40));
    let card_h = (title_h + pad * 3 + body_h + footer_h).min(height.saturating_sub(40));
    let card_x = (width.saturating_sub(card_w)) / 2;
    let card_y = (height.saturating_sub(card_h)) / 2;

    canvas.blend_card(card_x, card_y, card_w, card_h);

    let content_x = card_x + pad;
    canvas.draw_label(
        content_x as i32,
        (card_y + pad) as i32,
        "KEYBOARD SHORTCUTS",
        HELP_TITLE_SCALE,
        HELP_HEADING_COLOR,
    );

    let body_y = card_y + pad + title_h + pad * 2;
    for (index, column) in columns.iter().enumerate() {
        let x = content_x + index * (HELP_SECTION_WIDTH + HELP_COLUMN_GAP);
        let mut y = body_y;
        for section in column.iter() {
            canvas.draw_help_section(x, y, section);
            y += help_section_height(section) + pad;
        }
    }

    let footer_text = format!("Press ? or {} to close", close_key_label(close_key));
    let footer_w = footer_text.len() * TEXT_GLYPH_ADVANCE * HELP_TEXT_SCALE;
    let footer_x = card_x + (card_w.saturating_sub(footer_w)) / 2;
    let footer_y = card_y + card_h - footer_h;
    canvas.draw_label(
        footer_x as i32,
        footer_y as i32,
        &footer_text,
        HELP_TEXT_SCALE,
        HELP_DIM_COLOR,
    );
}

fn lookup_glyph(ch: char) -> Option<&'static [u8; TEXT_GLYPH_HEIGHT]> {
    match ch {
        'A' => Some(&[0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]),
        'B' => Some(&[0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E]),
        'C' => Some(&[0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E]),
        'D' => Some(&[0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E]),
        'E' => Some(&[0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F]),
        'F' => Some(&[0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10]),
        'G' => Some(&[0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F]),
        'H' => Some(&[0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]),
        'I' => Some(&[0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1F]),
        'J' => Some(&[0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0E]),
        'K' => Some(&[0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11]),
        'L' => Some(&[0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F]),
        'M' => Some(&[0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11]),
        'N' => Some(&[0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11]),
        'O' => Some(&[0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
        'P' => Some(&[0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10]),
        'Q' => Some(&[0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D]),
        'R' => Some(&[0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11]),
        'S' => Some(&[0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E]),
        'T' => Some(&[0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04]),
        'U' => Some(&[0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
        'V' => Some(&[0x11, 0x11, 0x11, 0x11, 0x0A, 0x0A, 0x04]),
        'W' => Some(&[0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A]),
        'X' => Some(&[0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11]),
        'Y' => Some(&[0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04]),
        'Z' => Some(&[0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F]),
        'a' => Some(&[0x00, 0x00, 0x0E, 0x01, 0x0F, 0x11, 0x0F]),
        'b' => Some(&[0x10, 0x10, 0x1E, 0x11, 0x11, 0x11, 0x1E]),
        'c' => Some(&[0x00, 0x00, 0x0E, 0x10, 0x10, 0x11, 0x0E]),
        'd' => Some(&[0x01, 0x01, 0x0F, 0x11, 0x11, 0x11, 0x0F]),
        'e' => Some(&[0x00, 0x00, 0x0E, 0x11, 0x1F, 0x10, 0x0E]),
        'f' => Some(&[0x06, 0x09, 0x08, 0x1C, 0x08, 0x08, 0x08]),
        'g' => Some(&[0x00, 0x0F, 0x11, 0x11, 0x0F, 0x01, 0x0E]),
        'h' => Some(&[0x10, 0x10, 0x1E, 0x11, 0x11, 0x11, 0x11]),
        'i' => Some(&[0x04, 0x00, 0x0C, 0x04, 0x04, 0x04, 0x0E]),
        'j' => Some(&[0x02, 0x00, 0x06, 0x02, 0x02, 0x12, 0x0C]),
        'k' => Some(&[0x10, 0x10, 0x12, 0x14, 0x18, 0x14, 0x12]),
        'l' => Some(&[0x0C, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E]),
        'm' => Some(&[0x00, 0x00, 0x1A, 0x15, 0x15, 0x15, 0x15]),
        'n' => Some(&[0x00, 0x00, 0x1E, 0x11, 0x11, 0x11, 0x11]),
        'o' => Some(&[0x00, 0x00, 0x0E, 0x11, 0x11, 0x11, 0x0E]),
        'p' => Some(&[0x00, 0x00, 0x1E, 0x11, 0x1E, 0x10, 0x10]),
        'q' => Some(&[0x00, 0x00, 0x0F, 0x11, 0x0F, 0x01, 0x01]),
        'r' => Some(&[0x00, 0x00, 0x16, 0x19, 0x10, 0x10, 0x10]),
        's' => Some(&[0x00, 0x00, 0x0F, 0x10, 0x0E, 0x01, 0x1E]),
        't' => Some(&[0x08, 0x08, 0x1C, 0x08, 0x08, 0x09, 0x06]),
        'u' => Some(&[0x00, 0x00, 0x11, 0x11, 0x11, 0x13, 0x0D]),
        'v' => Some(&[0x00, 0x00, 0x11, 0x11, 0x11, 0x0A, 0x04]),
        'w' => Some(&[0x00, 0x00, 0x11, 0x11, 0x15, 0x15, 0x0A]),
        'x' => Some(&[0x00, 0x00, 0x11, 0x0A, 0x04, 0x0A, 0x11]),
        'y' => Some(&[0x00, 0x00, 0x11, 0x11, 0x0F, 0x01, 0x0E]),
        'z' => Some(&[0x00, 0x00, 0x1F, 0x02, 0x04, 0x08, 0x1F]),
        '0' => Some(&[0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E]),
        '1' => Some(&[0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E]),
        '2' => Some(&[0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F]),
        '3' => Some(&[0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E]),
        '4' => Some(&[0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02]),
        '5' => Some(&[0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E]),
        '6' => Some(&[0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E]),
        '7' => Some(&[0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08]),
        '8' => Some(&[0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E]),
        '9' => Some(&[0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C]),
        '!' => Some(&[0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04]),
        '"' => Some(&[0x0A, 0x0A, 0x0A, 0x00, 0x00, 0x00, 0x00]),
        '#' => Some(&[0x0A, 0x0A, 0x1F, 0x0A, 0x1F, 0x0A, 0x0A]),
        '\'' => Some(&[0x04, 0x04, 0x08, 0x00, 0x00, 0x00, 0x00]),
        '(' => Some(&[0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02]),
        ')' => Some(&[0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08]),
        '+' => Some(&[0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00]),
        ',' => Some(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x08]),
        '-' => Some(&[0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00]),
        '.' => Some(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x06]),
        '/' => Some(&[0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10]),
        ':' => Some(&[0x00, 0x06, 0x06, 0x00, 0x06, 0x06, 0x00]),
        ';' => Some(&[0x00, 0x06, 0x06, 0x00, 0x06, 0x04, 0x08]),
        '=' => Some(&[0x00, 0x00, 0x1F, 0x00, 0x1F, 0x00, 0x00]),
        '?' => Some(&[0x0E, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04]),
        '[' => Some(&[0x0E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0E]),
        '\\' => Some(&[0x10, 0x08, 0x08, 0x04, 0x02, 0x02, 0x01]),
        ']' => Some(&[0x0E, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0E]),
        '_' => Some(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{
        ActiveAnnotation, AnnotationItem, AnnotationPoint, AnnotationShapeKind, BadgeLine,
        BadgeModel, ShapeAnnotation, StrokeAnnotation, TextAnnotation,
    };

    use super::{
        AnnotationScene, Canvas, CursorStyle, OverlayCursor, draw_annotation_overlay,
        draw_spotlight_overlay, lookup_glyph, paint_color_picker, paint_toast, paint_zoom_badge,
    };

    #[test]
    fn fill_rect_clips_to_buffer_bounds() {
        let mut pixels = vec![0_u8; 4 * 4 * 4];
        Canvas::new(&mut pixels, 4, 4).fill_rect(2, 2, 4, 4, 0xFFFF_FFFF);

        let painted = pixels
            .chunks_exact(4)
            .filter(|chunk| *chunk == 0xFFFF_FFFF_u32.to_ne_bytes())
            .count();

        assert_eq!(painted, 4);
    }

    #[test]
    fn spotlight_leaves_center_transparent() {
        let mut pixels = vec![0_u8; 5 * 5 * 4];
        draw_spotlight_overlay(&mut pixels, 5, 5, 2, 2, 1.5);

        let center = &pixels[(2 * 5 + 2) * 4..(2 * 5 + 3) * 4];
        let edge = &pixels[0..4];

        assert_eq!(center, &0x0000_0000_u32.to_ne_bytes());
        assert_eq!(edge, &0xAA00_0000_u32.to_ne_bytes());
    }

    #[test]
    fn annotation_overlay_draws_committed_strokes() {
        let mut pixels = vec![0_u8; 32 * 32 * 4];
        let annotations = vec![AnnotationItem::Stroke(StrokeAnnotation {
            points: vec![
                AnnotationPoint { x: 4, y: 4 },
                AnnotationPoint { x: 20, y: 20 },
            ],
            color: 0xFFFF_0000,
            width: 4,
        })];

        draw_annotation_overlay(
            &mut pixels,
            32,
            32,
            &AnnotationScene {
                annotations,
                ..Default::default()
            },
        );

        assert!(pixels.chunks_exact(4).any(|chunk| chunk != [0, 0, 0, 0]));
    }

    #[test]
    fn annotation_overlay_draws_active_shape_preview() {
        let mut pixels = vec![0_u8; 48 * 48 * 4];
        let active = ActiveAnnotation::Shape(ShapeAnnotation {
            kind: AnnotationShapeKind::Rectangle,
            start: AnnotationPoint { x: 8, y: 8 },
            end: AnnotationPoint { x: 32, y: 24 },
            color: 0xFF00_FF00,
            width: 4,
        });

        draw_annotation_overlay(
            &mut pixels,
            48,
            48,
            &AnnotationScene {
                active_annotation: Some(active),
                ..Default::default()
            },
        );

        assert!(pixels.chunks_exact(4).any(|chunk| chunk != [0, 0, 0, 0]));
    }

    #[test]
    fn annotation_overlay_draws_cursor_marker() {
        let mut pixels = vec![0_u8; 48 * 48 * 4];

        draw_annotation_overlay(
            &mut pixels,
            48,
            48,
            &AnnotationScene {
                cursor: Some(OverlayCursor {
                    position: AnnotationPoint { x: 20, y: 18 },
                    style: CursorStyle::Crosshair,
                }),
                ..Default::default()
            },
        );

        assert!(pixels.chunks_exact(4).any(|chunk| chunk != [0, 0, 0, 0]));
    }

    #[test]
    fn annotation_overlay_draws_move_hand_cursor() {
        let mut pixels = vec![0_u8; 64 * 64 * 4];

        draw_annotation_overlay(
            &mut pixels,
            64,
            64,
            &AnnotationScene {
                cursor: Some(OverlayCursor {
                    position: AnnotationPoint { x: 20, y: 12 },
                    style: CursorStyle::Hand,
                }),
                ..Default::default()
            },
        );

        let hotspot = &pixels[(12 * 64 + 20) * 4..(12 * 64 + 21) * 4];
        assert_eq!(hotspot, &0xFF00_0000_u32.to_ne_bytes());
    }

    #[test]
    fn zoom_badge_renders_title_and_hints() {
        let mut pixels = vec![0_u8; 640 * 136 * 4];

        paint_zoom_badge(
            &mut pixels,
            640,
            136,
            &BadgeModel {
                title: "DRAW".to_owned(),
                subtitle: "Tool: PEN".to_owned(),
                lines: [
                    BadgeLine::new("DRAG", "Drag to draw", 0xFFFF_C83D),
                    BadgeLine::new("TOOL", "P/H paint  L/R/E shape", 0xFFF4_F4F4),
                    BadgeLine::new("EDIT", "M move  T text  U/C edit  Esc back", 0xFFF4_F4F4),
                ],
            },
        );

        assert!(pixels.chunks_exact(4).any(|chunk| chunk != [0, 0, 0, 0]));
    }

    #[test]
    fn toast_renders_notification_text() {
        let mut pixels = vec![0_u8; 320 * 32 * 4];

        paint_toast(
            &mut pixels,
            320,
            32,
            "SCREENSHOT SAVED",
            "File saved to shot.png",
        );

        assert!(pixels.chunks_exact(4).any(|chunk| chunk != [0, 0, 0, 0]));
    }

    #[test]
    fn color_picker_renders_swatch_and_hex_text() {
        let mut pixels = vec![0_u8; 320 * 112 * 4];

        paint_color_picker(&mut pixels, 320, 112, 0xFF12_34AB, "#1234AB", true);

        assert!(pixels.chunks_exact(4).any(|chunk| chunk != [0, 0, 0, 0]));
        assert!(lookup_glyph('#').is_some());
    }

    #[test]
    fn annotation_overlay_draws_text_annotations() {
        let mut pixels = vec![0_u8; 96 * 96 * 4];
        let text = TextAnnotation {
            position: AnnotationPoint { x: 10, y: 12 },
            text: "HELLO".to_owned(),
            color: 0xFFFF_FFFF,
            scale: 4,
        };

        draw_annotation_overlay(
            &mut pixels,
            96,
            96,
            &AnnotationScene {
                annotations: vec![AnnotationItem::Text(text)],
                ..Default::default()
            },
        );

        assert!(pixels.chunks_exact(4).any(|chunk| chunk != [0, 0, 0, 0]));
    }

    #[test]
    fn annotation_overlay_draws_lowercase_and_fallback_text() {
        let mut pixels = vec![0_u8; 160 * 96 * 4];
        let text = TextAnnotation {
            position: AnnotationPoint { x: 4, y: 12 },
            text: "hello é!?".to_owned(),
            color: 0xFFFF_FFFF,
            scale: 4,
        };

        draw_annotation_overlay(
            &mut pixels,
            160,
            96,
            &AnnotationScene {
                annotations: vec![AnnotationItem::Text(text)],
                ..Default::default()
            },
        );

        assert!(pixels.chunks_exact(4).any(|chunk| chunk != [0, 0, 0, 0]));
        assert!(lookup_glyph('h').is_some());
        assert!(lookup_glyph('!').is_some());
    }
}
