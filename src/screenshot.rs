use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bytemuck::cast_slice;
use png::{BitDepth, ColorType, Encoder};
use wayland_client::protocol::wl_shm;

use crate::{
    error::{AppError, Result},
    overlay::{self, ZOOM_BADGE_HEIGHT, ZOOM_BADGE_MARGIN, ZOOM_BADGE_WIDTH},
    render,
    state::AppState,
    zoom::ViewRect,
};

struct SourceFrame<'a> {
    pixels: &'a [u32],
    width: usize,
    height: usize,
    stride: usize,
    format: SourcePixelFormat,
}

struct OverlayImage<'a> {
    pixels: &'a [u32],
    width: usize,
    height: usize,
}

struct RenderedScreenshot {
    pixels: Vec<u32>,
    width: usize,
    height: usize,
}

#[derive(Clone, Copy)]
enum SourcePixelFormat {
    Argb8888,
    Xrgb8888,
    Abgr8888,
    Xbgr8888,
    Rgba8888,
    Rgbx8888,
    Bgra8888,
    Bgrx8888,
}

impl SourcePixelFormat {
    fn from_wl_shm(format: wl_shm::Format) -> Result<Self> {
        match format {
            wl_shm::Format::Argb8888 => Ok(Self::Argb8888),
            wl_shm::Format::Xrgb8888 => Ok(Self::Xrgb8888),
            wl_shm::Format::Abgr8888 => Ok(Self::Abgr8888),
            wl_shm::Format::Xbgr8888 => Ok(Self::Xbgr8888),
            wl_shm::Format::Rgba8888 => Ok(Self::Rgba8888),
            wl_shm::Format::Rgbx8888 => Ok(Self::Rgbx8888),
            wl_shm::Format::Bgra8888 => Ok(Self::Bgra8888),
            wl_shm::Format::Bgrx8888 => Ok(Self::Bgrx8888),
            _ => Err(AppError::runtime(format!(
                "unsupported wl_shm screenshot format: {format:?}"
            ))),
        }
    }
}

pub fn output_png_bytes(state: &AppState, output_id: u32) -> Result<Vec<u8>> {
    let rendered = render_output(state, output_id)?;
    encode_png_bytes(&rendered.pixels, rendered.width, rendered.height)
}

pub fn save_output(state: &AppState, output_id: u32) -> Result<PathBuf> {
    let png = output_png_bytes(state, output_id)?;
    let path = write_png(&state.config.screenshot_dir, &png)?;
    tracing::info!(path = %path.display(), output_id, "saved screenshot");
    if let Err(err) = emit_saved_path(&path) {
        tracing::warn!(path = %path.display(), error = %err, "failed to write screenshot path");
    }
    Ok(path)
}

fn render_output(state: &AppState, output_id: u32) -> Result<RenderedScreenshot> {
    let output = state
        .outputs
        .get(&output_id)
        .ok_or_else(|| AppError::runtime(format!("output {output_id} is not available")))?;
    let window = state
        .windows
        .get(&output_id)
        .ok_or_else(|| AppError::runtime(format!("window {output_id} is not available")))?;
    let source = output
        .buffer
        .as_ref()
        .ok_or_else(|| AppError::runtime(format!("output {output_id} has no captured buffer")))?;
    let source_format = SourcePixelFormat::from_wl_shm(source.format)?;

    let (logical_width, logical_height) = output.logical_size();
    if logical_width <= 0 || logical_height <= 0 {
        return Err(AppError::runtime(format!(
            "output {output_id} has invalid logical dimensions"
        )));
    }

    let logical_width = logical_width as usize;
    let logical_height = logical_height as usize;
    let source_pixels = cast_slice::<u8, u32>(source.data.as_ref());
    let mut frame = vec![0u32; logical_width * logical_height];

    render_view(
        &mut frame,
        (logical_width, logical_height),
        SourceFrame {
            pixels: source_pixels,
            width: source.width as usize,
            height: source.height as usize,
            stride: (source.stride / 4) as usize,
            format: source_format,
        },
        window.view_source,
    );

    if window.spotlight_visible {
        let mut spotlight = vec![0u8; logical_width * logical_height * 4];
        let radius = logical_width.min(logical_height) as f64 * state.spotlight_radius_frac;
        render::draw_spotlight_overlay(
            spotlight.as_mut_slice(),
            logical_width,
            logical_height,
            window.pointer_x.max(0.0) as usize,
            window.pointer_y.max(0.0) as usize,
            radius,
        );
        blend_full_frame(&mut frame, cast_slice::<u8, u32>(&spotlight));
    }

    if window.annotation_visible {
        let mut annotations = vec![0u8; logical_width * logical_height * 4];
        overlay::draw_annotation_overlay_snapshot(
            state,
            output_id,
            annotations.as_mut_slice(),
            logical_width,
            logical_height,
        );
        blend_full_frame(&mut frame, cast_slice::<u8, u32>(&annotations));
    }

    if window.zoom_badge_visible {
        let mut badge = vec![0u8; ZOOM_BADGE_WIDTH as usize * ZOOM_BADGE_HEIGHT as usize * 4];
        let badge_model = state
            .interaction_mode
            .badge(state.effective_annotation_tool(), state.config.close_key);
        render::paint_zoom_badge(
            badge.as_mut_slice(),
            ZOOM_BADGE_WIDTH as usize,
            ZOOM_BADGE_HEIGHT as usize,
            &badge_model,
        );
        blend_region(
            &mut frame,
            (logical_width, logical_height),
            (ZOOM_BADGE_MARGIN as usize, ZOOM_BADGE_MARGIN as usize),
            OverlayImage {
                pixels: cast_slice::<u8, u32>(&badge),
                width: ZOOM_BADGE_WIDTH as usize,
                height: ZOOM_BADGE_HEIGHT as usize,
            },
        );
    }

    Ok(RenderedScreenshot {
        pixels: frame,
        width: logical_width,
        height: logical_height,
    })
}

