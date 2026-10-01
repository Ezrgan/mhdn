//! Pick Azahar out of a window list. Separate Windows keeps the top screen, the wide 5:3 one.

#![forbid(unsafe_code)]

use crate::geom::Rect;

pub const TOP_ASPECT: f32 = 400.0 / 240.0;
const MIN_SIDE: f32 = 80.0;
const ASPECT_TIE: f32 = 0.02;

#[derive(Debug, Clone, PartialEq)]
pub struct HostWindow {
    pub id: u32,
    pub owner_name: String,
    pub owner_pid: i32,
    pub layer: i32,
    pub bounds: Rect,
    pub onscreen: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct TrackQuery<'a> {
    pub owner_name: &'a str,
    pub owner_pid: Option<i32>,
    /// Stick to this id while it is still a candidate, so two Azahar windows do not swap.
    pub preferred_id: Option<u32>,
}

pub fn select_window<'a>(
    windows: &'a [HostWindow],
    query: &TrackQuery<'_>,
) -> Option<&'a HostWindow> {
    if let Some(id) = query.preferred_id {
        if let Some(window) = windows
            .iter()
            .find(|window| window.id == id && is_candidate(window, query))
        {
            return Some(window);
        }
    }

    let mut best: Option<&HostWindow> = None;
    for window in windows {
        if !is_candidate(window, query) {
            continue;
        }
        best = Some(match best {
            None => window,
            Some(current) if prefers(window, current) => window,
            Some(current) => current,
        });
    }
    best
}

fn is_candidate(window: &HostWindow, query: &TrackQuery<'_>) -> bool {
    if !window.onscreen || window.layer != 0 {
        return false;
    }
    if window.bounds.width < MIN_SIDE || window.bounds.height < MIN_SIDE {
        return false;
    }
    let name = window.owner_name.eq_ignore_ascii_case(query.owner_name);
    let pid = query.owner_pid.is_some_and(|pid| pid == window.owner_pid);
    name || pid
}

fn prefers(next: &HostWindow, current: &HostWindow) -> bool {
    let next_score = aspect_error(next.bounds);
    let current_score = aspect_error(current.bounds);
    if (next_score - current_score).abs() <= ASPECT_TIE {
        next.bounds.area() > current.bounds.area()
    } else {
        next_score < current_score
    }
}

fn aspect_error(bounds: Rect) -> f32 {
    if bounds.height <= 0.0 {
        return f32::MAX;
    }
    (bounds.width / bounds.height - TOP_ASPECT).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u32, name: &str, w: f32, h: f32) -> HostWindow {
        HostWindow {
            id,
            owner_name: name.to_string(),
            owner_pid: 42,
            layer: 0,
            bounds: Rect::new(0.0, 0.0, w, h),
            onscreen: true,
        }
    }

    fn query(preferred: Option<u32>) -> TrackQuery<'static> {
        TrackQuery {
            owner_name: "Azahar",
            owner_pid: None,
            preferred_id: preferred,
        }
    }

    #[test]
    fn the_top_screen_wins_over_a_taller_window() {
        let windows = vec![
            window(1, "Azahar", 800.0, 700.0),
            window(2, "Azahar", 1000.0, 600.0),
            window(3, "Finder", 1000.0, 600.0),
        ];
        let picked = select_window(&windows, &query(None)).unwrap();
        assert_eq!(picked.id, 2);
    }

    #[test]
    fn a_preferred_id_sticks_until_that_window_is_gone() {
        let windows = vec![
            window(1, "Azahar", 800.0, 700.0),
            window(2, "Azahar", 1000.0, 600.0),
        ];
        assert_eq!(select_window(&windows, &query(Some(1))).unwrap().id, 1);
        let only_wide = vec![window(2, "Azahar", 1000.0, 600.0)];
        assert_eq!(select_window(&only_wide, &query(Some(1))).unwrap().id, 2);
    }

    #[test]
    fn offscreen_menus_and_other_owners_are_ignored() {
        let mut menu = window(1, "Azahar", 1000.0, 600.0);
        menu.layer = 25;
        let mut hidden = window(2, "Azahar", 1000.0, 600.0);
        hidden.onscreen = false;
        let tiny = window(3, "Azahar", 40.0, 40.0);
        let other = window(4, "Notes", 1000.0, 600.0);
        assert!(select_window(&[menu, hidden, tiny, other], &query(None)).is_none());
    }

    #[test]
    fn a_pid_match_accepts_a_renamed_owner() {
        let windows = vec![window(7, "azahar-emu", 900.0, 540.0)];
        let query = TrackQuery {
            owner_name: "Azahar",
            owner_pid: Some(42),
            preferred_id: None,
        };
        assert_eq!(select_window(&windows, &query).unwrap().id, 7);
    }
}
