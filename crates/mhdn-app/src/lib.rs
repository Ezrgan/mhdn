//! Overlay application: track Azahar, project anchors, draw the debug HUD.

#![forbid(unsafe_code)]

mod hud;
mod run;
mod session;

pub use hud::{build_hud, camera_from, scene_label, HudStats};
pub use run::run;
pub use session::{sample_rate, RateWindow, RpcMeter};
