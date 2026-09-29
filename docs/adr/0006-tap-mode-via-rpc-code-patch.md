# ADR-0006: Damage tap via an RPC code patch

## Status

Accepted for the patch mechanism (2026-09-28). Live GO/NO-GO of the installed stub, and the 3GX loader spike, are not done.

Extends [ADR-0003](0003-two-tier-passive-active.md).

## Context

The Spanish MHXX v1.4 build keeps the public "Last Damage" hook. `0x008D03E8` is `ldr r12, [r3, #0xA8]`, `r1` is the signed HP delta, and `0x008D03FC` stores the new HP at `monster+0x360`. The cave `0x00BF2D00–0x00BF3000` was empty, and `0x00D32000–0x00D32100` stayed zero for 10 minutes of play. One real hit wrote `r1 = -8` at `0x00D32004` while the cheat was installed. Element, poison, bomb, Felyne, and another player's hits were not classified.

Azahar's RPC `WriteMemory` accepts a start address in the process image, the heap, the linear heap, or the New 3DS extra RAM, and it answers with an empty packet whether or not the write landed. The New 3DS alias at `0x30000000` is not in that list. The hook, the cave, and the ring are all inside the process image, so the tap can be installed over RPC.

A 3GX plugin would still be the path to attacker, crit, and element. Azahar issues #1381, #1125, and #2569 are open risks for the loader. That spike was not run on this machine.

## Decision

Ship a third source, **Modo Tap**, between passive HP deltas and the plugin:

1. The overlay (or `mhdn-probe tap`) writes an ARM stub into `0x00BF2D00`, then one branch at `0x008D03E8`.
2. The stub records `{seq, r1, r12, r3, lr, sp[0..4]}` in a 64×40 byte ring at `0x00D32000` and publishes `write_seq` at `0x00D32A00` after a CP15 barrier.
3. Install refuses if `0x008D03E8` is not the original load or our own branch, or if `0x008D03EC` / `0x008D03FC` are not the original words. That rejects the Last Damage cheat, which uses the same cave and hooks `0x008D03EC`.
4. Uninstall restores the original word at `0x008D03E8`.
5. Passive HP deltas stay as the fallback and as the cross-check. Priority remains Plugin > Tap > Passive.

The 3GX loader is **not** a GO and **not** a NO-GO. It was not executed. Phase 3 can consume passive deltas and, once a live session says the stub is stable, the tap ring. It must not depend on the plugin loader.

## Consequences

- **Positive:** Exact per-hit values do not require the plugin loader. The stub is data in the repo, and its branches are checked by a unit test.
- **Negative:** The live bar in plan 2.20 (10 minutes of Dual Blades, `Σ events == HP lost`, no crash) is still open. A bad stub can crash the guest until the title reboots. `fov`, hunter, and camera pointers are known; frame counter, scene flags, traces, and LAN are not, so the phase 2 definition of done is not met.
- **Follow-ups:** Run `mhdn-probe tap install`, fight, `tap events`, `tap uninstall`. Run the 2.21 loader spike before any plugin work. Do not install the tap while the Last Damage cheat is in memory; reboot the title after that cheat.
