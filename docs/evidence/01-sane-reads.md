# Phase 1: sane HP reads

Same solo session as phase 0: 7 quests, 2 logs, build `v0.4.0-dev+704950a`, windowed. The guards are `hp_bad` (impossible monster read), `dmg_drop` (HP delta larger than the bar), and `tap_unmatched` (tap whose object is not in the monster list).

## What the guards caught

One `hp_bad`, and nothing else.

`diag-1791139386.log`, t=2398.284: addr `0x300A7CE8`, species 4107, hp `4294967295` (`0xFFFFFFFF`), max_hp 168, reason `hp_above_max`. It landed 0.020 s after a scene / monster-list change. `diag-1791149888.log` has no `hp_bad`.

Across both logs: `dmg_drop` 0, `tap_unmatched` 0. No impossible read became a number on screen.

## List changes were not spikes

177 monster-list changes inside quests (136 + 41). 13 had a `dmg` within 1 s (10 + 3). Those hits were small and legitimate (max 23 and 26). `lost_tap` 0 in both logs.

## Small monsters still draw

Species 40xx still produce normal tap numbers. Kill shots keep the pre-clamp amount: 33 taps have `amount > hp_before` with `hp_after=0`, and two tiny-monster taps exceed max hp by a point or two (35 vs 33, 39 vs 38). Those are real hits. The recount counts `min(amount, hp_before)` when the previous HP is known, and still drops an amount above twice max hp, or above max hp when there is no previous HP.
