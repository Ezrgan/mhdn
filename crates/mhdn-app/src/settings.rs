//! What the settings window edits: number categories, colors, and the corner recount.
//!
//! Plain data and decisions. No windowing and no UI toolkit, so it is testable on its own.
//! Every default reproduces the look the overlay had before this window existed.
//!
//! The damage tap distinguishes three magnitude bands of ordinary hits, poison and other
//! status ticks, and the fixed damage from toppling a mounted monster. There is no
//! critical flag in the tap, so there is no critical category here.

#![forbid(unsafe_code)]

use mhdn_fx::{ORANGE, POISON, TOPPLE, WHITE, YELLOW};
use serde::{Deserialize, Serialize};

/// Glyph height of a floating number, in points. The value the overlay always used.
pub const DEFAULT_NUMBER_PX: f32 = 36.0;
pub const NUMBER_PX_RANGE: (f32, f32) = (16.0, 96.0);
/// Glyph height of the corner recount, in points. The value the overlay always used.
pub const DEFAULT_CORNER_PT: f32 = 22.0;
pub const CORNER_PT_RANGE: (f32, f32) = (12.0, 48.0);

/// The kinds of number the overlay really tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    /// A hit below the median of the last hunt hits. Also every hit while the window warms up.
    Small,
    /// A hit between the median and the 85th percentile.
    Medium,
    /// A hit above the 85th percentile: the bigger, orange spike.
    Large,
    /// A poison tick, and other status damage from the same caller.
    Poison,
    /// Fixed damage from toppling a mounted monster.
    Topple,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::Small,
        Category::Medium,
        Category::Large,
        Category::Poison,
        Category::Topple,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Small => "Small hits",
            Category::Medium => "Medium hits",
            Category::Large => "Large hits (spikes)",
            Category::Poison => "Poison",
            Category::Topple => "Mount topple",
        }
    }

    /// The color the overlay has always used for this category.
    pub fn default_rgb(self) -> [f32; 3] {
        match self {
            Category::Small => WHITE,
            Category::Medium => YELLOW,
            Category::Large => ORANGE,
            Category::Poison => POISON,
            Category::Topple => TOPPLE,
        }
    }

    /// Which category a spawned number belongs to, from the color the pool stored for it.
    pub fn from_spawn_rgb(rgb: [f32; 3]) -> Category {
        if rgb == TOPPLE {
            Category::Topple
        } else if rgb == POISON {
            Category::Poison
        } else if rgb == ORANGE {
            Category::Large
        } else if rgb == YELLOW {
            Category::Medium
        } else {
            Category::Small
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CategoryStyle {
    #[serde(default = "on")]
    pub show: bool,
    pub rgb: [f32; 3],
}

impl CategoryStyle {
    pub fn standard(category: Category) -> Self {
        Self {
            show: true,
            rgb: category.default_rgb(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NumberSettings {
    #[serde(default = "small")]
    pub small: CategoryStyle,
    #[serde(default = "medium")]
    pub medium: CategoryStyle,
    #[serde(default = "large")]
    pub large: CategoryStyle,
    #[serde(default = "poison")]
    pub poison: CategoryStyle,
    #[serde(default = "topple")]
    pub topple: CategoryStyle,
}

impl Default for NumberSettings {
    fn default() -> Self {
        Self {
            small: small(),
            medium: medium(),
            large: large(),
            poison: poison(),
            topple: topple(),
        }
    }
}

impl NumberSettings {
    pub fn get(&self, category: Category) -> &CategoryStyle {
        match category {
            Category::Small => &self.small,
            Category::Medium => &self.medium,
            Category::Large => &self.large,
            Category::Poison => &self.poison,
            Category::Topple => &self.topple,
        }
    }

    pub fn get_mut(&mut self, category: Category) -> &mut CategoryStyle {
        match category {
            Category::Small => &mut self.small,
            Category::Medium => &mut self.medium,
            Category::Large => &mut self.large,
            Category::Poison => &mut self.poison,
            Category::Topple => &mut self.topple,
        }
    }

    /// The color to draw a spawned number with, or `None` when its category is hidden.
    /// `spawn_rgb` is the color the number was created with, which names its category.
    pub fn color_for(&self, spawn_rgb: [f32; 3]) -> Option<[f32; 3]> {
        let style = self.get(Category::from_spawn_rgb(spawn_rgb));
        style.show.then_some(style.rgb)
    }
}

/// The recount in the corner of the game screen.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CornerSettings {
    #[serde(default = "on")]
    pub show_total: bool,
    #[serde(default = "on")]
    pub show_dps: bool,
    #[serde(default = "default_corner_pt")]
    pub size_pt: f32,
}

impl Default for CornerSettings {
    fn default() -> Self {
        Self {
            show_total: true,
            show_dps: true,
            size_pt: DEFAULT_CORNER_PT,
        }
    }
}

impl CornerSettings {
    /// The corner text, or `None` when both parts are off. The default is `DMG 40  DPS 12`.
    pub fn line(&self, total: u32, dps: f32) -> Option<String> {
        match (self.show_total, self.show_dps) {
            (true, true) => Some(format!("DMG {total}  DPS {dps:.0}")),
            (true, false) => Some(format!("DMG {total}")),
            (false, true) => Some(format!("DPS {dps:.0}")),
            (false, false) => None,
        }
    }
}

/// Keeps a hand-edited or corrupt value inside what the renderer can draw.
pub fn clamp_px(value: f32, range: (f32, f32), fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(range.0, range.1)
    } else {
        fallback
    }
}

fn on() -> bool {
    true
}

fn default_corner_pt() -> f32 {
    DEFAULT_CORNER_PT
}

fn small() -> CategoryStyle {
    CategoryStyle::standard(Category::Small)
}

fn medium() -> CategoryStyle {
    CategoryStyle::standard(Category::Medium)
}

fn large() -> CategoryStyle {
    CategoryStyle::standard(Category::Large)
}

fn poison() -> CategoryStyle {
    CategoryStyle::standard(Category::Poison)
}

fn topple() -> CategoryStyle {
    CategoryStyle::standard(Category::Topple)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhdn_fx::{HitKind, MagnitudeWindow};

    #[test]
    fn defaults_show_everything_in_the_colors_the_overlay_always_used() {
        let numbers = NumberSettings::default();
        for category in Category::ALL {
            let style = numbers.get(category);
            assert!(style.show, "{category:?} is shown by default");
            assert_eq!(style.rgb, category.default_rgb());
            assert_eq!(numbers.color_for(category.default_rgb()), Some(style.rgb));
        }
        assert_eq!(numbers.small.rgb, WHITE);
        assert_eq!(numbers.medium.rgb, YELLOW);
        assert_eq!(numbers.large.rgb, ORANGE);
        assert_eq!(numbers.poison.rgb, POISON);
        assert_eq!(numbers.topple.rgb, TOPPLE);
        assert!(numbers.topple.show);
        assert_eq!(Category::Topple.label(), "Mount topple");
        assert_eq!(DEFAULT_NUMBER_PX, 36.0);
    }

    #[test]
    fn the_corner_default_prints_exactly_what_it_printed_before() {
        let corner = CornerSettings::default();
        assert_eq!(corner.line(40, 12.4).as_deref(), Some("DMG 40  DPS 12"));
        assert_eq!(corner.size_pt, 22.0);
    }

    #[test]
    fn the_corner_parts_toggle_independently() {
        let corner = |show_total, show_dps| CornerSettings {
            show_total,
            show_dps,
            ..CornerSettings::default()
        };
        assert_eq!(
            corner(true, false).line(40, 12.0).as_deref(),
            Some("DMG 40")
        );
        assert_eq!(
            corner(false, true).line(40, 12.0).as_deref(),
            Some("DPS 12")
        );
        assert_eq!(corner(false, false).line(40, 12.0), None);
    }

    #[test]
    fn every_style_the_pool_produces_maps_to_its_own_category() {
        let mut window = MagnitudeWindow::new();
        let poison = window.style(3, HitKind::Poison);
        assert_eq!(Category::from_spawn_rgb(poison.rgb), Category::Poison);
        let topple = window.style(150, HitKind::Topple);
        assert_eq!(Category::from_spawn_rgb(topple.rgb), Category::Topple);
        assert!((topple.scale - 1.25).abs() < 0.001);
        for amount in 1..=20 {
            window.style(amount, HitKind::Hit);
        }
        let large = window.style(500, HitKind::Hit);
        assert_eq!(Category::from_spawn_rgb(large.rgb), Category::Large);
        let small = window.style(1, HitKind::Hit);
        assert_eq!(Category::from_spawn_rgb(small.rgb), Category::Small);
        let medium = window.style(window_median(), HitKind::Hit);
        assert_eq!(Category::from_spawn_rgb(medium.rgb), Category::Medium);
    }

    /// Near the middle of 1..=20 plus the hits above, inside p50..p85.
    fn window_median() -> u32 {
        14
    }

    #[test]
    fn a_hidden_category_has_no_color_so_numbers_draw_no_quad() {
        let mut numbers = NumberSettings::default();
        numbers.get_mut(Category::Poison).show = false;
        assert_eq!(numbers.color_for(POISON), None);
        assert!(numbers.color_for(ORANGE).is_some());
        numbers.get_mut(Category::Small).show = false;
        assert_eq!(numbers.color_for(WHITE), None);
    }

    #[test]
    fn a_recolored_category_changes_only_that_category() {
        let mut numbers = NumberSettings::default();
        numbers.get_mut(Category::Large).rgb = [0.0, 1.0, 0.0];
        assert_eq!(numbers.color_for(ORANGE), Some([0.0, 1.0, 0.0]));
        assert_eq!(numbers.color_for(YELLOW), Some(YELLOW));
    }

    #[test]
    fn an_old_config_without_topple_loads_the_default_mount_topple_style() {
        let old = "\
[small]
show = true
rgb = [1.0, 1.0, 1.0]

[medium]
show = true
rgb = [1.0, 0.95, 0.62]

[large]
show = true
rgb = [1.0, 0.55, 0.12]

[poison]
show = false
rgb = [0.72, 0.42, 0.95]
";
        let loaded: NumberSettings = toml::from_str(old).expect("old numbers section parses");
        assert!(!loaded.poison.show);
        assert_eq!(loaded.topple, NumberSettings::default().topple);
        assert_eq!(loaded.topple.rgb, TOPPLE);
        assert!(loaded.topple.show);
    }

    #[test]
    fn clamping_rejects_nan_and_out_of_range_sizes() {
        assert_eq!(clamp_px(f32::NAN, NUMBER_PX_RANGE, 36.0), 36.0);
        assert_eq!(clamp_px(1000.0, NUMBER_PX_RANGE, 36.0), 96.0);
        assert_eq!(clamp_px(1.0, CORNER_PT_RANGE, 22.0), 12.0);
        assert_eq!(clamp_px(30.0, NUMBER_PX_RANGE, 36.0), 30.0);
    }
}
