pub const MIN_VIEW_HEIGHT: f64 = 16.0;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ViewRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl ViewRect {
    pub fn full(buffer_size: Size) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: buffer_size.width,
            height: buffer_size.height,
        }
    }
}

pub fn restore_view(view: &mut ViewRect, initial: ViewRect) {
    *view = initial;
}

pub fn zoom_towards_factor(
    view: &mut ViewRect,
    factor: f64,
    center: Point,
    logical_size: Size,
    buffer_size: Size,
) {
    if !factor.is_finite() || factor <= 0.0 {
        return;
    }

    let ratio = aspect_ratio(logical_size, buffer_size);
    let (width, height) = scaled_view_size(*view, factor, ratio, buffer_size);
    if width == view.width && height == view.height {
        return;
    }

    let dx = normalized_axis(center.x, logical_size.width);
    let dy = normalized_axis(center.y, logical_size.height);
    let anchor_x = view.x + view.width * dx;
    let anchor_y = view.y + view.height * dy;

    view.x = anchor_x - width * dx;
    view.y = anchor_y - height * dy;
    view.width = width;
    view.height = height;

    clamp_view(view, buffer_size, ratio);
}

pub fn apply_zoom(
    view: &mut ViewRect,
    zoom_change: f64,
    center: Point,
    logical_size: Size,
    buffer_size: Size,
) {
    let ratio = aspect_ratio(logical_size, buffer_size);
    let width_delta = zoom_change * ratio;
    let height_delta = zoom_change;

    if view.width - width_delta < MIN_VIEW_HEIGHT * ratio
        || view.height - height_delta < MIN_VIEW_HEIGHT
    {
        return;
    }

    let dx = normalized_axis(center.x, logical_size.width);
    let dy = normalized_axis(center.y, logical_size.height);

    view.x += (width_delta * dx).round();
    view.width -= width_delta.round();
    view.y += (height_delta * dy).round();
    view.height -= height_delta.round();

    clamp_view(view, buffer_size, ratio);
}

pub fn clamp_view(view: &mut ViewRect, buffer_size: Size, ratio: f64) {
    let min_width = MIN_VIEW_HEIGHT * ratio;
    view.width = view.width.clamp(min_width, buffer_size.width);
    view.height = view.height.clamp(MIN_VIEW_HEIGHT, buffer_size.height);
    view.x = view.x.clamp(0.0, buffer_size.width - view.width);
    view.y = view.y.clamp(0.0, buffer_size.height - view.height);
}

pub fn interpolate_view(start: ViewRect, target: ViewRect, progress: f64) -> ViewRect {
    let progress = progress.clamp(0.0, 1.0);
    ViewRect {
        x: lerp(start.x, target.x, progress),
        y: lerp(start.y, target.y, progress),
        width: lerp(start.width, target.width, progress),
        height: lerp(start.height, target.height, progress),
    }
}

pub fn ease_out_cubic(progress: f64) -> f64 {
    let progress = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - progress).powi(3)
}

pub fn view_rect_nearly_equal(a: ViewRect, b: ViewRect) -> bool {
    const EPSILON: f64 = 0.01;
    (a.x - b.x).abs() <= EPSILON
        && (a.y - b.y).abs() <= EPSILON
        && (a.width - b.width).abs() <= EPSILON
        && (a.height - b.height).abs() <= EPSILON
}

pub fn screen_center(size: Size) -> Point {
    Point {
        x: size.width / 2.0,
        y: size.height / 2.0,
    }
}

pub fn aspect_ratio(logical_size: Size, buffer_size: Size) -> f64 {
    if logical_size.width > 0.0 && logical_size.height > 0.0 {
        logical_size.width / logical_size.height
    } else if buffer_size.width > 0.0 && buffer_size.height > 0.0 {
        buffer_size.width / buffer_size.height
    } else {
        1.0
    }
}

fn scaled_view_size(view: ViewRect, factor: f64, ratio: f64, buffer_size: Size) -> (f64, f64) {
    let min_height = MIN_VIEW_HEIGHT;
    let min_width = min_height * ratio;
    let mut height = (view.height * factor).clamp(min_height, buffer_size.height);
    let mut width = height * ratio;

    if width > buffer_size.width {
        width = buffer_size.width;
        height = (width / ratio).clamp(min_height, buffer_size.height);
    }
    if width < min_width {
        width = min_width;
        height = min_height;
    }

    (width, height)
}

fn normalized_axis(value: f64, size: f64) -> f64 {
    if size > 0.0 {
        (value / size).clamp(0.0, 1.0)
    } else {
        0.5
    }
}

fn lerp(start: f64, target: f64, progress: f64) -> f64 {
    start + (target - start) * progress
}

#[cfg(test)]
mod tests {
    use super::{
        MIN_VIEW_HEIGHT, Point, Size, ViewRect, apply_zoom, ease_out_cubic, interpolate_view,
        restore_view, screen_center, zoom_towards_factor,
    };

    fn buffer() -> Size {
        Size {
            width: 1920.0,
            height: 1080.0,
        }
    }

