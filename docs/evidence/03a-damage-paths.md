# Phase 3a: damage kind by caller

One solo quest. Log `diag-1791161519.log`. Build `v0.4.0-dev+7ce1389`. `dmg` n=304. Quest duration 406.6 s (t=121.262–527.909). Large monster species 8, gen 1, max_hp 1634, mon `0x300E5B08`.

## Kind counts

| kind | source | n | sum | lr | amount |
| --- | --- | ---: | ---: | --- | --- |
| hit | tap | 240 | 1407 | `0x008BA260` | 1–20 |
| hit | passive | 1 | 10 | `0x00000000` | 10 |
| poison | tap | 61 | 305 | `0x008BA214` | 5 |
| topple | tap | 2 | 200 | `0x008BA870` | 100 |
| status | — | 0 | 0 | — | — |
| unknown | — | 0 | 0 | — | — |

Tap share: n=303 sum=1912. Residual: n=1 sum=10 (species 4097, t=267.852, confidence `hp_delta`). `lost_tap` 0. `hp_bad` 0. `dmg_drop` 0. `tap_unmatched` 0. `amount>max_hp` 0. `amount>hp_before` 6, all with `hp_after=0`.

Tap `lr` values: `0x008BA260` n=240, `0x008BA214` n=61, `0x008BA870` n=2. No other tap return address.

## Poison

All 61 ticks are species 8, mon `0x300E5B08`, amount 5, source tap, `lr` `0x008BA214`. Two bursts separated by 104.969 s:

| burst | n | t | span s | sum | mean gap s | min gap s | max gap s |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 40 | 286.452–325.453 | 39.001 | 200 | 1.0000 | 0.982 | 1.017 |
| 2 | 21 | 430.422–450.424 | 20.002 | 105 | 1.0001 | 0.983 | 1.034 |

`sp3` is `0x00000000` on 60 ticks. One tick at t=444.422 has `sp3` `0x00000003`, amount 5, same mon.

Measured fact: mean gap ~1.0 s. Earlier solo sessions measured ~2.0 s. This log does not identify the cause.

## Topple

Both events are species 8, mon `0x300E5B08`, source tap, `lr` `0x008BA870`, amount 100 (no 150 in this log):

| t | hp_before | hp_after | max_hp |
| --- | ---: | ---: | ---: |
| 271.335 | 1490 | 1390 | 1634 |
| 356.670 | 687 | 587 | 1634 |

## Fullscreen and guest_fps

Quest split: windowed 20.7 s, fullscreen 386.0 s. `fullscreen=true` from t=141.912 to t=530.049. In-quest `guest_fps` n=403, mean 59.6, mode 60. Samples above 1.5× mode: 2, both before fullscreen (t=121.973 value 110, t=134.012 value 203). Fullscreen in-quest samples n=383, mean 59.1, p5 59.0, min 0.
