//! AppKit adjustments on the winit `NSWindow`. See `docs/adr/0005`.

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSColor, NSScreenSaverWindowLevel, NSStatusBar,
    NSStatusItem, NSVariableStatusItemLength, NSView, NSWindow, NSWindowCollectionBehavior,
};
use objc2_foundation::NSString;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
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
