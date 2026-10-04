# Phase 0: diagnostic traces

Solo session, 7 quests, 2 logs, build `v0.4.0-dev+704950a`. Windowed the whole time except 4.6 s of fullscreen in the village. `tools/diag_summary.py` parsed both logs.

## What was logged

- `dmg`: one line per damage event (source, confidence, amount, monster, `hp_before` / `hp_after`, frames, `lr`, and for taps `r3` plus `sp`).
- `scene`: scene name and the monster list (slot, address, species, hp, max hp, generation) whenever that list changes.
- `sec`: once a second, `guest_fps`, `tap_n` / `tap_sum`, `residual_n` / `residual_sum`, `lost_tap`, `rpc_req_s`, `pumped`, `redraws`.
- Wide tap words (`sp[5..15]`) only when `MHDN_TAP_WIDE=1`.

## diag-1791139386.log

3 quests: species 1 at max hp 5310; species 3 at 4365 and 4095; plus small monsters.

| source | confidence | n | sum |
| --- | --- | ---: | ---: |
| tap | exact | 1004 | 8248 |
| passive | hp_delta | 4 | 33 |
| passive | aggregated | 6 | 63 |

Tap share of damage per quest: 99.5%, 100%, 95.9%. `hp_bad` n=1 at t=2398.284, addr `0x300A7CE8`, species 4107, hp `4294967295`, max_hp 168, reason `hp_above_max`, 0.020 s after a scene / monster-list change. `dmg_drop` 0, `tap_unmatched` 0. 136 monster-list changes inside quests; 10 had a `dmg` within 1 s, all small hits (max 23). `lost_tap` 0. Every passive amount was in 1–49.

## diag-1791149888.log

4 quests: species 61 at max hp 3492 and 3708 (three of the latter); species 32 at 2900.

| source | confidence | n | sum |
| --- | --- | ---: | ---: |
| tap | exact | 1593 | 15661 |
| passive | hp_delta | 21 | 185 |
| passive | aggregated | 1 | 17 |

Tap share per quest: 99.1%, 99.0%, 99.2%, 97.6%. `hp_bad` 0, `dmg_drop` 0, `tap_unmatched` 0. 41 list changes, 3 with a `dmg` within 1 s (max 26). `lost_tap` 0.

## Both logs

Kill shots record pre-clamp damage. 33 tap events have `amount > hp_before` with `hp_after=0`. Two exceed max hp on tiny monsters (35 vs max 33; 39 vs max 38). Small monsters (species 40xx) still get normal tap numbers.

Tap words: `lr` is `0x008BA260` about 97% of the time, `0x008BA214` about 3%, `0x008BA870` rare. `r3 == mon-0x418` in 100% of taps. `sp4 == mon+0x100` in 100%. `sp3 == amount` in about 96%. `sp0` is mostly 0, `sp1` is part of the pool, `sp2` is a heap pointer or a small int. `sp[0..4]` and `r3` are target-side; the attacker is not among them.

Performance, quest samples, windowed only: `rpc_req_s` mean about 950–966, `pumped` 0, redraws mean 9–22/s. `guest_fps` mode is 60, but the series is noisy (mean 70–103, many samples at 0 and above 90). The frame counter is not a clean fps measure; treat `guest_fps` as a rough counter delta, not a frame-time.

The `dmg`, `scene`, and `sec` lines are complete and parseable.
