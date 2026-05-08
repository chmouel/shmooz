use bytemuck::cast_slice_mut;

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

#[allow(clippy::too_many_arguments)]
pub fn fill_rect(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    rect_width: usize,
    rect_height: usize,
    color: u32,
) {
    fill_rect_pixels(
        pixels_u32(pixels),
        width,
        height,
        x,
        y,
        rect_width,
        rect_height,
        color,
    );
}

#[allow(clippy::too_many_arguments)]
fn fill_rect_pixels(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    rect_width: usize,
    rect_height: usize,
    color: u32,
) {
    let x0 = x.min(width);
    let y0 = y.min(height);
    let x1 = x.saturating_add(rect_width).min(width);
    let y1 = y.saturating_add(rect_height).min(height);

    for yy in y0..y1 {
        pixels[yy * width + x0..yy * width + x1].fill(color);
    }
}

pub fn draw_spotlight_overlay(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    center_x: usize,
    center_y: usize,
    radius: f64,
) {
    let pixels = pixels_u32(pixels);
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
    annotations: &[AnnotationItem],
    active_annotation: Option<&ActiveAnnotation>,
    active_text: Option<&TextAnnotation>,
    cursor: Option<OverlayCursor>,
) {
    let pixels = pixels_u32(pixels);
    pixels.fill(0);

    for annotation in annotations {
        draw_annotation_item(pixels, width, height, annotation);
    }

    if let Some(active_annotation) = active_annotation {
        match active_annotation {
            ActiveAnnotation::Stroke(stroke) => draw_stroke(pixels, width, height, stroke),
            ActiveAnnotation::Shape(shape) => draw_shape(pixels, width, height, shape),
        }
    }

    if let Some(active_text) = active_text {
        draw_text_annotation(pixels, width, height, active_text);
    }

    if let Some(cursor) = cursor {
        draw_cursor_marker(pixels, width, height, cursor);
    }
}

fn paint_badge_background(pixels: &mut [u8], width: usize, height: usize) {
    fill_rect(pixels, width, height, 0, 0, width, height, 0xE014_1414);
    fill_rect(pixels, width, height, 0, 0, width, 3, 0xFFFF_C83D);
    fill_rect(
        pixels,
        width,
        height,
        0,
        height.saturating_sub(3),
        width,
        3,
        0xFFFF_C83D,
    );
    fill_rect(pixels, width, height, 0, 0, 5, height, 0xAA4A_2812);
}

pub fn paint_zoom_badge(pixels: &mut [u8], width: usize, height: usize, badge: &BadgeModel) {
    pixels_u32(pixels).fill(0);

    paint_badge_background(pixels, width, height);
    fill_rect(
        pixels,
        width,
        height,
        18,
        52,
        width.saturating_sub(36),
        1,
        0x5030_3030,
    );
    fill_rect(
        pixels,
        width,
        height,
        18,
        80,
        width.saturating_sub(36),
        1,
        0x4430_3030,
    );
    fill_rect(
        pixels,
        width,
        height,
        18,
        104,
        width.saturating_sub(36),
        1,
        0x4430_3030,
    );

    draw_label(
        pixels,
        width,
        height,
        18,
        12,
        &badge.title,
        BADGE_TITLE_SCALE,
        0xFFFF_FFFF,
    );
    draw_label(
        pixels,
        width,
        height,
        18,
        34,
        &badge.subtitle,
        BADGE_SUBTITLE_SCALE,
        0xFFE7_BD73,
    );

    let row_y = [60, 84, 108];
    for (index, line) in badge.lines.iter().enumerate() {
        let y = row_y[index];
        draw_label(
            pixels,
            width,
            height,
            18,
            y,
            line.label,
            BADGE_HINT_SCALE,
            0xFFFF_C83D,
        );
        draw_label(
            pixels,
            width,
            height,
            88,
            y,
            &line.text,
            BADGE_HINT_SCALE,
            line.color,
        );
    }
}

pub fn paint_toast(pixels: &mut [u8], width: usize, height: usize, title: &str, message: &str) {
    pixels_u32(pixels).fill(0);

    paint_badge_background(pixels, width, height);
    fill_rect(
        pixels,
        width,
        height,
        18,
        54,
        width.saturating_sub(36),
        1,
        0x5030_3030,
    );

    draw_label(
        pixels,
        width,
        height,
        18,
        16,
        title,
        TOAST_TITLE_SCALE,
        0xFFFF_FFFF,
    );
    draw_label(
        pixels,
        width,
        height,
        18,
        40,
        "NOTIFICATION",
        TOAST_TEXT_SCALE,
        0xFFE7_BD73,
    );
    draw_label(
        pixels,
        width,
        height,
        18,
        68,
        message,
        TOAST_TEXT_SCALE,
        0xFFFF_FFFF,
    );
}

