//! Present policy and the idle gate. An empty overlay must not redraw.

#![forbid(unsafe_code)]

use wgpu::{CompositeAlphaMode, PowerPreference, PresentMode, TextureFormat};

pub const PRESENT_MODE: PresentMode = PresentMode::Fifo;
pub const POWER_PREFERENCE: PowerPreference = PowerPreference::LowPower;

/// Prefer a mode that keeps RGB premultiplied. `Auto` is the last resort.
pub fn select_alpha_mode(modes: &[CompositeAlphaMode]) -> CompositeAlphaMode {
    const ORDER: [CompositeAlphaMode; 3] = [
        CompositeAlphaMode::PreMultiplied,
        CompositeAlphaMode::PostMultiplied,
        CompositeAlphaMode::Auto,
    ];
    ORDER
        .into_iter()
        .find(|mode| modes.contains(mode))
        .unwrap_or(CompositeAlphaMode::Auto)
}

pub fn pick_format(formats: &[TextureFormat]) -> Option<TextureFormat> {
    const PREFERRED: [TextureFormat; 4] = [
        TextureFormat::Bgra8Unorm,
        TextureFormat::Rgba8Unorm,
        TextureFormat::Bgra8UnormSrgb,
        TextureFormat::Rgba8UnormSrgb,
    ];
    PREFERRED
        .into_iter()
        .find(|format| formats.contains(format))
        .or_else(|| formats.first().copied())
}

pub fn premul(rgb: [f32; 3], alpha: f32) -> [f32; 4] {
    [rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha]
}

/// `request` then `take` once per frame. Idle stays false, so the app skips present.
#[derive(Debug, Default)]
pub struct FrameClock {
    requested: bool,
}

impl FrameClock {
    pub fn request(&mut self) {
        self.requested = true;
    }

    pub fn take(&mut self) -> bool {
        let draw = self.requested;
        self.requested = false;
        draw
    }
}

/// How the next frame should be produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    /// Ask the window system for a redraw and draw when it arrives.
    Request,
    /// The window system never answered, so draw right now.
    Now,
}

/// Frames asked for with no redraw delivered before the window system is given up on.
const STALLED_AFTER: u32 = 20;

/// Notices a window that never receives its redraw event. On Windows a layered
/// click-through window can sit without `WM_PAINT` forever, so `request_redraw` is a
/// no-op and nothing is ever drawn. Once that is seen, frames are drawn inline for the
/// rest of the run. A disabled watch always asks for a redraw.
#[derive(Debug)]
pub struct PaintWatch {
    enabled: bool,
    pending: u32,
    stalled: bool,
}

impl PaintWatch {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            pending: 0,
            stalled: false,
        }
    }

    /// Call each time a frame is due.
    pub fn frame(&mut self) -> Paint {
        if !self.enabled {
            return Paint::Request;
        }
        if self.stalled || self.pending >= STALLED_AFTER {
            self.stalled = true;
            return Paint::Now;
        }
        self.pending += 1;
        Paint::Request
    }

    /// Call whenever a frame is drawn.
    pub fn drawn(&mut self) {
        self.pending = 0;
    }

    pub fn stalled(&self) -> bool {
        self.stalled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watch_that_is_answered_never_draws_inline() {
        let mut watch = PaintWatch::new(true);
        for _ in 0..200 {
            assert_eq!(watch.frame(), Paint::Request);
            watch.drawn();
        }
        assert!(!watch.stalled());
    }

    #[test]
    fn unanswered_requests_switch_to_inline_drawing_for_good() {
        let mut watch = PaintWatch::new(true);
        for _ in 0..STALLED_AFTER {
            assert_eq!(watch.frame(), Paint::Request);
        }
        assert_eq!(watch.frame(), Paint::Now);
        assert!(watch.stalled());
        // Drawing inline resets the count but does not hand control back.
        watch.drawn();
        assert_eq!(watch.frame(), Paint::Now);
    }

    #[test]
    fn a_disabled_watch_always_requests() {
        let mut watch = PaintWatch::new(false);
        for _ in 0..200 {
            assert_eq!(watch.frame(), Paint::Request);
        }
        assert!(!watch.stalled());
    }

    #[test]
    fn alpha_mode_prefers_premultiplied() {
        let modes = [
            CompositeAlphaMode::Opaque,
            CompositeAlphaMode::PostMultiplied,
            CompositeAlphaMode::PreMultiplied,
        ];
        assert_eq!(select_alpha_mode(&modes), CompositeAlphaMode::PreMultiplied);
        assert_eq!(
            select_alpha_mode(&[CompositeAlphaMode::PostMultiplied]),
            CompositeAlphaMode::PostMultiplied
        );
        assert_eq!(select_alpha_mode(&[]), CompositeAlphaMode::Auto);
    }

    #[test]
    fn format_prefers_unorm_bgra() {
        assert_eq!(
            pick_format(&[TextureFormat::Rgba8UnormSrgb, TextureFormat::Bgra8Unorm]),
            Some(TextureFormat::Bgra8Unorm)
        );
        assert_eq!(pick_format(&[]), None);
    }

    #[test]
    fn premul_scales_rgb_and_keeps_alpha() {
        assert_eq!(premul([1.0, 0.5, 0.0], 0.5), [0.5, 0.25, 0.0, 0.5]);
    }

    #[test]
    fn an_idle_clock_does_not_draw() {
        let mut clock = FrameClock::default();
        assert!(!clock.take());
        clock.request();
        assert!(clock.take());
        assert!(!clock.take());
    }
}
