//! AppKit adjustments on the winit `NSWindow`. See `docs/adr/0005`.

use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSColor, NSScreenSaverWindowLevel, NSStatusBar,
    NSStatusItem, NSVariableStatusItemLength, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWorkspace,
};
use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::error::EventLoopError;
use winit::event_loop::EventLoop;
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
use winit::window::Window;

use crate::error::PlatformError;

/// Click-through, clear, topmost, and allowed next to a native fullscreen space.
///
/// `click_through` is false while calibration needs the mouse and the keyboard.
/// A regular activation policy is required for that, so the overlay can become
/// key. Click-through returns the process to an accessory app.
pub fn apply_click_through(window: &Window, click_through: bool) -> Result<(), PlatformError> {
    let marker = MainThreadMarker::new().ok_or(PlatformError::NotMainThread)?;
    let ns_window = ns_window(window)?;
    let app = NSApplication::sharedApplication(marker);
    let policy = if click_through {
        NSApplicationActivationPolicy::Accessory
    } else {
        NSApplicationActivationPolicy::Regular
    };
    let _ = app.setActivationPolicy(policy);

    ns_window.setIgnoresMouseEvents(click_through);
    ns_window.setOpaque(false);
    ns_window.setHasShadow(false);
    ns_window.setMovable(false);
    ns_window.setBackgroundColor(Some(&NSColor::clearColor()));
    ns_window.setLevel(NSScreenSaverWindowLevel);
    ns_window.setCollectionBehavior(collection_behavior());
    Ok(())
}

/// Event loop for an accessory app. Space membership is decided when a window is
/// first ordered in, and a regular app's windows never join another app's
/// native fullscreen space.
pub fn overlay_event_loop() -> Result<EventLoop<()>, EventLoopError> {
    EventLoop::builder()
        .with_activation_policy(ActivationPolicy::Accessory)
        .with_activate_ignoring_other_apps(false)
        .build()
}

/// Shows the overlay on the active space. Returns true when it had to be reordered.
pub fn join_active_space(window: &Window) -> Result<bool, PlatformError> {
    let ns_window = ns_window(window)?;
    if ns_window.isVisible() && ns_window.isOnActiveSpace() {
        return Ok(false);
    }
    ns_window.orderFrontRegardless();
    Ok(true)
}

/// Keeps App Nap and timer coalescing off while alive. Without it, an accessory app
/// under Game Mode sees its 16 ms sampler sleeps stretch to about 120 ms.
pub struct LatencyCritical {
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

pub fn begin_latency_critical() -> LatencyCritical {
    let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
        NSActivityOptions::UserInteractive,
        &NSString::from_str("mhdn follows the game frame by frame"),
    );
    LatencyCritical { token }
}

impl Drop for LatencyCritical {
    fn drop(&mut self) {
        unsafe { NSProcessInfo::processInfo().endActivity(&self.token) };
    }
}

const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;

extern "C" {
    fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
}

/// Puts the calling thread in the user-interactive QoS class. Returns false if the kernel refused.
pub fn raise_thread_qos() -> bool {
    unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) == 0 }
}

/// Process id of the app that owns the keyboard. `None` when AppKit has no answer.
pub fn frontmost_pid() -> Option<i32> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
}

/// Translucent red fill used by the spike, so the panel is visible without wgpu.
pub fn tint_spike(window: &Window) -> Result<(), PlatformError> {
    let ns_window = ns_window(window)?;
    let color = NSColor::colorWithRed_green_blue_alpha(1.0, 0.2, 0.15, 0.45);
    ns_window.setBackgroundColor(Some(&color));
    Ok(())
}

/// Menu-bar title. The item has to stay alive or AppKit removes it.
pub struct MenuStatus {
    item: Retained<NSStatusItem>,
    marker: MainThreadMarker,
}

impl MenuStatus {
    pub fn install() -> Result<Self, PlatformError> {
        let marker = MainThreadMarker::new().ok_or(PlatformError::NotMainThread)?;
        let bar = NSStatusBar::systemStatusBar();
        let item = bar.statusItemWithLength(NSVariableStatusItemLength);
        let status = Self { item, marker };
        status.set_title("mhdn");
        Ok(status)
    }

    pub fn set_title(&self, title: &str) {
        let Some(button) = self.item.button(self.marker) else {
            return;
        };
        button.setTitle(&NSString::from_str(title));
    }
}

fn collection_behavior() -> NSWindowCollectionBehavior {
    NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::Stationary
        | NSWindowCollectionBehavior::IgnoresCycle
        | NSWindowCollectionBehavior::FullScreenAuxiliary
}

fn ns_window(window: &Window) -> Result<Retained<NSWindow>, PlatformError> {
    let handle = window
        .window_handle()
        .map_err(|err| PlatformError::Handle(err.to_string()))?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err(PlatformError::NotAppKit);
    };
    let view = unsafe { Retained::<NSView>::retain(appkit.ns_view.as_ptr().cast::<NSView>()) }
        .ok_or(PlatformError::NoWindow)?;
    view.window().ok_or(PlatformError::NoWindow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::overlay_collection_behavior;

    #[test]
    fn collection_bits_match_appkit() {
        assert_eq!(
            collection_behavior().bits(),
            overlay_collection_behavior() as _
        );
        assert_eq!(NSScreenSaverWindowLevel, 1000);
    }
}
