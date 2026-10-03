//! Hosts other than macOS keep a plain winit window until Phase 10.

#![forbid(unsafe_code)]

use crate::error::PlatformError;

pub fn apply_click_through(
    _window: &winit::window::Window,
    _click_through: bool,
) -> Result<(), PlatformError> {
    Ok(())
}

pub fn overlay_event_loop() -> Result<winit::event_loop::EventLoop<()>, winit::error::EventLoopError>
{
    winit::event_loop::EventLoop::new()
}

pub fn join_active_space(_window: &winit::window::Window) -> Result<bool, PlatformError> {
    Ok(false)
}

pub struct LatencyCritical;

pub fn begin_latency_critical() -> LatencyCritical {
    LatencyCritical
}

pub fn raise_thread_qos() -> bool {
    false
}

pub fn frontmost_pid() -> Option<i32> {
    None
}

/// Menu-bar status is a macOS item. Other hosts only keep the label API.
pub struct MenuStatus;

impl MenuStatus {
    pub fn install() -> Result<Self, PlatformError> {
        Ok(Self)
    }

    pub fn set_title(&self, _title: &str) {}
}
