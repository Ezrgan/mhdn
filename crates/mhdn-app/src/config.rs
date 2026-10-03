//! Overlay `config.toml`. One calibration record per Azahar layout.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mhdn_game::Snapshot;
use mhdn_platform::Insets;
use mhdn_proj::{LayoutOption, LayoutSettings, ScreenRect};
use serde::{Deserialize, Serialize};

use crate::settings::{
    clamp_px, CornerSettings, NumberSettings, CORNER_PT_RANGE, DEFAULT_CORNER_PT,
    DEFAULT_NUMBER_PX, NUMBER_PX_RANGE,
};

const DEFAULT_LATENCY_MS: u64 = 33;
const DEFAULT_TEXT_SCALE: f32 = 2.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OverlayConfig {
    #[serde(default = "default_latency")]
    pub display_latency_ms: u64,
    #[serde(default)]
    pub layout_override: Option<String>,
    #[serde(default = "default_debug_hud")]
    pub debug_hud: bool,
    #[serde(default)]
    pub insets: InsetConfig,
    #[serde(default)]
    pub style: StyleConfig,
    #[serde(default)]
    pub calibration: BTreeMap<String, LayoutCalibration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct InsetConfig {
    pub title_bar_pt: Option<f32>,
    pub status_bar_pt: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StyleConfig {
    #[serde(default = "default_text_scale")]
    pub text_scale: f32,
    #[serde(default = "default_on")]
    pub show_numbers: bool,
    #[serde(default = "default_on")]
    pub show_recount: bool,
    #[serde(default = "default_number_px")]
    pub number_px: f32,
    #[serde(default)]
    pub anchor: NumberAnchor,
    /// Per-category color and visibility. Missing in older files, which show everything.
    #[serde(default)]
    pub numbers: NumberSettings,
    /// The total and DPS recount in the corner. Missing in older files, which show both.
    #[serde(default)]
    pub corner: CornerSettings,
}

/// Where a hit's number spawns until the exact contact point is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NumberAnchor {
    /// Between the hunter and the monster, at weapon reach. Always near the middle of the view.
    #[default]
    Hunter,
    /// Above the monster's body, by the species anchor height.
    Monster,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct LayoutCalibration {
    pub nudge_x: f32,
    pub nudge_y: f32,
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            display_latency_ms: DEFAULT_LATENCY_MS,
            layout_override: None,
            debug_hud: false,
            insets: InsetConfig::default(),
            style: StyleConfig::default(),
            calibration: BTreeMap::new(),
        }
    }
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            text_scale: DEFAULT_TEXT_SCALE,
            show_numbers: true,
            show_recount: true,
            number_px: DEFAULT_NUMBER_PX,
            anchor: NumberAnchor::default(),
            numbers: NumberSettings::default(),
            corner: CornerSettings::default(),
        }
    }
}

impl StyleConfig {
    /// Floating number size in points, kept inside what the renderer can draw.
    pub fn number_size(&self) -> f32 {
        clamp_px(self.number_px, NUMBER_PX_RANGE, DEFAULT_NUMBER_PX)
    }

    /// Corner recount size in points, kept inside what the renderer can draw.
    pub fn corner_size(&self) -> f32 {
        clamp_px(self.corner.size_pt, CORNER_PT_RANGE, DEFAULT_CORNER_PT)
    }
}

fn default_latency() -> u64 {
    DEFAULT_LATENCY_MS
}

fn default_debug_hud() -> bool {
    false
}

fn default_text_scale() -> f32 {
    DEFAULT_TEXT_SCALE
}

fn default_on() -> bool {
    true
}

fn default_number_px() -> f32 {
    DEFAULT_NUMBER_PX
}

impl OverlayConfig {
    pub fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        match toml::from_str(&text) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("mhdn: config {}: {err}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        fs::write(path, text)
    }

    pub fn calibration_for(&self, layout: &str) -> LayoutCalibration {
        self.calibration.get(layout).copied().unwrap_or_default()
    }

    pub fn set_calibration(&mut self, layout: impl Into<String>, value: LayoutCalibration) {
        self.calibration.insert(layout.into(), value);
    }

    pub fn latency(&self) -> Duration {
        Duration::from_millis(self.display_latency_ms)
    }

    pub fn insets_for(&self, show_status_bar: bool) -> Insets {
        let mut insets = Insets::chrome(show_status_bar);
        if let Some(title) = self.insets.title_bar_pt {
            insets.title_bar = title;
        }
        if let Some(status) = self.insets.status_bar_pt {
            insets.status_bar = status;
        }
        insets
    }

