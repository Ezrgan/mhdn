# ADR-0002: Rust + wgpu for the overlay

## Status

Accepted (2026-09-27)

## Context

The overlay must render crisp text at monitor resolution, composite with premultiplied alpha, stay within
tight CPU/GPU budgets (~5% of one core in combat), and run on macOS (Metal) and Windows (DX12/Vulkan).
Existing MHXX overlays use Python/PyQt and show GC jitter and higher CPU use.

## Decision

Use a **Rust workspace** with **winit** for events/windowing, **wgpu** for rendering, and **glam** for math.
Platform-specific overlay behavior (click-through, always-on-top, window tracking) lives in `mhdn-platform`
with **objc2** on macOS and **windows-rs** on Windows.

## Alternatives considered

| Option | Why not chosen |
|--------|----------------|
| C++ / ImGui / GLFW | Viable performance; less memory safety; more boilerplate for cross-platform window tricks |
| Swift / AppKit only | Excellent on macOS; no shared code for Windows |
| Python / PyQt | Fast prototype; GC and GIL hurt steady 60–120 Hz overlay timing |
| Electron | Too heavy for a lightweight always-on-top utility |

## Consequences

- **Positive:** Predictable performance, single renderer across APIs, strong ecosystem for CLI tooling (`mhdn-probe`).
- **Negative:** Steeper learning curve than Python; macOS may need native NSPanel work beyond winit defaults (see F5.1).
- **Follow-ups:** MVP text via `fontdue`; MSDF atlas in Phase 8; `tracing` for observability in Phase 9.
