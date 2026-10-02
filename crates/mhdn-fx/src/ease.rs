//! Pop, rise, hold, and fade for one floating number.
//!
//! The number pops in 90 ms, keeps rising through a 450 ms hold, then fades
//! for 350 ms. Rise speed is chosen per hit, between 60 and 90 px/s.

#![forbid(unsafe_code)]

pub const POP_MS: f32 = 90.0;
pub const HOLD_MS: f32 = 450.0;
pub const FADE_MS: f32 = 350.0;
pub const LIFE_MS: f32 = POP_MS + HOLD_MS + FADE_MS;
pub const RISE_MIN: f32 = 60.0;
pub const RISE_MAX: f32 = 90.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub rise_px: f32,
    pub alpha: f32,
    pub scale: f32,
    pub alive: bool,
}

pub fn pose(age_ms: f32, speed_px_s: f32) -> Pose {
    if !(0.0..LIFE_MS).contains(&age_ms) {
        return Pose {
            rise_px: 0.0,
            alpha: 0.0,
            scale: 1.0,
            alive: false,
        };
    }
    let scale = if age_ms <= POP_MS {
        let t = ease_out_back(age_ms / POP_MS);
        1.35 + (1.0 - 1.35) * t
    } else {
        1.0
    };
    let rise_window = POP_MS + HOLD_MS;
    let u = (age_ms / rise_window).clamp(0.0, 1.0);
    let distance = speed_px_s * (rise_window / 1000.0);
    let rise_px = distance * ease_out_cubic(u);
    let alpha = if age_ms < rise_window {
        1.0
    } else {
        1.0 - ((age_ms - rise_window) / FADE_MS).clamp(0.0, 1.0)
    };
    Pose {
        rise_px,
        alpha,
        scale,
        alive: alpha > 0.0,
    }
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_out_back(t: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + C3 * t * t * t + C1 * t * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pop_starts_large_and_settles_at_one() {
        let start = pose(0.0, 75.0);
        assert!((start.scale - 1.35).abs() < 0.02);
        assert_eq!(start.alpha, 1.0);
        assert!(start.rise_px.abs() < 0.01);
        let popped = pose(POP_MS, 75.0);
        assert!((popped.scale - 1.0).abs() < 0.02);
        assert!(popped.alive);
    }

    #[test]
    fn the_hold_keeps_the_number_opaque_while_it_rises() {
        let mid = pose(POP_MS + HOLD_MS * 0.5, 80.0);
        assert_eq!(mid.alpha, 1.0);
        assert!((mid.scale - 1.0).abs() < 0.001);
        let end_hold = pose(POP_MS + HOLD_MS - 1.0, 60.0);
        let full = 60.0 * ((POP_MS + HOLD_MS) / 1000.0);
        assert!(end_hold.rise_px > full * 0.9);
        assert!(end_hold.rise_px <= full + 0.1);
    }

    #[test]
    fn the_fade_ends_the_number() {
        let fading = pose(POP_MS + HOLD_MS + FADE_MS * 0.5, 70.0);
        assert!((fading.alpha - 0.5).abs() < 0.02);
        assert!(!pose(LIFE_MS, 70.0).alive);
    }

    #[test]
    fn easing_curves_start_at_zero_and_end_at_one() {
        assert!(ease_out_cubic(0.0).abs() < 0.001);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < 0.001);
        assert!(ease_out_back(0.0).abs() < 0.001);
        assert!((ease_out_back(1.0) - 1.0).abs() < 0.001);
        assert!(ease_out_cubic(0.5) > 0.5);
    }
}
