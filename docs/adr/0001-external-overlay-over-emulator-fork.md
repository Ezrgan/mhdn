# ADR-0001: External overlay instead of an emulator fork

## Status

Accepted (2026-09-27)

## Context

Damage numbers require knowing when damage occurs, where in the world to anchor text, and how to map that
point to pixels on the host display. An emulator fork could draw inside Azahar's swap chain with perfect
sync and direct access to GPU uniforms, but maintaining a large C++ fork (GPL, continuous rebases) conflicts
with the goal of a small, user-friendly tool that works with stock Azahar builds.

Alternatives considered: in-process DLL injection into Azahar (fragile, backend-specific, poor on macOS with
SIP), cheat codes only (no structured events), and drawing from a 3GX plugin at 400×240 (blurry, sync issues).

## Decision

Build a **separate host process** that reads game state through Azahar's **official UDP RPC** (passive mode)
and optionally a **3GX plugin** that publishes hit events (active mode). All rendering happens in a native
transparent overlay window (Rust + wgpu), never inside the emulated framebuffer.

## Consequences

- **Positive:** No custom Azahar build; multi-backend graphics on the host; clear separation of concerns;
  GPLv3-compatible reuse of Azahar layout math without shipping emulator code.
- **Negative:** Window tracking and layout reimplementation on each OS; RPC reads are asynchronous (mitigated
  with seqlock on the guest frame counter); slight latency vs. in-emulator drawing.
- **Follow-ups:** Platform modules for macOS (primary) and Windows (F10); spike for fullscreen overlay (ADR-0005).
