# Village wakes

Log `diag-1791166272.log`. Build `v0.4.0-dev+49b11d6`. Open `unix=1791166272` (2026-10-04 22:11:12 -04:00). Scene `Village` at t=0.880. `fullscreen=false` at t=0.880. Quests 0. `dmg` n=0. `lost_tap` 0. `hp_bad` 0. `dmg_drop` 0. `tap_unmatched` 0.

Wall 22:09:00–22:10:51 -04:00 is log `diag-1791165270.log`, build `v0.4.0-dev+7ce1389`, open `unix=1791165270`. Last `sec` t=981.924 (22:10:51). Scene `Village` from t=486.459. `redraws=0`. No `wakes` field. This log does not cover 22:10:52–22:11:11.

`sec` lines of `diag-1791166272.log` with wall 22:11:13–22:11:59 (n=47), scene `Village`, `fullscreen=false`:

| field | median | min | max |
| --- | ---: | ---: | ---: |
| wakes | 60 | 22 | 61 |
| redraws | 0 | 0 | 6 |
| snaps | 4 | 3 | 4 |
| pumped | 0 | 0 | 0 |
| rpc_req_s | 48 | 47 | 83 |
| guest_fps | 60 | 30 | 61 |

The min row is one `sec` at t=1.232: `wakes=22` `redraws=6` `snaps=3` `pumped=0` `rpc_req_s=68` `guest_fps=30`. The other 46 `sec`: `wakes` median 60, min 56, max 61; `redraws=0`; `snaps=4`; `pumped=0`.

`wakes` histogram, n=47: 22×1, 56×1, 57×2, 58×4, 59×15, 60×11, 61×13.

A 33 ms wait is 30 wakes/s. Measured median 60 wakes/s.

Snapshot through t=220.250 (22:14:52), still scene `Village`, `fullscreen=false`. `sec` with `redraws=0` n=217: `wakes` median 60, min 54, max 61; `snaps` median 4; `pumped` 0; `rpc_req_s` median 48; `guest_fps` median 60.

Wall 22:17 is this same log. Only scene line in the file is `Village` at t=0.880. `sec` with t=300–400 (n=99, about 22:16:12–22:17:52): `wakes` median 60, min 54, max 61; `redraws` 0; `alive` 0; `snaps` median 4; `pumped` 0; `rpc_req_s` median 48; `guest_fps` median 60. CleanMyMac on this process at 22:17: about 19–21% (peak noted 22.7% at 22:10, same app after the previous process was quit). Scale of that percentage is not in the log.

Log `diag-1791167563.log`. Build `v0.4.0-dev+5a1df47`. Exe `/Users/belkismartinez/Downloads/mhdn-test-5.app`. Open `unix=1791167563` (2026-10-04 22:32:43 -04:00). Scene `Village` at t=0.779. `fullscreen=false` at t=0.779. `parked=true` and `behind=true` from t=0.838. `settings_focused` true at t=0.838, false at t=3.426. Quests 0. Tap not installed at t=0.779. `lost_tap` 0. `hp_bad` 0. `dmg_drop` 0. `tap_unmatched` 0.

`sec` lines with wall 22:33:00–22:34:59 -04:00 (n=118), scene `Village`, `fullscreen=false`, `parked=true`, `behind=true`:

| field | median | min | max |
| --- | ---: | ---: | ---: |
| wakes | 50 | 48 | 54 |
| redraws | 0 | 0 | 0 |
| alive | 0 | 0 | 0 |
| pumped | 0 | 0 | 0 |
| snaps | 4 | 4 | 5 |
| guest_fps | 60 | 55 | 75 |
| rpc_req_s | 39 | 39 | 75 |

`wakes` histogram, n=118: 48×5, 49×8, 50×60, 51×6, 52×35, 53×2, 54×2. `redraws` histogram: 0×118. Seconds with `wakes`≥56: 0. Seconds with `wakes`≤35: 0.

A 33 ms wait is 30 wakes/s. Measured median in this window is 50 wakes/s.

Village stretch of this process, snapshot through t=261.755 (22:37:04 -04:00). The log was still open. Only scene line is `Village` at t=0.779. `fullscreen=false`. `parked=true` and `behind=true` on every `sec` (n=257). `wakes` median 50, min 47, max 80; `redraws` median 0, min 0, max 6; `alive` 0; `pumped` 0; `snaps` median 4, min 3, max 5; `guest_fps` median 60, min 30, max 76; `rpc_req_s` median 40, min 38, max 75.

`wakes` histogram, n=257: 47×1, 48×12, 49×17, 50×145, 51×13, 52×60, 53×5, 54×2, 62×1, 80×1. The two `wakes`≥56 are t=3.283 (`wakes=80`) and t=4.303 (`wakes=62`). The `redraws=6` is t=1.262 (`wakes=50` `snaps=3` `guest_fps=30` `rpc_req_s=58`). `sec` with t≥5 (n=253): `wakes` median 50, min 47, max 54; `redraws` 0; `alive` 0; `pumped` 0; `snaps` median 4, min 4, max 5; `guest_fps` median 60, min 46, max 76. Seconds with `wakes`≥56: 0. Seconds with `wakes`≤35: 0.

CleanMyMac on this process at 22:34: about 5% (user). Scale of that percentage is not in the log.