fn draw_annotation_item(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    annotation: &AnnotationItem,
) {
    match annotation {
        AnnotationItem::Stroke(stroke) => draw_stroke(pixels, width, height, stroke),
        AnnotationItem::Shape(shape) => draw_shape(pixels, width, height, shape),
        AnnotationItem::Text(text) => draw_text_annotation(pixels, width, height, text),
    }
}

fn draw_stroke(pixels: &mut [u32], width: usize, height: usize, stroke: &StrokeAnnotation) {
    if stroke.points.len() == 1 {
        let point = stroke.points[0];
        draw_disc(
            pixels,
            width,
            height,
            point.x,
            point.y,
            (stroke.width.max(1) as i32) / 2,
            stroke.color,
        );
        return;
    }

    for segment in stroke.points.windows(2) {
        let start = segment[0];
        let end = segment[1];
        draw_line(
            pixels,
            width,
            height,
            start.x,
            start.y,
            end.x,
            end.y,
            stroke.color,
            stroke.width.max(1) as i32,
        );
    }
}

fn draw_shape(pixels: &mut [u32], width: usize, height: usize, shape: &ShapeAnnotation) {
    match shape.kind {
        AnnotationShapeKind::Line => draw_line(
            pixels,
            width,
            height,
            shape.start.x,
            shape.start.y,
            shape.end.x,
            shape.end.y,
            shape.color,
            shape.width.max(1) as i32,
        ),
        AnnotationShapeKind::Rectangle => draw_rectangle(
            pixels,
            width,
            height,
            shape.start.x,
            shape.start.y,
            shape.end.x,
            shape.end.y,
            shape.color,
            shape.width.max(1) as i32,
        ),
        AnnotationShapeKind::Ellipse => draw_ellipse(
            pixels,
            width,
            height,
            shape.start.x,
            shape.start.y,
            shape.end.x,
            shape.end.y,
            shape.color,
            shape.width.max(1) as i32,
        ),
    }
}

fn draw_cursor_marker(pixels: &mut [u32], width: usize, height: usize, cursor: OverlayCursor) {
    match cursor.style {
        CursorStyle::Crosshair => draw_crosshair_cursor(pixels, width, height, cursor.position),
        CursorStyle::Hand => draw_hand_cursor(pixels, width, height, cursor.position),
    }
}

fn draw_crosshair_cursor(pixels: &mut [u32], width: usize, height: usize, cursor: AnnotationPoint) {
    draw_line(
        pixels,
        width,
        height,
        cursor.x - 12,
        cursor.y,
        cursor.x + 12,
        cursor.y,
        0xFF00_0000,
        5,
    );
    draw_line(
        pixels,
        width,
        height,
        cursor.x,
        cursor.y - 12,
        cursor.x,
        cursor.y + 12,
        0xFF00_0000,
        5,
    );
    draw_line(
        pixels,
        width,
        height,
        cursor.x - 12,
        cursor.y,
        cursor.x + 12,
        cursor.y,
        0xFFFF_FFFF,
        2,
    );
    draw_line(
        pixels,
        width,
        height,
        cursor.x,
        cursor.y - 12,
        cursor.x,
        cursor.y + 12,
        0xFFFF_FFFF,
        2,
    );
    draw_disc(pixels, width, height, cursor.x, cursor.y, 4, 0xFFFF_C83D);
}