    /// Layout the overlay should use. A named override replaces Azahar's option.
    pub fn effective_layout(&self, mut detected: LayoutSettings) -> LayoutSettings {
        if let Some(name) = self.layout_override.as_deref() {
            if let Some(option) = parse_layout_name(name) {
                detected.option = option;
            }
        }
        detected
    }
}

pub fn layout_name(option: LayoutOption) -> &'static str {
    match option {
        LayoutOption::Default => "default",
        LayoutOption::SingleScreen => "single_screen",
        LayoutOption::LargeScreen => "large_screen",
        LayoutOption::SideScreen => "side_screen",
        LayoutOption::SeparateWindows => "separate_windows",
        LayoutOption::HybridScreen => "hybrid_screen",
        LayoutOption::CustomLayout => "custom",
    }
}

pub fn parse_layout_name(name: &str) -> Option<LayoutOption> {
    let normalized = name.trim().to_ascii_lowercase().replace('-', "_");
    Some(match normalized.as_str() {
        "default" => LayoutOption::Default,
        "single_screen" | "singlescreen" => LayoutOption::SingleScreen,
        "large_screen" | "largescreen" => LayoutOption::LargeScreen,
        "side_screen" | "sidescreen" => LayoutOption::SideScreen,
        "separate_windows" | "separatewindows" => LayoutOption::SeparateWindows,
        "hybrid_screen" | "hybridscreen" => LayoutOption::HybridScreen,
        "custom" | "custom_layout" | "customlayout" => LayoutOption::CustomLayout,
        _ => return None,
    })
}

pub fn apply_calibration(rect: ScreenRect, cal: LayoutCalibration) -> ScreenRect {
    ScreenRect::new(
        rect.x + cal.left + cal.nudge_x,
        rect.y + cal.top + cal.nudge_y,
        (rect.width - cal.left - cal.right).max(1.0),
        (rect.height - cal.top - cal.bottom).max(1.0),
    )
}

/// A few recent snapshots, so the HUD can render one display-latency behind RAM.
#[derive(Debug, Default)]
pub struct SnapshotDelay {
    frames: VecDeque<Snapshot>,
}

impl SnapshotDelay {
    pub fn push(&mut self, snapshot: Snapshot) {
        self.frames.push_back(snapshot);
        while self.frames.len() > 8 {
            self.frames.pop_front();
        }
    }

    pub fn latest(&self) -> Option<&Snapshot> {
        self.frames.back()
    }

    pub fn sample(&self, latency: Duration) -> Option<&Snapshot> {
        let latest = self.frames.back()?;
        let lag = u64::try_from(latency.as_micros()).unwrap_or(u64::MAX);
        let target = latest.host_us.saturating_sub(lag);
        self.frames
            .iter()
            .rev()
            .find(|frame| frame.host_us <= target)
            .or_else(|| self.frames.front())
    }
}

#[cfg(target_os = "macos")]
pub fn config_dir() -> PathBuf {
    home().join("Library/Application Support/mhdn")
}

