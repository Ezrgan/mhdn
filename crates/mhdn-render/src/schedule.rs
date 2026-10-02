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

#[cfg(test)]
mod tests {
    use super::*;

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
