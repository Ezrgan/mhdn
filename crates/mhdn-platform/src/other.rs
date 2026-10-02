//! Hosts other than macOS keep a plain winit window until Phase 10.

#![forbid(unsafe_code)]

use crate::error::PlatformError;

pub fn apply_click_through(
    _window: &winit::window::Window,
    _click_through: bool,
) -> Result<(), PlatformError> {
    Ok(())
}

/// Menu-bar status is a macOS item. Other hosts only keep the label API.
pub struct MenuStatus;

impl MenuStatus {
    pub fn install() -> Result<Self, PlatformError> {
        Ok(Self)
    }

    pub fn set_title(&self, _title: &str) {}
}
