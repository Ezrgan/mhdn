//! Overlay application: track Azahar, project anchors, draw damage numbers.

#![forbid(unsafe_code)]

mod calibrate;
mod config;
mod diag;
mod hud;
mod numbers;
mod run;
mod session;
mod settings;
mod settings_window;
mod status;
mod trace;

pub use hud::{build_hud, camera_from, scene_label, HudStats};
pub use run::run;
pub use session::{sample_rate, RateWindow, RpcMeter};
