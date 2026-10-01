//! Azahar `qt-config.ini` layout keys.
//!
//! Qt writes `key=value` plus `key\default=bool`. Enumerations are integers.
//! A keyboard shortcut that changes the layout without saving the ini is not
//! visible here; the overlay config can override it later.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

use crate::layout::{AspectRatio, CustomRect, LayoutInput, LayoutOption, SmallScreenPosition};

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read Azahar config {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to watch Azahar config {path}: {source}")]
    Watch {
        path: PathBuf,
        #[source]
        source: notify::Error,
    },
    #[error("invalid value for {key}: {value}")]
    Invalid { key: String, value: String },
}

/// Layout-related keys. Missing keys keep Azahar's defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutSettings {
    pub option: LayoutOption,
    pub large_screen_proportion: f32,
    pub small_screen_position: SmallScreenPosition,
    pub swap_screen: bool,
    pub upright_screen: bool,
    pub screen_top_stretch: bool,
    pub aspect_ratio: AspectRatio,
    pub custom_top: CustomRect,
    pub custom_bottom: CustomRect,
    pub show_status_bar: bool,
    pub single_window_mode: bool,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            option: LayoutOption::Default,
            large_screen_proportion: 4.0,
            small_screen_position: SmallScreenPosition::TopRight,
            swap_screen: false,
            upright_screen: false,
            screen_top_stretch: false,
            aspect_ratio: AspectRatio::Native,
            custom_top: CustomRect::default(),
            custom_bottom: CustomRect::default(),
            show_status_bar: false,
            single_window_mode: true,
        }
    }
}

impl LayoutSettings {
    pub fn layout(&self, width: u32, height: u32, separate_secondary: bool) -> LayoutInput {
        LayoutInput {
            width,
            height,
            option: self.option,
            large_screen_proportion: self.large_screen_proportion,
            small_screen_position: self.small_screen_position,
            swap_screen: self.swap_screen,
            upright_screen: self.upright_screen,
            screen_top_stretch: self.screen_top_stretch,
            aspect_ratio: self.aspect_ratio,
            custom_top: self.custom_top,
            custom_bottom: self.custom_bottom,
            separate_secondary,
        }
    }
}

pub fn parse_layout_settings(text: &str) -> Result<LayoutSettings, ConfigError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut settings = LayoutSettings::default();
    let mut custom_top = [0.0_f32; 4];
    let mut custom_bottom = [0.0_f32; 4];
    let mut saw_top = [false; 4];
    let mut saw_bottom = [false; 4];

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with(';')
            || line.starts_with('[')
        {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.contains('\\') {
            continue;
        }
        let value = unquote(value.trim());
        apply(
            &mut settings,
            key,
            value,
            &mut custom_top,
            &mut custom_bottom,
            &mut saw_top,
            &mut saw_bottom,
        )?;
    }

    if saw_top.iter().any(|seen| *seen) {
        settings.custom_top = CustomRect {
            x: custom_top[0],
            y: custom_top[1],
            width: custom_top[2],
            height: custom_top[3],
        };
    }
    if saw_bottom.iter().any(|seen| *seen) {
        settings.custom_bottom = CustomRect {
            x: custom_bottom[0],
            y: custom_bottom[1],
            width: custom_bottom[2],
            height: custom_bottom[3],
        };
    }
    Ok(settings)
}

fn apply(
    settings: &mut LayoutSettings,
    key: &str,
    value: &str,
    custom_top: &mut [f32; 4],
    custom_bottom: &mut [f32; 4],
    saw_top: &mut [bool; 4],
    saw_bottom: &mut [bool; 4],
) -> Result<(), ConfigError> {
    let invalid = || ConfigError::Invalid {
        key: key.to_string(),
        value: value.to_string(),
    };
    match key {
        "layout_option" => {
            settings.option = LayoutOption::from_ini(parse_i64(value).ok_or_else(invalid)?)
                .ok_or_else(invalid)?;
        }
        "large_screen_proportion" => {
            let proportion: f32 = value.parse().map_err(|_| invalid())?;
            if !proportion.is_finite() || proportion <= 0.0 {
                return Err(invalid());
            }
            settings.large_screen_proportion = proportion;
        }
        "small_screen_position" => {
            settings.small_screen_position =
                SmallScreenPosition::from_ini(parse_i64(value).ok_or_else(invalid)?)
                    .ok_or_else(invalid)?;
        }
        "swap_screen" => settings.swap_screen = parse_bool(value).ok_or_else(invalid)?,
        "upright_screen" => settings.upright_screen = parse_bool(value).ok_or_else(invalid)?,
        "screen_top_stretch" => {
            settings.screen_top_stretch = parse_bool(value).ok_or_else(invalid)?
        }
        "aspect_ratio" => {
            settings.aspect_ratio =
                AspectRatio::from_ini(parse_i64(value).ok_or_else(invalid)?).ok_or_else(invalid)?;
        }
        "showStatusBar" => settings.show_status_bar = parse_bool(value).ok_or_else(invalid)?,
        "singleWindowMode" => {
            settings.single_window_mode = parse_bool(value).ok_or_else(invalid)?
        }
        "custom_top_x" => set_custom(custom_top, saw_top, 0, value).map_err(|_| invalid())?,
        "custom_top_y" => set_custom(custom_top, saw_top, 1, value).map_err(|_| invalid())?,
        "custom_top_width" => set_custom(custom_top, saw_top, 2, value).map_err(|_| invalid())?,
        "custom_top_height" => set_custom(custom_top, saw_top, 3, value).map_err(|_| invalid())?,
        "custom_bottom_x" => {
            set_custom(custom_bottom, saw_bottom, 0, value).map_err(|_| invalid())?
        }
        "custom_bottom_y" => {
            set_custom(custom_bottom, saw_bottom, 1, value).map_err(|_| invalid())?
        }
        "custom_bottom_width" => {
            set_custom(custom_bottom, saw_bottom, 2, value).map_err(|_| invalid())?
        }
        "custom_bottom_height" => {
            set_custom(custom_bottom, saw_bottom, 3, value).map_err(|_| invalid())?
        }
        _ => {}
    }
    Ok(())
}