    #[test]
    fn zoom_in_changes_view_size_and_keeps_center() {
        let buffer_size = buffer();
        let logical_size = buffer_size;
        let center = screen_center(logical_size);
        let mut view = ViewRect::full(buffer_size);

        apply_zoom(&mut view, 100.0, center, logical_size, buffer_size);

        assert!(view.width < buffer_size.width);
        assert!(view.height < buffer_size.height);
        assert!((view.x + view.width / 2.0 - center.x).abs() <= 1.0);
        assert!((view.y + view.height / 2.0 - center.y).abs() <= 1.0);
    }

    #[test]
    fn zoom_out_restores_towards_initial_without_crossing_bounds() {
        let buffer_size = buffer();
        let logical_size = buffer_size;
        let center = screen_center(logical_size);
        let mut view = ViewRect::full(buffer_size);

        apply_zoom(&mut view, 200.0, center, logical_size, buffer_size);
        apply_zoom(&mut view, -400.0, center, logical_size, buffer_size);

        assert_eq!(view.width, buffer_size.width);
        assert_eq!(view.height, buffer_size.height);
        assert_eq!(view.x, 0.0);
        assert_eq!(view.y, 0.0);
    }

    #[test]
    fn restore_resets_exactly_to_initial_rectangle() {
        let initial = ViewRect::full(buffer());
        let mut current = initial;

        current.x = 50.0;
        current.y = 40.0;
        current.width = 1200.0;
        current.height = 675.0;

        restore_view(&mut current, initial);

        assert_eq!(current, initial);
    }

    #[test]
    fn pointer_centered_and_screen_centered_zoom_use_different_origins() {
        let buffer_size = buffer();
        let logical_size = buffer_size;
        let mut pointer_centered = ViewRect::full(buffer_size);
        let mut screen_centered = ViewRect::full(buffer_size);

        apply_zoom(
            &mut pointer_centered,
            100.0,
            Point { x: 10.0, y: 10.0 },
            logical_size,
            buffer_size,
        );
        apply_zoom(
            &mut screen_centered,
            100.0,
            screen_center(logical_size),
            logical_size,
            buffer_size,
        );

        assert!(pointer_centered.x < screen_centered.x);
        assert!(pointer_centered.y < screen_centered.y);
    }

    #[test]
    fn factor_zoom_keeps_pointer_source_anchor() {
        let buffer_size = buffer();
        let logical_size = buffer_size;
        let center = Point { x: 480.0, y: 270.0 };
        let mut view = ViewRect::full(buffer_size);
        let before_x = view.x + view.width * (center.x / logical_size.width);
        let before_y = view.y + view.height * (center.y / logical_size.height);

        zoom_towards_factor(&mut view, 0.75, center, logical_size, buffer_size);

        let after_x = view.x + view.width * (center.x / logical_size.width);
        let after_y = view.y + view.height * (center.y / logical_size.height);
        assert!((before_x - after_x).abs() <= 0.01);
        assert!((before_y - after_y).abs() <= 0.01);
    }

    #[test]
    fn factor_zoom_out_clamps_to_full_buffer() {
        let buffer_size = buffer();
        let logical_size = buffer_size;
        let center = screen_center(logical_size);
        let mut view = ViewRect {
            x: 240.0,
            y: 135.0,
            width: 1440.0,
            height: 810.0,
        };

        zoom_towards_factor(&mut view, 10.0, center, logical_size, buffer_size);

        assert_eq!(view, ViewRect::full(buffer_size));
    }

    #[test]
    fn factor_zoom_respects_minimum_view_height() {
        let buffer_size = buffer();
        let logical_size = buffer_size;
        let center = screen_center(logical_size);
        let mut view = ViewRect::full(buffer_size);

        zoom_towards_factor(&mut view, 0.0001, center, logical_size, buffer_size);

        assert_eq!(view.height, MIN_VIEW_HEIGHT);
        assert_eq!(
            view.width,
            MIN_VIEW_HEIGHT * (logical_size.width / logical_size.height)
        );
    }

    #[test]
    fn factor_zoom_keeps_logical_aspect_ratio() {
        let buffer_size = Size {
            width: 2400.0,
            height: 1200.0,
        };
        let logical_size = Size {
            width: 1600.0,
            height: 800.0,
        };
        let center = screen_center(logical_size);
        let mut view = ViewRect::full(buffer_size);

        zoom_towards_factor(&mut view, 0.5, center, logical_size, buffer_size);

        assert!((view.width / view.height - 2.0).abs() <= 0.01);
    }

    #[test]
    fn interpolation_starts_moves_and_finishes() {
        let start = ViewRect::full(buffer());
        let target = ViewRect {
            x: 240.0,
            y: 135.0,
            width: 1440.0,
            height: 810.0,
        };

        assert_eq!(interpolate_view(start, target, 0.0), start);
        let middle = interpolate_view(start, target, ease_out_cubic(0.5));
        assert!(middle.x > start.x);
        assert!(middle.x < target.x);
        assert!(middle.width < start.width);
        assert!(middle.width > target.width);
        assert_eq!(interpolate_view(start, target, 1.0), target);
    }
}
