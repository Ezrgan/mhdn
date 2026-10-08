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
use mhdn_game::{Attacker, AttackerFilter};
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

/// Who adds to the floating numbers and the corner meter.
///
/// Missing keys in an old `config.toml` load these defaults: only you and your Felyne.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageCount {
    #[serde(default = "on")]
    pub you: bool,
    #[serde(default = "on")]
    pub your_felyne: bool,
    #[serde(default = "off")]
    pub other_hunters: bool,
    #[serde(default = "off")]
    pub their_felynes: bool,
    #[serde(default = "off")]
    pub other: bool,
    #[serde(default = "off")]
    pub unknown: bool,
    /// Draw numbers for attackers that are off, in gray. They still do not add to the meter.
    #[serde(default = "off")]
    pub gray_uncounted: bool,
}

impl Default for DamageCount {
    fn default() -> Self {
        Self {
            you: true,
            your_felyne: true,
            other_hunters: false,
            their_felynes: false,
            other: false,
            unknown: false,
            gray_uncounted: false,
        }
    }
}

impl DamageCount {
    pub fn filter(self) -> AttackerFilter {
        let mut filter = AttackerFilter::from_slice(&[]);
        let pairs = [
            (Attacker::You, self.you),
            (Attacker::YourFelyne, self.your_felyne),
            (Attacker::OtherHunter, self.other_hunters),
            (Attacker::OtherFelyne, self.their_felynes),
            (Attacker::Other, self.other),
            (Attacker::Unknown, self.unknown),
        ];
        for (attacker, allowed) in pairs {
            filter = filter.with(attacker, allowed);
        }
        filter
    }

    /// Every attacker counts. Tests of the pre-filter corner use this.
    #[cfg(test)]
    pub fn allowing_all() -> Self {
        Self {
            you: true,
            your_felyne: true,
            other_hunters: true,
            their_felynes: true,
            other: true,
            unknown: true,
            gray_uncounted: false,
        }
    }
}

/// Which block the corner draws. One choice, saved in `config.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerMeterMode {
    /// Large monsters, quest share, poison and topple: the corner the overlay already draws.
    #[default]
    Session,
    /// The monster you last hit with the filter on.
    CurrentMonster,
    /// One line per monster hit, plus the quest total.
    WholeQuest,
}

/// Residual HP drops (passive HP delta), not each tap hit.
///
/// Shown by default so an old config keeps drawing them. The color replaces the
/// magnitude color for those numbers only.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GroupedHpSettings {
    #[serde(default = "on")]
    pub show: bool,
    #[serde(default = "default_grouped_rgb")]
    pub rgb: [f32; 3],
}

pub const DEFAULT_GROUPED_RGB: [f32; 3] = [0.75, 0.82, 0.95];

impl Default for GroupedHpSettings {
    fn default() -> Self {
        Self {
            show: true,
            rgb: DEFAULT_GROUPED_RGB,
        }
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
    #[serde(default)]
    pub mode: CornerMeterMode,
}

impl Default for CornerSettings {
    fn default() -> Self {
        Self {
            show_total: true,
            show_dps: true,
            size_pt: DEFAULT_CORNER_PT,
            mode: CornerMeterMode::Session,
        }
    }
}

impl CornerSettings {
    /// The old one-line corner, kept so a saved config still has the two switches.
    /// The overlay now draws one block per monster instead of this string.
    #[cfg_attr(not(test), allow(dead_code))]
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

fn off() -> bool {
    false
}

fn default_grouped_rgb() -> [f32; 3] {
    DEFAULT_GROUPED_RGB
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
    fn damage_count_defaults_to_you_and_your_felyne_and_an_empty_table_loads() {
        let count = DamageCount::default();
        let filter = count.filter();
        assert!(filter.allows(Attacker::You));
        assert!(filter.allows(Attacker::YourFelyne));
        assert!(!filter.allows(Attacker::OtherHunter));
        assert!(!filter.allows(Attacker::OtherFelyne));
        assert!(!filter.allows(Attacker::Other));
        assert!(!filter.allows(Attacker::Unknown));
        assert!(!count.gray_uncounted);
        assert_eq!(filter, AttackerFilter::default());
        let old: DamageCount = toml::from_str("").expect("missing table");
        assert_eq!(old, count);
        let partial: DamageCount = toml::from_str("unknown = true\n").expect("partial");
        assert!(partial.unknown);
        assert!(partial.you);
        assert!(!partial.other_hunters);
        let round: DamageCount = toml::from_str(&toml::to_string(&count).unwrap()).unwrap();
        assert_eq!(round, count);
    }

    #[test]
    fn corner_mode_and_grouped_drops_default_when_the_keys_are_missing() {
        assert_eq!(CornerSettings::default().mode, CornerMeterMode::Session);
        assert!(GroupedHpSettings::default().show);
        assert_eq!(GroupedHpSettings::default().rgb, DEFAULT_GROUPED_RGB);
        let corner: CornerSettings = toml::from_str("show_dps = false\n").expect("old corner");
        assert!(!corner.show_dps);
        assert!(corner.show_total);
        assert_eq!(corner.mode, CornerMeterMode::Session);
        assert_eq!(corner.size_pt, DEFAULT_CORNER_PT);
        let grouped: GroupedHpSettings = toml::from_str("").expect("missing grouped");
        assert_eq!(grouped, GroupedHpSettings::default());
        let hidden: GroupedHpSettings =
            toml::from_str("show = false\nrgb = [0.2, 0.3, 0.4]\n").unwrap();
        assert!(!hidden.show);
        assert_eq!(hidden.rgb, [0.2, 0.3, 0.4]);
        #[derive(Deserialize)]
        struct ModeFile {
            mode: CornerMeterMode,
        }
        let whole: ModeFile = toml::from_str("mode = \"whole_quest\"").unwrap();
        assert_eq!(whole.mode, CornerMeterMode::WholeQuest);
        let current: ModeFile = toml::from_str("mode = \"current_monster\"").unwrap();
        assert_eq!(current.mode, CornerMeterMode::CurrentMonster);
        let session: ModeFile = toml::from_str("mode = \"session\"").unwrap();
        assert_eq!(session.mode, CornerMeterMode::Session);
    }

    #[test]
    fn clamping_rejects_nan_and_out_of_range_sizes() {
        assert_eq!(clamp_px(f32::NAN, NUMBER_PX_RANGE, 36.0), 36.0);
        assert_eq!(clamp_px(1000.0, NUMBER_PX_RANGE, 36.0), 96.0);
        assert_eq!(clamp_px(1.0, CORNER_PT_RANGE, 22.0), 12.0);
        assert_eq!(clamp_px(30.0, NUMBER_PX_RANGE, 36.0), 30.0);
    }
}
