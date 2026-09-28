# ADR-0004: Projection from reconstructed camera parameters

## Status

Accepted (2026-09-27)

## Context

To place numbers on monsters, world positions must be projected with the same view/projection the game uses
for the top screen (400×240). Options include reading the GPU view matrix from RAM or PICA uniform buffers,
or reconstructing `eye`, `target`, and `fov_y` from game structures.

MHXX / MT Framework on 3DS often applies a **90° rotation** in the path to the PICA; a raw matrix in memory
may not match naive `look_at` + `perspective` without extra unwrapping. Matrix layout and alignment in RAM are
also RE targets with more false positives than geometric camera parameters.

## Decision

**Primary:** locate **camera parameters** (eye, target, up, vertical FOV) in guest memory and build
`V = look_at_rh(eye, target, up)` and `P = perspective_rh(fov_y, aspect = 400/240, near, far)` in `mhdn-proj`.

**Contingency:** if parameters are not found in F2, use a validated **view matrix** from RAM or PICA uniforms,
with rotation detection by projecting the hunter silhouette (F2 contingency).

## Alternatives considered

- GPU matrix only: fastest if correct; high ambiguity from rotation and memory layout.
- 2D HUD fixed near monster icons: avoids camera RE but fails the MHW-like world-anchored goal; reserved as last-resort MVP HUD.

## Consequences

- **Positive:** Clear validation (hunter/monster reprojection in debug HUD); tests with golden vectors; optional
  consistent camera block from plugin in active mode (seqlock).
- **Negative:** F2 camera RE is on the critical path; interpolation needed because guest runs at 30 FPS (F4.4).
- **Follow-ups:** Port Azahar `framebuffer_layout.cpp` math; read `qt-config.ini` for layout; `display_latency_ms` tuning.