fn emit_saved_path(path: &Path) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", path.display())?;
    stdout.flush()
}

fn render_view(
    destination: &mut [u32],
    destination_size: (usize, usize),
    source: SourceFrame<'_>,
    view_source: ViewRect,
) {
    let (destination_width, destination_height) = destination_size;
    if destination.is_empty() || source.pixels.is_empty() || source.width == 0 || source.height == 0
    {
        return;
    }

    let x_scale = view_source.width / destination_width as f64;
    let y_scale = view_source.height / destination_height as f64;

    for y in 0..destination_height {
        let source_y = view_source.y + (y as f64 + 0.5) * y_scale - 0.5;
        for x in 0..destination_width {
            let source_x = view_source.x + (x as f64 + 0.5) * x_scale - 0.5;
            destination[y * destination_width + x] = sample_bilinear(&source, source_x, source_y);
        }
    }
}

fn sample_bilinear(source: &SourceFrame<'_>, x: f64, y: f64) -> u32 {
    let clamped_x = x.clamp(0.0, source.width.saturating_sub(1) as f64);
    let clamped_y = y.clamp(0.0, source.height.saturating_sub(1) as f64);
    let x0 = clamped_x.floor() as usize;
    let y0 = clamped_y.floor() as usize;
    let x1 = (x0 + 1).min(source.width.saturating_sub(1));
    let y1 = (y0 + 1).min(source.height.saturating_sub(1));
    let tx = clamped_x - x0 as f64;
    let ty = clamped_y - y0 as f64;

    let top = blend_samples(
        normalize_source_pixel(source.format, source.pixels[y0 * source.stride + x0]),
        normalize_source_pixel(source.format, source.pixels[y0 * source.stride + x1]),
        tx,
    );
    let bottom = blend_samples(
        normalize_source_pixel(source.format, source.pixels[y1 * source.stride + x0]),
        normalize_source_pixel(source.format, source.pixels[y1 * source.stride + x1]),
        tx,
    );
    blend_samples(top, bottom, ty)
}

