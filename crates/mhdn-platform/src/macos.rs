//! AppKit adjustments on the winit `NSWindow`. See `docs/adr/0005`.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSColor, NSMenu, NSMenuItem,
    NSScreenSaverWindowLevel, NSStatusBar, NSStatusItem, NSVariableStatusItemLength, NSView,
    NSWindow, NSWindowCollectionBehavior, NSWorkspace,
};
use objc2_foundation::{NSActivityOptions, NSObject, NSProcessInfo, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::error::EventLoopError;
use winit::event_loop::EventLoop;
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
use winit::window::Window;

use crate::error::PlatformError;
use crate::OverlayUserEvent;

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
pub fn overlay_event_loop() -> Result<EventLoop<OverlayUserEvent>, EventLoopError> {
    let mut builder = EventLoop::<OverlayUserEvent>::with_user_event();
    builder
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

struct MenuIvars {
    on_event: Box<dyn Fn(OverlayUserEvent)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = MenuIvars]
    struct MenuHandler;

    impl MenuHandler {
        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            (self.ivars().on_event)(OverlayUserEvent::Quit);
        }

        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: Option<&AnyObject>) {
            (self.ivars().on_event)(OverlayUserEvent::ShowSettings);
        }
    }
);

impl MenuHandler {
    fn new(marker: MainThreadMarker, on_event: Box<dyn Fn(OverlayUserEvent)>) -> Retained<Self> {
        let this = Self::alloc(marker).set_ivars(MenuIvars { on_event });
        unsafe { msg_send![super(this), init] }
    }
}

/// Menu-bar item. The item, its menu, and the quit target have to stay alive
/// or AppKit drops them. The button shows the live status; the menu is how you quit.
pub struct MenuStatus {
    item: Retained<NSStatusItem>,
    /// AppKit does not retain the menu on our behalf for the lifetime we need.
    #[allow(dead_code)]
    menu: Retained<NSMenu>,
    /// `setTarget` does not retain the action target.
    #[allow(dead_code)]
    handler: Retained<MenuHandler>,
    marker: MainThreadMarker,
}

impl MenuStatus {
    pub fn install(on_event: impl Fn(OverlayUserEvent) + 'static) -> Result<Self, PlatformError> {
        let marker = MainThreadMarker::new().ok_or(PlatformError::NotMainThread)?;
        let bar = NSStatusBar::systemStatusBar();
        let item = bar.statusItemWithLength(NSVariableStatusItemLength);
        let handler = MenuHandler::new(marker, Box::new(on_event));
        let menu = NSMenu::new(marker);
        let settings = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(marker),
                &NSString::from_str("Settings\u{2026}"),
                Some(sel!(showSettings:)),
                &NSString::from_str(""),
            )
        };
        let quit = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(marker),
                &NSString::from_str("Quit"),
                Some(sel!(quit:)),
                &NSString::from_str("q"),
            )
        };
        let target = Retained::as_ptr(&handler).cast::<AnyObject>();
        // The status item does not retain the action target.
        unsafe {
            settings.setTarget(Some(&*target));
            quit.setTarget(Some(&*target));
        };
        menu.addItem(&settings);
        menu.addItem(&quit);
        item.setMenu(Some(&menu));
        let status = Self {
            item,
            menu,
            handler,
            marker,
        };
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
