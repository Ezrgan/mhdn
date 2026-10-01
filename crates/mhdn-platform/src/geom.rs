//! Rectangles in the same space winit uses on macOS: top-left of the primary display, Y down.

#![forbid(unsafe_code)]

/// Default title-bar inset when the window is not in native fullscreen.
pub const DEFAULT_TITLE_BAR_PT: f32 = 28.0;
/// Azahar status bar, applied only when `showStatusBar` is on.
pub const DEFAULT_STATUS_BAR_PT: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    pub fn area(self) -> f32 {
        self.width.max(0.0) * self.height.max(0.0)
    }
}

/// Points trimmed from a window's outer bounds to reach Azahar's render area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Insets {
    pub title_bar: f32,
    pub status_bar: f32,
    pub left: f32,
    pub right: f32,
}

impl Insets {
    pub fn chrome(show_status_bar: bool) -> Self {
        Self {
            title_bar: DEFAULT_TITLE_BAR_PT,
            status_bar: if show_status_bar {
                DEFAULT_STATUS_BAR_PT
            } else {
                0.0
            },
            left: 0.0,
            right: 0.0,
        }
    }
}

impl Default for Insets {
    fn default() -> Self {
        Self::chrome(false)
    }
}

/// Drop the title bar unless the outer bounds already match a display.
/// The status bar stays when the Azahar config says it is visible.
pub fn content_rect(bounds: Rect, insets: Insets, fullscreen: bool) -> Rect {
    let top = if fullscreen {
        0.0
    } else {
        insets.title_bar.max(0.0)
    };
    let bottom = insets.status_bar.max(0.0);
    let left = insets.left.max(0.0);
    let right = insets.right.max(0.0);
    Rect {
        x: bounds.x + left,
        y: bounds.y + top,
        width: (bounds.width - left - right).max(1.0),
        height: (bounds.height - top - bottom).max(1.0),
    }
}

pub fn is_fullscreen(bounds: Rect, screens: &[Rect]) -> bool {
    screens.iter().any(|screen| nearly_equal(bounds, *screen))
}

fn nearly_equal(left: Rect, right: Rect) -> bool {
    (left.x - right.x).abs() <= 1.0
        && (left.y - right.y).abs() <= 1.0
        && (left.width - right.width).abs() <= 1.0
        && (left.height - right.height).abs() <= 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_rect_drops_the_title_bar_and_the_status_bar() {
        let bounds = Rect::new(10.0, 20.0, 800.0, 600.0);
        let got = content_rect(bounds, Insets::chrome(true), false);
        assert_eq!(got, Rect::new(10.0, 48.0, 800.0, 548.0));
    }

    #[test]
    fn fullscreen_keeps_the_status_bar_and_drops_the_title() {
        let bounds = Rect::new(0.0, 0.0, 1440.0, 900.0);
        let got = content_rect(bounds, Insets::chrome(true), true);
        assert_eq!(got.y, 0.0);
        assert_eq!(got.height, 900.0 - DEFAULT_STATUS_BAR_PT);
    }

    #[test]
    fn a_window_that_matches_a_display_is_fullscreen() {
        let screen = Rect::new(0.0, 0.0, 1512.0, 982.0);
        assert!(is_fullscreen(screen, &[screen]));
        assert!(!is_fullscreen(
            Rect::new(40.0, 40.0, 800.0, 600.0),
            &[screen]
        ));
    }
}