fn draw_hand_cursor(pixels: &mut [u32], width: usize, height: usize, cursor: AnnotationPoint) {
    const OUTLINE: u32 = 0xFF00_0000;
    const FILL: u32 = 0xFFFF_FFFF;
    const ACCENT: u32 = 0xFFFF_C83D;

    draw_box(
        pixels,
        width,
        height,
        cursor.x - 2,
        cursor.y,
        5,
        13,
        OUTLINE,
        FILL,
    );
    draw_box(
        pixels,
        width,
        height,
        cursor.x + 2,
        cursor.y + 3,
        4,
        11,
        OUTLINE,
        FILL,
    );
    draw_box(
        pixels,
        width,
        height,
        cursor.x + 5,
        cursor.y + 5,
        4,
        10,
        OUTLINE,
        FILL,
    );
    draw_box(
        pixels,
        width,
        height,
        cursor.x + 8,
        cursor.y + 7,
        4,
        8,
        OUTLINE,
        FILL,
    );
    draw_box(
        pixels,
        width,
        height,
        cursor.x - 2,
        cursor.y + 12,
        14,
        10,
        OUTLINE,
        FILL,
    );
    draw_box(
        pixels,
        width,
        height,
        cursor.x - 8,
        cursor.y + 12,
        7,
        5,
        OUTLINE,
        FILL,
    );
    draw_box(
        pixels,
        width,
        height,
        cursor.x - 10,
        cursor.y + 15,
        8,
        5,
        OUTLINE,
        FILL,
    );
    fill_rect_i32(
        pixels,
        width,
        height,
        cursor.x + 1,
        cursor.y + 15,
        6,
        3,
        ACCENT,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_box(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    box_width: usize,
    box_height: usize,
    outline: u32,
    fill: u32,
) {
    fill_rect_i32(pixels, width, height, x, y, box_width, box_height, outline);
    if box_width > 2 && box_height > 2 {
        fill_rect_i32(
            pixels,
            width,
            height,
            x + 1,
            y + 1,
            box_width - 2,
            box_height - 2,
            fill,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_rect_i32(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    rect_width: usize,
    rect_height: usize,
    color: u32,
) {
    let x0 = x.max(0) as usize;
    let y0 = y.max(0) as usize;
    let x1 = x.saturating_add(rect_width as i32).max(0) as usize;
    let y1 = y.saturating_add(rect_height as i32).max(0) as usize;
    fill_rect_pixels(
        pixels,
        width,
        height,
        x0.min(width),
        y0.min(height),
        x1.saturating_sub(x0)
            .min(width.saturating_sub(x0.min(width))),
        y1.saturating_sub(y0)
            .min(height.saturating_sub(y0.min(height))),
        color,
    );
}

fn draw_text_annotation(pixels: &mut [u32], width: usize, height: usize, text: &TextAnnotation) {
    if text.text.is_empty() {
        fill_rect_pixels(
            pixels,
            width,
            height,
            text.position.x.max(0) as usize,
            text.position.y.max(0) as usize,
            2,
            TEXT_GLYPH_HEIGHT * text.scale.max(1),
            0xFFFF_C83D,
        );
        return;
    }

    draw_label_pixels(
        pixels,
        width,
        height,
        text.position.x + 2,
        text.position.y + 2,
        &text.text,
        text.scale.max(1),
        0xC000_0000,
    );
    draw_label_pixels(
        pixels,
        width,
        height,
        text.position.x,
        text.position.y,
        &text.text,
        text.scale.max(1),
        text.color,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_line(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: u32,
    thickness: i32,
) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let steps = dx.abs().max(dy.abs()).max(1);
    let radius = thickness.max(1) / 2;

    for step in 0..=steps {
        let t = step as f64 / steps as f64;
        let x = x0 as f64 + dx as f64 * t;
        let y = y0 as f64 + dy as f64 * t;
        draw_disc(
            pixels,
            width,
            height,
            x.round() as i32,
            y.round() as i32,
            radius,
            color,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_rectangle(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: u32,
    thickness: i32,
) {
    let left = x0.min(x1);
    let right = x0.max(x1);
    let top = y0.min(y1);
    let bottom = y0.max(y1);

    draw_line(
        pixels, width, height, left, top, right, top, color, thickness,
    );
    draw_line(
        pixels, width, height, right, top, right, bottom, color, thickness,
    );
    draw_line(
        pixels, width, height, right, bottom, left, bottom, color, thickness,
    );
    draw_line(
        pixels, width, height, left, bottom, left, top, color, thickness,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_ellipse(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: u32,
    thickness: i32,
) {
    let left = x0.min(x1) as f64;
    let right = x0.max(x1) as f64;
    let top = y0.min(y1) as f64;
    let bottom = y0.max(y1) as f64;
    let rx = ((right - left) / 2.0).max(1.0);
    let ry = ((bottom - top) / 2.0).max(1.0);
    let cx = left + rx;
    let cy = top + ry;
    let step_count = ((rx + ry) * 3.0).round() as i32;
    let step_count = step_count.max(24);

    for step in 0..=step_count {
        let theta = std::f64::consts::TAU * step as f64 / step_count as f64;
        let x = cx + rx * theta.cos();
        let y = cy + ry * theta.sin();
        draw_disc(
            pixels,
            width,
            height,
            x.round() as i32,
            y.round() as i32,
            thickness.max(1) / 2,
            color,
        );
    }
}

fn draw_disc(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    center_x: i32,
    center_y: i32,
    radius: i32,
    color: u32,
) {
    let radius = radius.max(1);
    let radius_sq = radius * radius;

    for y in (center_y - radius)..=(center_y + radius) {
        for x in (center_x - radius)..=(center_x + radius) {
            let dx = x - center_x;
            let dy = y - center_y;
            if dx * dx + dy * dy <= radius_sq {
                blend_pixel(pixels, width, height, x, y, color);
            }
        }
    }
}

fn blend_pixel(pixels: &mut [u32], width: usize, height: usize, x: i32, y: i32, color: u32) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }

    let index = y as usize * width + x as usize;
    pixels[index] = alpha_over(pixels[index], color);
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

#[allow(clippy::too_many_arguments)]
fn draw_label(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    label: &str,
    scale: usize,
    color: u32,
) {
    let pixels = pixels_u32(pixels);
    draw_label_pixels(
        pixels, width, height, x as i32, y as i32, label, scale, color,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_label_pixels(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    mut x: i32,
    y: i32,
    label: &str,
    scale: usize,
    color: u32,
) {
    for ch in label.chars() {
        x = draw_text_glyph(pixels, width, height, x, y, ch, scale, color);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text_glyph(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    mut x: i32,
    y: i32,
    ch: char,
    scale: usize,
    color: u32,
) -> i32 {
    if ch == ' ' {
        return x + (6 * scale) as i32;
    }

    if let Some(glyph) = lookup_glyph(ch) {
        return draw_bitmap_glyph(pixels, width, height, x, y, glyph, scale, color);
    }

    let fallback = format!("U+{:04X}", ch as u32);
    for fallback_ch in fallback.chars() {
        x = draw_text_glyph(pixels, width, height, x, y, fallback_ch, scale, color);
    }
    x
}

#[allow(clippy::too_many_arguments)]
fn draw_bitmap_glyph(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    glyph: &[u8; TEXT_GLYPH_HEIGHT],
    scale: usize,
    color: u32,
) -> i32 {
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5 {
            if bits & (1 << (4 - col)) == 0 {
                continue;
            }

            let pixel_x = x + (col * scale) as i32;
            let pixel_y = y + (row * scale) as i32;
            if pixel_x < 0 || pixel_y < 0 {
                continue;
            }

            fill_rect_pixels(
                pixels,
                width,
                height,
                pixel_x as usize,
                pixel_y as usize,
                scale,
                scale,
                color,
            );
        }
    }

    x + (6 * scale) as i32
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

fn pixels_u32(pixels: &mut [u8]) -> &mut [u32] {
    cast_slice_mut(pixels)
}

#[cfg(test)]
mod tests {
    use crate::state::{
        ActiveAnnotation, AnnotationItem, AnnotationPoint, AnnotationShapeKind, BadgeLine,
        BadgeModel, ShapeAnnotation, StrokeAnnotation, TextAnnotation,
    };

    use super::{
        CursorStyle, OverlayCursor, draw_annotation_overlay, draw_spotlight_overlay, fill_rect,
        lookup_glyph, paint_toast, paint_zoom_badge,
    };

    #[test]
    fn fill_rect_clips_to_buffer_bounds() {
        let mut pixels = vec![0_u8; 4 * 4 * 4];
        fill_rect(&mut pixels, 4, 4, 2, 2, 4, 4, 0xFFFF_FFFF);

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

        draw_annotation_overlay(&mut pixels, 32, 32, &annotations, None, None, None);

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

        draw_annotation_overlay(&mut pixels, 48, 48, &[], Some(&active), None, None);

        assert!(pixels.chunks_exact(4).any(|chunk| chunk != [0, 0, 0, 0]));
    }

    #[test]
    fn annotation_overlay_draws_cursor_marker() {
        let mut pixels = vec![0_u8; 48 * 48 * 4];

        draw_annotation_overlay(
            &mut pixels,
            48,
            48,
            &[],
            None,
            None,
            Some(OverlayCursor {
                position: AnnotationPoint { x: 20, y: 18 },
                style: CursorStyle::Crosshair,
            }),
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
            &[],
            None,
            None,
            Some(OverlayCursor {
                position: AnnotationPoint { x: 20, y: 12 },
                style: CursorStyle::Hand,
            }),
        );

        let hotspot = &pixels[(12 * 64 + 20) * 4..(12 * 64 + 21) * 4];
        assert_eq!(hotspot, &0xFF00_0000_u32.to_ne_bytes());
    }

    #[test]
    fn zoom_badge_renders_title_and_hints() {
        let mut pixels = vec![0_u8; 480 * 136 * 4];

        paint_zoom_badge(
            &mut pixels,
            480,
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
            &[AnnotationItem::Text(text)],
            None,
            None,
            None,
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
            &[AnnotationItem::Text(text)],
            None,
            None,
            None,
        );

        assert!(pixels.chunks_exact(4).any(|chunk| chunk != [0, 0, 0, 0]));
        assert!(lookup_glyph('h').is_some());
        assert!(lookup_glyph('!').is_some());
    }
}
