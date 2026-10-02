//! Hosts other than macOS keep a plain winit window until Phase 10.

#![forbid(unsafe_code)]

use crate::error::PlatformError;

pub fn apply_click_through(
    _window: &winit::window::Window,
    _click_through: bool,
) -> Result<(), PlatformError> {
    Ok(())
}