fn set_custom(
    slot: &mut [f32; 4],
    seen: &mut [bool; 4],
    index: usize,
    value: &str,
) -> Result<(), ()> {
    let parsed: f32 = value.parse().map_err(|_| ())?;
    if !parsed.is_finite() || parsed < 0.0 {
        return Err(());
    }
    slot[index] = parsed;
    seen[index] = true;
    Ok(())
}

fn parse_i64(value: &str) -> Option<i64> {
    value.parse().ok()
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
}

/// Reloads layout settings when `qt-config.ini` changes.
pub struct LayoutWatcher {
    path: PathBuf,
    settings: LayoutSettings,
    raw: String,
    rx: Receiver<notify::Result<notify::Event>>,
    _watcher: RecommendedWatcher,
}

impl LayoutWatcher {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ConfigError> {
        let path = path.into();
        let raw = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        let settings = parse_layout_settings(&raw)?;
        let (tx, rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(tx, Config::default()).map_err(|source| {
            ConfigError::Watch {
                path: path.clone(),
                source,
            }
        })?;
        let watch_at = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        watcher
            .watch(watch_at, RecursiveMode::NonRecursive)
            .map_err(|source| ConfigError::Watch {
                path: path.clone(),
                source,
            })?;
        Ok(Self {
            path,
            settings,
            raw,
            rx,
            _watcher: watcher,
        })
    }

    pub fn settings(&self) -> &LayoutSettings {
        &self.settings
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Re-read the file. Returns whether the parsed settings changed.
    /// Notify events are drained so a saved ini is picked up on the next call.
    pub fn poll(&mut self) -> Result<bool, ConfigError> {
        while self.rx.try_recv().is_ok() {}
        let raw = fs::read_to_string(&self.path).map_err(|source| ConfigError::Read {
            path: self.path.clone(),
            source,
        })?;
        if raw == self.raw {
            return Ok(false);
        }
        let settings = parse_layout_settings(&raw)?;
        let changed = settings != self.settings;
        self.settings = settings;
        self.raw = raw;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{resolve, LayoutInput, Rect};

    const SAMPLE: &str = r#"
[Layout]
layout_option\default=false
layout_option=2
large_screen_proportion=4
small_screen_position=2
swap_screen=false
upright_screen=false
screen_top_stretch=false
singleWindowMode=true
showStatusBar=true
aspect_ratio=0
custom_top_x=0
custom_top_y=0
custom_top_width=0
custom_top_height=0
"#;

    #[test]
    fn parses_the_recorded_azahar_layout() {
        let settings = parse_layout_settings(SAMPLE).unwrap();
        assert_eq!(settings.option, LayoutOption::LargeScreen);
        assert_eq!(settings.large_screen_proportion, 4.0);
        assert_eq!(
            settings.small_screen_position,
            SmallScreenPosition::BottomRight
        );
        assert!(settings.show_status_bar);
        assert!(settings.single_window_mode);
        assert!(!settings.swap_screen);
        assert!(!settings.screen_top_stretch);

        let input = settings.layout(1920, 1080, false);
        let got = resolve(&input);
        let expected = resolve(&LayoutInput::large_bottom_right(1920, 1080));
        assert_eq!(got.top, expected.top);
        assert_eq!(got.bottom, expected.bottom);
        assert_eq!(
            got.top,
            Rect {
                x: 0,
                y: 60,
                width: 1600,
                height: 960,
            }
        );
    }

    #[test]
    fn missing_keys_use_azahar_defaults() {
        let settings = parse_layout_settings("[Layout]\n").unwrap();
        assert_eq!(settings, LayoutSettings::default());
    }

    #[test]
    fn rejects_unknown_enumerations() {
        let error = parse_layout_settings("layout_option=99\n").unwrap_err();
        assert!(error.to_string().contains("layout_option"), "{error}");
    }

    #[test]
    fn reload_sees_a_rewritten_ini() {
        let path = std::env::temp_dir().join(format!("mhdn-layout-{}.ini", std::process::id()));
        fs::write(&path, SAMPLE).unwrap();
        let mut watcher = LayoutWatcher::open(&path).unwrap();
        assert_eq!(watcher.settings().option, LayoutOption::LargeScreen);
        assert!(!watcher.poll().unwrap());

        fs::write(&path, "layout_option=1\nswap_screen=true\n").unwrap();
        assert!(watcher.poll().unwrap());
        assert_eq!(watcher.settings().option, LayoutOption::SingleScreen);
        assert!(watcher.settings().swap_screen);
        let _ = fs::remove_file(&path);
    }
}