fn normalize_source_pixel(format: SourcePixelFormat, pixel: u32) -> u32 {
    match format {
        SourcePixelFormat::Argb8888 => pixel,
        SourcePixelFormat::Xrgb8888 => pixel | 0xFF00_0000,
        SourcePixelFormat::Abgr8888 => {
            (pixel & 0xFF00_0000)
                | ((pixel & 0x0000_00FF) << 16)
                | (pixel & 0x0000_FF00)
                | ((pixel & 0x00FF_0000) >> 16)
        }
        SourcePixelFormat::Xbgr8888 => {
            0xFF00_0000
                | ((pixel & 0x0000_00FF) << 16)
                | (pixel & 0x0000_FF00)
                | ((pixel & 0x00FF_0000) >> 16)
        }
        SourcePixelFormat::Rgba8888 => {
            ((pixel & 0x0000_00FF) << 24)
                | ((pixel & 0xFF00_0000) >> 8)
                | ((pixel & 0x00FF_0000) >> 8)
                | ((pixel & 0x0000_FF00) >> 8)
        }
        SourcePixelFormat::Rgbx8888 => {
            0xFF00_0000
                | ((pixel & 0xFF00_0000) >> 8)
                | ((pixel & 0x00FF_0000) >> 8)
                | ((pixel & 0x0000_FF00) >> 8)
        }
        SourcePixelFormat::Bgra8888 => {
            ((pixel & 0x0000_00FF) << 24)
                | ((pixel & 0x0000_FF00) << 8)
                | ((pixel & 0x00FF_0000) >> 8)
                | ((pixel & 0xFF00_0000) >> 24)
        }
        SourcePixelFormat::Bgrx8888 => {
            0xFF00_0000
                | ((pixel & 0x0000_FF00) << 8)
                | ((pixel & 0x00FF_0000) >> 8)
                | ((pixel & 0xFF00_0000) >> 24)
        }
    }
}

fn blend_samples(left: u32, right: u32, amount: f64) -> u32 {
    let inverse = 1.0 - amount;
    let a = (((left >> 24) & 0xFF) as f64 * inverse + ((right >> 24) & 0xFF) as f64 * amount)
        .round() as u32;
    let r = (((left >> 16) & 0xFF) as f64 * inverse + ((right >> 16) & 0xFF) as f64 * amount)
        .round() as u32;
    let g = (((left >> 8) & 0xFF) as f64 * inverse + ((right >> 8) & 0xFF) as f64 * amount).round()
        as u32;
    let b = ((left & 0xFF) as f64 * inverse + (right & 0xFF) as f64 * amount).round() as u32;
    (a << 24) | (r << 16) | (g << 8) | b
}

fn blend_full_frame(destination: &mut [u32], overlay: &[u32]) {
    for (destination_pixel, overlay_pixel) in destination.iter_mut().zip(overlay.iter().copied()) {
        *destination_pixel = render::alpha_over(*destination_pixel, overlay_pixel);
    }
}

fn blend_region(
    destination: &mut [u32],
    destination_size: (usize, usize),
    offset: (usize, usize),
    overlay: OverlayImage<'_>,
) {
    let (destination_width, destination_height) = destination_size;
    let (offset_x, offset_y) = offset;

    for y in 0..overlay.height {
        let destination_y = offset_y + y;
        if destination_y >= destination_height {
            break;
        }

        for x in 0..overlay.width {
            let destination_x = offset_x + x;
            if destination_x >= destination_width {
                break;
            }

            let destination_index = destination_y * destination_width + destination_x;
            let overlay_index = y * overlay.width + x;
            destination[destination_index] = render::alpha_over(
                destination[destination_index],
                overlay.pixels[overlay_index],
            );
        }
    }
}

fn write_png(directory: &Path, png: &[u8]) -> Result<PathBuf> {
    fs::create_dir_all(directory).map_err(|err| AppError::screenshot(directory, err))?;

    let (file, path) = create_output_file(directory)?;
    let mut writer = std::io::BufWriter::new(file);
    writer
        .write_all(png)
        .map_err(|err| AppError::screenshot(&path, err))?;
    writer
        .flush()
        .map_err(|err| AppError::screenshot(&path, err))?;
    Ok(path)
}

fn encode_png_bytes(pixels: &[u32], width: usize, height: usize) -> Result<Vec<u8>> {
    let mut encoded = Vec::new();
    let writer = std::io::BufWriter::new(&mut encoded);
    let mut encoder = Encoder::new(writer, width as u32, height as u32);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut png_writer = encoder
        .write_header()
        .map_err(|err| AppError::runtime(format!("failed to encode screenshot as PNG: {err}")))?;
    let rgba = rgba_bytes(pixels);
    png_writer
        .write_image_data(&rgba)
        .map_err(|err| AppError::runtime(format!("failed to encode screenshot as PNG: {err}")))?;
    png_writer
        .finish()
        .map_err(|err| AppError::runtime(format!("failed to encode screenshot as PNG: {err}")))?;
    Ok(encoded)
}