#[cfg(target_os = "windows")]
pub fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mhdn")
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn config_dir() -> PathBuf {
    home().join(".config/mhdn")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

#[cfg(not(target_os = "windows"))]
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhdn_game::{CameraState, FovUnit, Scene, Vec3};

    #[test]
    fn config_roundtrips_and_a_missing_file_is_the_default() {
        let path =
            std::env::temp_dir().join(format!("mhdn-config-test-{}.toml", std::process::id()));
        let _ = fs::remove_file(&path);
        assert_eq!(OverlayConfig::load(&path).display_latency_ms, 33);
        let mut config = OverlayConfig {
            display_latency_ms: 50,
            layout_override: Some("custom".to_string()),
            debug_hud: false,
            insets: InsetConfig {
                title_bar_pt: Some(30.0),
                ..InsetConfig::default()
            },
            style: StyleConfig {
                text_scale: 3.0,
                ..StyleConfig::default()
            },
            ..OverlayConfig::default()
        };
        config.set_calibration(
            "custom",
            LayoutCalibration {
                nudge_x: 2.0,
                left: 4.0,
                ..LayoutCalibration::default()
            },
        );
        config.save(&path).unwrap();
        let loaded = OverlayConfig::load(&path);
        assert_eq!(loaded, config);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn an_old_config_without_the_new_keys_still_loads_with_todays_look() {
        let old = "display_latency_ms = 40\n\n[style]\ntext_scale = 2.5\nshow_numbers = false\nshow_recount = true\nnumber_px = 30.0\nanchor = \"monster\"\n";
        let config: OverlayConfig = toml::from_str(old).expect("old file parses");
        assert_eq!(config.display_latency_ms, 40);
        assert_eq!(config.style.text_scale, 2.5);
        assert!(!config.style.show_numbers);
        assert_eq!(config.style.number_px, 30.0);
        assert_eq!(config.style.anchor, NumberAnchor::Monster);
        assert_eq!(config.style.numbers, NumberSettings::default());
        assert_eq!(config.style.corner, CornerSettings::default());
        let bare: OverlayConfig = toml::from_str("").expect("empty file parses");
        assert_eq!(bare, OverlayConfig::default());
    }

    #[test]
    fn a_partial_new_section_fills_the_rest_with_defaults() {
        let text = "[style.numbers.poison]\nshow = false\nrgb = [0.1, 0.2, 0.3]\n\n[style.corner]\nshow_dps = false\n";
        let config: OverlayConfig = toml::from_str(text).expect("partial file parses");
        assert!(!config.style.numbers.poison.show);
        assert_eq!(config.style.numbers.poison.rgb, [0.1, 0.2, 0.3]);
        assert_eq!(config.style.numbers.large, NumberSettings::default().large);
        assert!(!config.style.corner.show_dps);
        assert!(config.style.corner.show_total);
        assert_eq!(config.style.corner.size_pt, DEFAULT_CORNER_PT);
    }

    #[test]
    fn edited_settings_roundtrip_through_the_file() {
        let path =
            std::env::temp_dir().join(format!("mhdn-settings-test-{}.toml", std::process::id()));
        let mut config = OverlayConfig::default();
        config.style.anchor = NumberAnchor::Monster;
        config.style.number_px = 48.0;
        config.style.numbers.small.show = false;
        config.style.numbers.large.rgb = [0.25, 0.5, 0.75];
        config.style.corner.show_total = false;
        config.style.corner.size_pt = 30.0;
        config.save(&path).unwrap();
        assert_eq!(OverlayConfig::load(&path), config);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_bad_size_in_the_file_is_clamped_when_used() {
        let style = StyleConfig {
            number_px: f32::NAN,
            corner: CornerSettings {
                size_pt: 5000.0,
                ..CornerSettings::default()
            },
            ..StyleConfig::default()
        };
        assert_eq!(style.number_size(), DEFAULT_NUMBER_PX);
        assert_eq!(style.corner_size(), CORNER_PT_RANGE.1);
        assert_eq!(StyleConfig::default().number_size(), 36.0);
        assert_eq!(StyleConfig::default().corner_size(), 22.0);
    }

    #[test]
    fn an_override_replaces_the_detected_layout_option() {
        let config = OverlayConfig {
            layout_override: Some("Large-Screen".to_string()),
            ..OverlayConfig::default()
        };
        let effective = config.effective_layout(LayoutSettings::default());
        assert_eq!(effective.option, LayoutOption::LargeScreen);
        assert_eq!(layout_name(effective.option), "large_screen");
    }

    #[test]
    fn calibration_insets_move_the_top_screen() {
        let rect = ScreenRect::new(10.0, 20.0, 400.0, 240.0);
        let moved = apply_calibration(
            rect,
            LayoutCalibration {
                nudge_x: 1.0,
                nudge_y: -2.0,
                left: 3.0,
                top: 4.0,
                right: 5.0,
                bottom: 6.0,
            },
        );
        assert_eq!(moved, ScreenRect::new(14.0, 22.0, 392.0, 230.0));
    }

    #[test]
    fn display_latency_selects_an_older_snapshot() {
        let mut delay = SnapshotDelay::default();
        delay.push(frame(0));
        delay.push(frame(33_000));
        let sampled = delay.sample(Duration::from_millis(33)).unwrap();
        assert_eq!(sampled.host_us, 0);
        assert_eq!(
            delay.sample(Duration::from_millis(0)).unwrap().host_us,
            33_000
        );
    }

    fn frame(host_us: u64) -> Snapshot {
        Snapshot {
            guest_frame: 1,
            host_us,
            scene: Scene::Village,
            camera: Some(CameraState {
                eye: Vec3::new(0.0, 0.0, 1.0),
                target: Vec3::new(0.0, 0.0, 0.0),
                fov_y: 1.0,
                fov_unit: FovUnit::Rad,
            }),
            hunter_pos: None,
            monsters: Vec::new(),
        }
    }
}
