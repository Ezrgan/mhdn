# ADR-0003: Two-tier passive and active modes with one `DamageEvent` pipeline

## Status

Accepted (2026-09-27)

## Context

Passive mode can ship earlier using known monster HP pointer chains: damage is inferred from HP deltas between
samples. That cannot distinguish multiple hits in one frame, critical hits, elemental numbers, or which hunter
dealt damage—especially painful in **local multiplayer**, where clients may see delayed or batched HP updates.

Active mode requires reverse engineering the game's damage application function and a 3GX hook, which is
riskier and slower to validate but yields exact per-hit semantics.

## Decision

Implement **Modo Pasivo** first (RPC-only), then **Modo Activo** (plugin ring buffer readable via RPC).
Both produce the same **`DamageEvent`** type consumed by animation and rendering. The sampler **auto-selects**
active mode when it detects a valid `MHDN` shared block; otherwise it falls back to passive. HP deltas remain
as a cross-check in active mode.

## Alternatives considered

- Active-only: blocked on long RE timeline; no early user value.
- Passive-only forever: unacceptable for multiplayer attribution and crit/element styling goals.

## Consequences

- **Positive:** MVP (v0.1.0) unblocked; plugin work reuses the full overlay stack; passive remains a permanent fallback.
- **Negative:** Two data paths to test; passive limitations must be documented clearly for users.
- **Follow-ups:** F7 plugin spike (GO/NO-GO on RPC visibility of `0x07000000`); F2.17 host vs. client HP validation.
