# ADR-0005: winit window with AppKit overlay flags

## Status

Accepted (2026-10-01)

## Context

The overlay has to sit above Azahar, including when Azahar is in native macOS fullscreen, and it must not
take clicks or keystrokes. Phase 5.1 was the spike that chooses how that window is created.

winit 0.30 already owns the event loop and gives wgpu a `raw-window-handle`. On macOS that handle is an
`NSView` inside an `NSWindow`. A hand-built `NSPanel` (`.nonactivatingPanel`) can also host a wgpu surface,
but then this crate owns window lifetime, resizing, and the display link itself.

Apple's documented way to draw over a fullscreen app is an `NSWindow` (or `NSPanel`) at
`NSScreenSaverWindowLevel` whose collection behavior includes `canJoinAllSpaces`, `fullScreenAuxiliary`,
`stationary`, and `ignoresCycle`. Click-through is `ignoresMouseEvents`. The window must be non-opaque, with
a clear background and no shadow, and `NSApp` should use the accessory activation policy so the overlay does
not take over the Dock.

## Decision

Create the overlay with **winit**, then set those AppKit properties on the underlying `NSWindow` through
objc2. The spike binary is `cargo run -p mhdn-platform --example overlay-spike`: a translucent rectangle,
click-through, at screensaver level. Ctrl-C in the terminal closes it, because the window does not receive
keys while click-through is on.

A dedicated `NSPanel` stays the fallback if a future winit release hides the AppKit window, or if native
fullscreen still covers this window on a newer macOS. Windows click-through is out of scope until Phase 10;
on other hosts the same winit window is created and the AppKit call is a no-op.

## Alternatives considered

| Option | Why not chosen |
|--------|----------------|
| `NSPanel` owned by this crate, wgpu via `raw-window-handle` only | Duplicates the event loop winit already provides. Revisit only if the `NSWindow` path fails on fullscreen. |
| winit defaults (`NSNormalWindowLevel`, activating window) | Drops behind native fullscreen and steals clicks from the game. |
| Drawing inside Azahar | Rejected in ADR-0001. |

## Consequences

- **Positive:** One window type for wgpu on macOS and Windows. The fullscreen and click-through behavior is
  an explicit, tested set of AppKit flags rather than a winit preset.
- **Negative:** The spike cannot be asserted in CI. The bit mask and window level are checked against AppKit
  on macOS; covering Azahar in fullscreen stays a manual check.
- **Follow-ups:** Phase 5.2 finds the Azahar window. Phase 5.3 moves this window onto that rectangle.
  Calibration (5.6) turns `ignoresMouseEvents` off while the user drags the screen rect.
