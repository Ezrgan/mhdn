//! Color and size from the last 50 hits of this hunt.
//!
//! Poison and other status stay out of the window: they are small purple ticks, not hits.
//! Mount-topple damage stays out too, and keeps the large size.

#![forbid(unsafe_code)]

use std::collections::VecDeque;

pub const WINDOW: usize = 50;
const MIN_SAMPLES: usize = 8;

pub const WHITE: [f32; 3] = [1.0, 1.0, 1.0];
pub const YELLOW: [f32; 3] = [1.0, 0.95, 0.62];
pub const ORANGE: [f32; 3] = [1.0, 0.55, 0.12];
pub const POISON: [f32; 3] = [0.72, 0.42, 0.95];
/// Mount-topple numbers. Distinct from the hit bands so settings can recolor them.
pub const TOPPLE: [f32; 3] = [0.22, 0.82, 0.94];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitKind {
    Hit,
    Poison,
    Topple,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitStyle {
    pub rgb: [f32; 3],
    pub scale: f32,
}

#[derive(Debug, Clone)]
pub struct MagnitudeWindow {
    values: VecDeque<u32>,
}

impl Default for MagnitudeWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl MagnitudeWindow {
    pub fn new() -> Self {
        Self {
            values: VecDeque::with_capacity(WINDOW),
        }
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn style(&mut self, amount: u32, kind: HitKind) -> HitStyle {
        match kind {
            HitKind::Poison => HitStyle {
                rgb: POISON,
                scale: 0.75,
            },
            HitKind::Topple => HitStyle {
                rgb: TOPPLE,
                scale: 1.25,
            },
            HitKind::Hit => self.hit_style(amount),
        }
    }

    fn hit_style(&mut self, amount: u32) -> HitStyle {
        if self.values.len() == WINDOW {
            self.values.pop_front();
        }
        self.values.push_back(amount);
        if self.values.len() < MIN_SAMPLES {
            return HitStyle {
                rgb: WHITE,
                scale: 1.0,
            };
        }
        let p50 = percentile(&self.values, 0.50);
        let p85 = percentile(&self.values, 0.85);
        let amount = amount as f32;
        if amount < p50 {
            HitStyle {
                rgb: WHITE,
                scale: 1.0,
            }
        } else if amount <= p85 {
            HitStyle {
                rgb: YELLOW,
                scale: 1.0,
            }
        } else {
            HitStyle {
                rgb: ORANGE,
                scale: 1.25,
            }
        }
    }
}

fn percentile(values: &VecDeque<u32>, p: f32) -> f32 {
    let mut tmp = [0u32; WINDOW];
    let n = values.len().min(WINDOW);
    for (index, value) in values.iter().take(n).enumerate() {
        tmp[index] = *value;
    }
    tmp[..n].sort_unstable();
    let index = ((n - 1) as f32 * p).round() as usize;
    tmp[index] as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_hits_stay_white_until_the_window_has_a_spread() {
        let mut window = MagnitudeWindow::new();
        for amount in [10, 20, 30, 40] {
            let style = window.style(amount, HitKind::Hit);
            assert_eq!(style.rgb, WHITE);
            assert_eq!(style.scale, 1.0);
        }
        assert_eq!(window.len(), 4);
    }

    #[test]
    fn a_hit_above_p85_is_orange_and_larger() {
        let mut window = MagnitudeWindow::new();
        for amount in 1..=20 {
            window.style(amount, HitKind::Hit);
        }
        let big = window.style(100, HitKind::Hit);
        assert_eq!(big.rgb, ORANGE);
        assert!((big.scale - 1.25).abs() < 0.001);
        let small = window.style(1, HitKind::Hit);
        assert_eq!(small.rgb, WHITE);
    }

    #[test]
    fn poison_is_small_and_purple_and_does_not_enter_the_window() {
        let mut window = MagnitudeWindow::new();
        let style = window.style(5, HitKind::Poison);
        assert_eq!(style.rgb, POISON);
        assert!((style.scale - 0.75).abs() < 0.001);
        assert_eq!(window.len(), 0);
    }

    #[test]
    fn topple_is_large_and_cyan_and_does_not_enter_the_window() {
        let mut window = MagnitudeWindow::new();
        let style = window.style(150, HitKind::Topple);
        assert_eq!(style.rgb, TOPPLE);
        assert!((style.scale - 1.25).abs() < 0.001);
        assert_eq!(window.len(), 0);
    }
}
