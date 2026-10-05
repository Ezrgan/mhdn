# Hunt CPU

Log `diag-1791167563.log`. Build `v0.4.0-dev+5a1df47`. Exe `/Users/belkismartinez/Downloads/mhdn-test-5.app`. Open `unix=1791167563` (2026-10-04 22:32:43 -04:00). Same process as the village stretch in `docs/evidence/06-wakes-village.md`. No newer log covers 22:39.

One `azahar` line, t=0.779: `fullscreen=false`, window `1440x823`, `y=53`. `fullscreen` seconds in this file: 0.

Wall 22:39:00–22:42:59 -04:00 is t=377.699–616.621. `sec` n=236. Gap >1.5 s: 0.

| scene | sec n | wall |
| --- | ---: | --- |
| Village | 69 | 22:39:00–22:42:44 |
| Loading | 22 | 22:39:57–22:42:59 |
| InQuest | 145 | 22:40:05–22:42:32 |

InQuest t=442.016–589.639, dur=147.6 s. Windowed 147.6 s. Fullscreen 0.0 s. `dmg` n=32, all inside InQuest, first 22:40:27, last 22:42:25. `events` sum=32. Seconds with `events>0` or a `dmg` line: 25. Quest seconds with neither: 120. `tap_n` sum=0. `residual_n` sum=32. `tap_installed=0` on all 236 `sec`. Tap line at t=442.016: blocked. `lost_tap` 0. `hp_bad` 0. `dmg_drop` 0. `tap_unmatched` 0. `pumped=0` on all 145 quest `sec`.

Quest `sec` (n=145), then the 25 seconds with damage, then the 120 quest seconds without damage. Each cell is median / min / max.

| field | quest n=145 | with damage n=25 | quest, no damage n=120 |
| --- | ---: | ---: | ---: |
| guest_fps | 61 / 50 / 218 | 61 / 59 / 218 | 61 / 50 / 216 |
| wakes | 52 / 48 / 469 | 60 / 50 / 469 | 52 / 48 / 436 |
| redraws | 0 / 0 / 464 | 51 / 0 / 464 | 0 / 0 / 429 |
| snaps | 30 / 26 / 56 | 45 / 28 / 56 | 30 / 26 / 55 |
| pumped | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| alive | 0 / 0 / 3 | 1 / 0 / 3 | 0 / 0 / 0 |
| rpc_req_s | 840 / 686 / 1101 | 853 / 749 / 1019 | 837 / 686 / 1101 |

`guest_fps` in quest: 111/145 seconds are 59–63. 32/145 are >75 (median of those 32 = 204.5, min 78, max 218). Of those 32, 18 are within 2 s of a `scene` line and 25 are within 3 s. 7 of those 32 have damage. On all 32, `wakes`≤61 and `redraws`≤61. The log has no acceleration field.

`redraws=0` on 97/145 quest seconds (95/120 without damage, 2/25 with damage). `wakes`≤61 on 128/145. Seconds with `wakes`>100 or `redraws`>100: 16, none within 2 s of a `scene` line, 7 of them with damage.

Minute 22:41, all InQuest, n=59. `guest_fps` median 61, min 59, max 63. With damage n=8: `wakes` median 213.5 (min 52, max 469), `redraws` median 185.5 (min 0, max 464), `snaps` median 43.5, `rpc_req_s` median 842.5, `guest_fps` median 60 (min 59, max 62). Without damage n=51: `wakes` median 52 (min 49, max 436), `redraws` median 0 (min 0, max 429), `snaps` median 30, `rpc_req_s` median 846, `guest_fps` median 61 (min 59, max 63). `pumped` 0.

Village `sec` before the first Loading (t=377–433.996, n=56): `wakes` median 52 (min 48, max 54), `redraws` median 0 (min 0, max 3), `snaps` median 4, `pumped` 0, `rpc_req_s` median 39 (min 38, max 67), `guest_fps` median 60 (min 55, max 205). `guest_fps`>70 on 11/56, eight of them 22:39:49–22:39:56 (165–205), just before Loading at 22:39:56.

Loading `sec` in the window, n=22: `guest_fps` median 0 (15 seconds at 0, max 270), `wakes` median 50 (min 48, max 54), `redraws` median 0, `snaps` median 4, `pumped` 0, `rpc_req_s` median 36 (min 33, max 45). `dmg` n=0.

CleanMyMac on this process during this hunt, as read by the user: about 8–23.9%. While hitting, 15–23%. In quest with few damage events, 8–15%. That percentage is not in the log. Scale of one core is not in the log.

F6 written criterion: CPU of mhdn in a hunt ≤ 15% of one core, and `guest_fps` the same with and without mhdn. Measured while hitting: 15–23% (user, CleanMyMac). Quest seconds with little damage: 8–15% (user). `guest_fps` in the 22:40–22:42 quest window: median 61, max 218. The log does not show the game losing pace. Fullscreen seconds in this file: 0. The only `azahar` line has `fullscreen=false`. User observation, separate from this log: in fullscreen the fps looks the same with and without the app, and the user always plays at 60. Owner decision 2026-10-04: close F6. The written 15% cap is recorded as exceeded while hitting (15–23%), with fps intact.