fn create_output_file(directory: &Path) -> Result<(std::fs::File, PathBuf)> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AppError::runtime("system clock is before the Unix epoch"))?;
    let stem = format!("shmooz-{}-{:03}", now.as_secs(), now.subsec_millis());

    for suffix in 0..1024 {
        let filename = if suffix == 0 {
            format!("{stem}.png")
        } else {
            format!("{stem}-{suffix}.png")
        };
        let path = directory.join(filename);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(AppError::screenshot(&path, err)),
        }
    }

    Err(AppError::runtime(format!(
        "could not allocate a unique screenshot filename in {}",
        directory.display()
    )))
}

fn rgba_bytes(pixels: &[u32]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for &pixel in pixels {
        rgba.extend_from_slice(&[
            ((pixel >> 16) & 0xFF) as u8,
            ((pixel >> 8) & 0xFF) as u8,
            (pixel & 0xFF) as u8,
            ((pixel >> 24) & 0xFF) as u8,
        ]);
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::{SourceFrame, SourcePixelFormat, encode_png_bytes, render_view};
    use crate::{render, zoom::ViewRect};
    use wayland_client::protocol::wl_shm;

    #[test]
    fn blend_pixel_composites_premultiplied_overlay() {
        let destination = 0xFF20_3040;
        let overlay = 0x8080_0000;

        assert_eq!(render::alpha_over(destination, overlay), 0xFF90_1820);
    }

    #[test]
    fn render_view_tracks_zoomed_source_rectangle() {
        let source = [0xFFFF_0000, 0xFF00_FF00, 0xFF00_00FF, 0xFFFF_FFFF];
        let mut destination = [0u32; 1];

        render_view(
            &mut destination,
            (1, 1),
            SourceFrame {
                pixels: &source,
                width: 2,
                height: 2,
                stride: 2,
                format: SourcePixelFormat::Argb8888,
            },
            ViewRect {
                x: 1.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
        );

        assert_eq!(destination[0], 0xFF00_FF00);
    }

    #[test]
    fn render_view_uses_source_stride_for_padded_rows() {
        let source = [
            0xFFFF_0000,
            0xFF00_FF00,
            0x0000_0000,
            0xFF00_00FF,
            0xFFFF_FFFF,
            0x0000_0000,
        ];
        let mut destination = [0u32; 1];

        render_view(
            &mut destination,
            (1, 1),
            SourceFrame {
                pixels: &source,
                width: 2,
                height: 2,
                stride: 3,
                format: SourcePixelFormat::Argb8888,
            },
            ViewRect {
                x: 1.0,
                y: 1.0,
                width: 1.0,
                height: 1.0,
            },
        );

        assert_eq!(destination[0], 0xFFFF_FFFF);
    }

    #[test]
    fn render_view_normalizes_supported_8888_formats() {
        let cases = [
            (SourcePixelFormat::Argb8888, 0xAA11_2233, 0xAA11_2233),
            (SourcePixelFormat::Xrgb8888, 0x0011_2233, 0xFF11_2233),
            (SourcePixelFormat::Abgr8888, 0xAA33_2211, 0xAA11_2233),
            (SourcePixelFormat::Xbgr8888, 0x0033_2211, 0xFF11_2233),
            (SourcePixelFormat::Rgba8888, 0x1122_33AA, 0xAA11_2233),
            (SourcePixelFormat::Rgbx8888, 0x1122_3300, 0xFF11_2233),
            (SourcePixelFormat::Bgra8888, 0x3322_11AA, 0xAA11_2233),
            (SourcePixelFormat::Bgrx8888, 0x3322_1100, 0xFF11_2233),
        ];

        for (format, source_pixel, expected) in cases {
            let source = [source_pixel];
            let mut destination = [0u32; 1];

            render_view(
                &mut destination,
                (1, 1),
                SourceFrame {
                    pixels: &source,
                    width: 1,
                    height: 1,
                    stride: 1,
                    format,
                },
                ViewRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
            );

            assert_eq!(destination[0], expected);
        }
    }

    #[test]
    fn screenshot_format_rejects_non_8888_layouts() {
        assert!(SourcePixelFormat::from_wl_shm(wl_shm::Format::Rgb888).is_err());
    }

    #[test]
    fn encode_png_bytes_writes_png_signature() {
        let encoded = encode_png_bytes(&[0xFFFF_0000], 1, 1).unwrap();

        assert_eq!(&encoded[..8], b"\x89PNG\r\n\x1a\n");
    }
}
