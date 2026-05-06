use bytemuck::cast_slice_mut;

const ZOOM_BADGE_ICON_SIZE: usize = 34;
const ZOOM_BADGE_ICON_STROKE: usize = 4;
const ZOOM_BADGE_FONT_SCALE: usize = 4;

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

    for y in 0..height {
        let row = &mut pixels[y * width..(y + 1) * width];
        row.fill(0xAA00_0000);

        let dy = y as i32 - center_y;
        let dy_sq = f64::from(dy * dy);
        if dy_sq >= radius_sq {
            continue;
        }

        let half_width = (radius_sq - dy_sq).sqrt() as i32;
        let x0 = (center_x - half_width).clamp(0, width.saturating_sub(1) as i32) as usize;
        let x1 = (center_x + half_width + 1).clamp(0, width as i32) as usize;
        row[x0..x1].fill(0x0000_0000);
    }
}

pub fn paint_zoom_badge(pixels: &mut [u8], width: usize, height: usize) {
    pixels_u32(pixels).fill(0);

    fill_rect(pixels, width, height, 0, 0, width, height, 0xD91A_1A1A);
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

    draw_zoom_badge_icon(pixels, width, height, 16, 19);
    draw_zoom_badge_label(pixels, width, height, 68, 22, "ZOOM MODE");
}

fn draw_zoom_badge_icon(pixels: &mut [u8], width: usize, height: usize, x: usize, y: usize) {
    let pixels = pixels_u32(pixels);
    let radius = ZOOM_BADGE_ICON_SIZE / 2 - 3;
    let center_x = x + radius + 2;
    let center_y = y + radius + 2;
    let inner_radius = radius.saturating_sub(ZOOM_BADGE_ICON_STROKE);
    let outer_sq = (radius * radius) as i64;
    let inner_sq = (inner_radius * inner_radius) as i64;
    let ring = 0xFFFF_C83D;
    let handle = 0xFFFF_FFFF;

    for yy in y..(y + ZOOM_BADGE_ICON_SIZE).min(height) {
        for xx in x..(x + ZOOM_BADGE_ICON_SIZE).min(width) {
            let dx = xx as i64 - center_x as i64;
            let dy = yy as i64 - center_y as i64;
            let dist_sq = dx * dx + dy * dy;
            if dist_sq <= outer_sq && dist_sq >= inner_sq {
                pixels[yy * width + xx] = ring;
            }
        }
    }

    for offset in 0..12 {
        fill_rect_pixels(
            pixels,
            width,
            height,
            x + 22 + offset,
            y + 24 + offset,
            5,
            10,
            handle,
        );
    }
}

fn draw_zoom_badge_label(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    mut x: usize,
    y: usize,
    label: &str,
) {
    let pixels = pixels_u32(pixels);
    let color = 0xFFFF_FFFF;

    for ch in label.chars() {
        let Some(glyph) = lookup_zoom_badge_glyph(ch) else {
            x += 6 * ZOOM_BADGE_FONT_SCALE;
            continue;
        };

        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) == 0 {
                    continue;
                }
                fill_rect_pixels(
                    pixels,
                    width,
                    height,
                    x + col * ZOOM_BADGE_FONT_SCALE,
                    y + row * ZOOM_BADGE_FONT_SCALE,
                    ZOOM_BADGE_FONT_SCALE,
                    ZOOM_BADGE_FONT_SCALE,
                    color,
                );
            }
        }

        x += 6 * ZOOM_BADGE_FONT_SCALE;
    }
}

fn lookup_zoom_badge_glyph(ch: char) -> Option<&'static [u8; 7]> {
    match ch {
        'D' => Some(&[0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E]),
        'E' => Some(&[0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F]),
        'M' => Some(&[0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11]),
        'O' => Some(&[0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
        'Z' => Some(&[0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F]),
        _ => None,
    }
}

fn pixels_u32(pixels: &mut [u8]) -> &mut [u32] {
    cast_slice_mut(pixels)
}

#[cfg(test)]
mod tests {
    use super::{draw_spotlight_overlay, fill_rect};

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
}
