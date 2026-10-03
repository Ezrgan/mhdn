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

## Addendum: native fullscreen and Game Mode (measured)

- **Space membership.** The flags alone were not enough. The window was created visible while the app was
  still `Regular` and only switched to `Accessory` afterwards, so it never joined Azahar's fullscreen space:
  `CGWindowList` showed no mhdn window there. The event loop now starts as `Accessory`, and the window is
  created hidden, configured, then shown. `join_active_space` reorders it if it is ever off the active space.
- **Game Mode throttling.** Azahar's fullscreen turns on Game Mode. In that mode every wake of a thread in
  a background process lands about 116 ms late. This was measured with `tools/sleepprobe.swift`:
  - it applies to default QoS, user-interactive QoS, Mach time-constraint (real-time) threads, and threads
    woken by a `CVDisplayLink`;
  - `NSActivityUserInteractive` does not lift it either;
  - RPC latency stays at 0.1 ms, but the sampler thread fell from about 57 to 8 samples/s and the numbers
    stuttered.
- **Main-thread pump.** The main thread keeps vsync while it presents frames. In a fullscreen hunt it
  therefore renders continuously and calls `Pump::pump()` every frame, sampling on the sampler's own
  schedule. This restores about 42 samples/s, above the game's 30 fps. Elsewhere the sampler thread alone
  does the work.

## Addendum: native fullscreen and Game Mode (measured)

- **Space membership.** Setting the flags after winit shows the window as a regular app is not enough. The
  window stayed on the desktop space and never appeared in Azahar's fullscreen space. The event loop now
  starts with the accessory policy, and the window is created hidden, configured, and then shown. `follow()`
  reorders it with `orderFrontRegardless` if it is ever off the active space.
- **Game Mode throttling.** Fullscreen Azahar turns on Game Mode. While it is on, every wake-up of every
  thread in our process lands about 116 ms late. This was measured with `tools/sleepprobe.swift` and holds
  for default QoS, user-interactive QoS, a Mach time-constraint (real-time) thread, and a `CVDisplayLink`
  callback. The sampler fell from about 57 to 8 samples/s, so the text moved in steps. RPC latency did not
  change (0.12 ms), and neither `NSActivityLatencyCritical` nor a QoS raise helped.
- **What still runs on time.** The main thread keeps vsync pace while it presents frames. In a fullscreen
  hunt it now renders continuously and takes samples itself through `mhdn_game::Pump` whenever the period
  is due. That brings the rate back to about 42 samples/s, above the game's 30 fps. Windowed play is
  unchanged: the sampler thread keeps up and the main thread never samples.
