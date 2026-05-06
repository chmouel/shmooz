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

    let dx = if logical_size.width > 0.0 {
        center.x / logical_size.width
    } else {
        0.5
    };
    let dy = if logical_size.height > 0.0 {
        center.y / logical_size.height
    } else {
        0.5
    };

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

pub fn screen_center(size: Size) -> Point {
    Point {
        x: size.width / 2.0,
        y: size.height / 2.0,
    }
}

fn aspect_ratio(logical_size: Size, buffer_size: Size) -> f64 {
    if logical_size.width > 0.0 && logical_size.height > 0.0 {
        logical_size.width / logical_size.height
    } else if buffer_size.width > 0.0 && buffer_size.height > 0.0 {
        buffer_size.width / buffer_size.height
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::{Point, Size, ViewRect, apply_zoom, restore_view, screen_center};

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
}
