//! Move the overlay every poll, but resize its surface at most 30 times a second.

#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use winit::dpi::{LogicalPosition, LogicalSize};
use winit::window::Window;

use crate::geom::Rect;

/// One guest-scale poll. Live resize must not rebuild the GPU surface faster than this.
pub const MIN_SURFACE_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceUpdate {
    Idle,
    Move { x: i32, y: i32 },
    MoveResize(PixelRect),
}

pub fn round_rect(rect: Rect) -> PixelRect {
    PixelRect {
        x: round_i32(rect.x),
        y: round_i32(rect.y),
        width: round_u32(rect.width).max(1),
        height: round_u32(rect.height).max(1),
    }
}

fn round_i32(value: f32) -> i32 {
    let rounded = value.round();
    if !rounded.is_finite() {
        return 0;
    }
    let clamped = rounded.clamp(i32::MIN as f32, i32::MAX as f32);
    clamped as i32
}

fn round_u32(value: f32) -> u32 {
    let rounded = value.round();
    if !rounded.is_finite() || rounded <= 0.0 {
        return 0;
    }
    let clamped = rounded.min(u32::MAX as f32);
    clamped as u32
}

#[derive(Debug)]
pub struct FollowController {
    applied: Option<PixelRect>,
    pending: Option<PixelRect>,
    last_resize: Option<Instant>,
    interval: Duration,
}

impl FollowController {
    pub fn new(interval: Duration) -> Self {
        Self {
            applied: None,
            pending: None,
            last_resize: None,
            interval,
        }
    }

    pub fn observe(&mut self, now: Instant, next: PixelRect) -> SurfaceUpdate {
        let Some(applied) = self.applied else {
            self.commit_resize(now, next);
            return SurfaceUpdate::MoveResize(next);
        };

        if self.pending.is_some() && self.resize_due(now) {
            self.commit_resize(now, next);
            return SurfaceUpdate::MoveResize(next);
        }

        let moved = applied.x != next.x || applied.y != next.y;
        let resized = applied.width != next.width || applied.height != next.height;
        if resized {
            if self.resize_due(now) {
                self.commit_resize(now, next);
                return SurfaceUpdate::MoveResize(next);
            }
            self.pending = Some(next);
            if moved {
                let kept = PixelRect {
                    x: next.x,
                    y: next.y,
                    width: applied.width,
                    height: applied.height,
                };
                self.applied = Some(kept);
                return SurfaceUpdate::Move {
                    x: next.x,
                    y: next.y,
                };
            }
            return SurfaceUpdate::Idle;
        }

        self.pending = None;
        if moved {
            self.applied = Some(next);
            return SurfaceUpdate::Move {
                x: next.x,
                y: next.y,
            };
        }
        SurfaceUpdate::Idle
    }

    fn commit_resize(&mut self, now: Instant, next: PixelRect) {
        self.applied = Some(next);
        self.pending = None;
        self.last_resize = Some(now);
    }

    fn resize_due(&self, now: Instant) -> bool {
        self.last_resize
            .is_none_or(|then| now.saturating_duration_since(then) >= self.interval)
    }
}

/// Applies a [`FollowController`] decision to a winit window. Position is the
/// top-left of the content rect in winit's logical coordinates.
pub struct OverlayHost {
    follow: FollowController,
}

impl Default for OverlayHost {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayHost {
    pub fn new() -> Self {
        Self {
            follow: FollowController::new(MIN_SURFACE_INTERVAL),
        }
    }

    pub fn sync(&mut self, window: &Window, content: Rect, now: Instant) -> SurfaceUpdate {
        let update = self.follow.observe(now, round_rect(content));
        match update {
            SurfaceUpdate::Idle => {}
            SurfaceUpdate::Move { x, y } => {
                window.set_outer_position(LogicalPosition::new(f64::from(x), f64::from(y)));
            }
            SurfaceUpdate::MoveResize(rect) => {
                window
                    .set_outer_position(LogicalPosition::new(f64::from(rect.x), f64::from(rect.y)));
                let _ = window.request_inner_size(LogicalSize::new(
                    f64::from(rect.width),
                    f64::from(rect.height),
                ));
            }
        }
        update
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(x: i32, y: i32, width: u32, height: u32) -> PixelRect {
        PixelRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn position_updates_immediately_and_size_waits_for_the_interval() {
        let start = Instant::now();
        let mut follow = FollowController::new(Duration::from_millis(33));
        assert_eq!(
            follow.observe(start, px(0, 0, 100, 80)),
            SurfaceUpdate::MoveResize(px(0, 0, 100, 80))
        );
        assert_eq!(
            follow.observe(start, px(4, 1, 100, 80)),
            SurfaceUpdate::Move { x: 4, y: 1 }
        );
        assert_eq!(
            follow.observe(start + Duration::from_millis(10), px(4, 1, 140, 90)),
            SurfaceUpdate::Idle
        );
        assert_eq!(
            follow.observe(start + Duration::from_millis(33), px(6, 1, 140, 90)),
            SurfaceUpdate::MoveResize(px(6, 1, 140, 90))
        );
    }

    #[test]
    fn a_move_during_a_deferred_resize_keeps_the_old_size() {
        let start = Instant::now();
        let mut follow = FollowController::new(Duration::from_millis(33));
        follow.observe(start, px(0, 0, 100, 80));
        assert_eq!(
            follow.observe(start + Duration::from_millis(5), px(8, 2, 160, 80)),
            SurfaceUpdate::Move { x: 8, y: 2 }
        );
    }

    #[test]
    fn subpixel_jitter_rounds_away_before_it_can_resize() {
        let rounded = round_rect(Rect::new(1.2, 3.6, 100.4, 80.2));
        assert_eq!(rounded, px(1, 4, 100, 80));
    }
}
