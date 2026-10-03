//! Window tracker. The macOS poll and the tests share one pure function.

#![forbid(unsafe_code)]

use crate::geom::{content_rect, is_fullscreen, Insets, Rect};
use crate::select::{select_window, HostWindow, TrackQuery};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Display {
    pub frame: Rect,
    pub scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackedWindow {
    pub id: u32,
    pub bounds: Rect,
    pub content_rect: Rect,
    pub scale: f32,
    pub onscreen: bool,
    pub is_fullscreen: bool,
    pub owner_pid: i32,
}

pub trait WindowTracker {
    fn poll(&mut self) -> Option<TrackedWindow>;
    fn set_insets(&mut self, insets: Insets);
}

pub struct NullTracker;

impl WindowTracker for NullTracker {
    fn poll(&mut self) -> Option<TrackedWindow> {
        None
    }

    fn set_insets(&mut self, _insets: Insets) {}
}

/// In-memory tracker used by tests and by the macOS backend after it has copied the list.
pub struct ListTracker {
    pub windows: Vec<HostWindow>,
    pub displays: Vec<Display>,
    pub insets: Insets,
    pub owner_pid: Option<i32>,
    preferred_id: Option<u32>,
}

impl ListTracker {
    pub fn new(insets: Insets) -> Self {
        Self {
            windows: Vec::new(),
            displays: Vec::new(),
            insets,
            owner_pid: None,
            preferred_id: None,
        }
    }
}

impl WindowTracker for ListTracker {
    fn poll(&mut self) -> Option<TrackedWindow> {
        let tracked = track_windows(
            &self.windows,
            &self.displays,
            self.insets,
            self.owner_pid,
            self.preferred_id,
        )?;
        self.preferred_id = Some(tracked.id);
        Some(tracked)
    }

    fn set_insets(&mut self, insets: Insets) {
        self.insets = insets;
    }
}

pub fn track_windows(
    windows: &[HostWindow],
    displays: &[Display],
    insets: Insets,
    owner_pid: Option<i32>,
    preferred_id: Option<u32>,
) -> Option<TrackedWindow> {
    let query = TrackQuery {
        owner_name: "Azahar",
        owner_pid,
        preferred_id,
    };
    let window = select_window(windows, &query)?;
    let screens: Vec<Rect> = displays.iter().map(|display| display.frame).collect();
    let fullscreen = is_fullscreen(window.bounds, &screens);
    Some(TrackedWindow {
        id: window.id,
        bounds: window.bounds,
        content_rect: content_rect(window.bounds, insets, fullscreen),
        scale: scale_for(window.bounds, displays),
        onscreen: window.onscreen,
        is_fullscreen: fullscreen,
        owner_pid: window.owner_pid,
    })
}

/// The overlay draws at screen-saver level, above every app. It may only show while the
/// emulator, or the overlay itself during calibration, owns the keyboard.
pub fn host_in_front(host_pid: i32, frontmost: Option<i32>, own_pid: i32) -> bool {
    match frontmost {
        Some(pid) => pid == host_pid || pid == own_pid,
        None => true,
    }
}

fn scale_for(bounds: Rect, displays: &[Display]) -> f32 {
    let cx = bounds.x + bounds.width * 0.5;
    let cy = bounds.y + bounds.height * 0.5;
    displays
        .iter()
        .find(|display| display.frame.contains(cx, cy))
        .map(|display| display.scale)
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(1.0)
}

#[cfg(target_os = "macos")]
mod system {
    use super::*;
    use crate::macos_list::{list_displays, list_windows};

    pub struct MacTracker {
        insets: Insets,
        preferred_id: Option<u32>,
    }

    impl MacTracker {
        pub fn new(insets: Insets) -> Self {
            Self {
                insets,
                preferred_id: None,
            }
        }
    }

    impl WindowTracker for MacTracker {
        fn poll(&mut self) -> Option<TrackedWindow> {
            let displays = list_displays();
            let windows = list_windows();
            let tracked = track_windows(&windows, &displays, self.insets, None, self.preferred_id)?;
            self.preferred_id = Some(tracked.id);
            Some(tracked)
        }

        fn set_insets(&mut self, insets: Insets) {
            self.insets = insets;
        }
    }

    pub fn system_tracker(insets: Insets) -> Box<dyn WindowTracker> {
        Box::new(MacTracker::new(insets))
    }
}

#[cfg(windows)]
mod system {
    use super::*;

    pub fn system_tracker(insets: Insets) -> Box<dyn WindowTracker> {
        Box::new(crate::windows::WinTracker::new(insets))
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod system {
    use super::*;

    pub fn system_tracker(insets: Insets) -> Box<dyn WindowTracker> {
        let _ = insets;
        Box::new(NullTracker)
    }
}

pub use system::system_tracker;

#[cfg(test)]
mod tests {
    use super::*;

    fn azahar(id: u32, bounds: Rect) -> HostWindow {
        HostWindow {
            id,
            owner_name: "Azahar".to_string(),
            owner_pid: 9,
            layer: 0,
            bounds,
            onscreen: true,
        }
    }

    #[test]
    fn the_overlay_shows_only_while_the_emulator_or_itself_is_in_front() {
        assert!(host_in_front(10, Some(10), 99));
        assert!(host_in_front(10, Some(99), 99));
        assert!(!host_in_front(10, Some(42), 99));
        assert!(host_in_front(10, None, 99));
    }

    #[test]
    fn the_content_rect_excludes_chrome_and_reports_the_display_scale() {
        let bounds = Rect::new(40.0, 30.0, 800.0, 600.0);
        let displays = vec![Display {
            frame: Rect::new(0.0, 0.0, 1440.0, 900.0),
            scale: 2.0,
        }];
        let tracked = track_windows(
            &[azahar(3, bounds)],
            &displays,
            Insets::chrome(true),
            None,
            None,
        )
        .unwrap();
        assert!(!tracked.is_fullscreen);
        assert_eq!(tracked.scale, 2.0);
        assert_eq!(tracked.content_rect.y, 30.0 + 28.0);
        assert_eq!(tracked.content_rect.height, 600.0 - 28.0 - 24.0);
    }

    #[test]
    fn a_bounds_match_is_fullscreen_and_drops_the_title_inset() {
        let frame = Rect::new(0.0, 0.0, 1512.0, 982.0);
        let tracked = track_windows(
            &[azahar(1, frame)],
            &[Display { frame, scale: 2.0 }],
            Insets::chrome(false),
            None,
            None,
        )
        .unwrap();
        assert!(tracked.is_fullscreen);
        assert_eq!(tracked.content_rect, frame);
    }
}
