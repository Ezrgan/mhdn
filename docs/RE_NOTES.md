# Reverse Engineering Notes — MHXX / Azahar Combat Overlay

Log of reverse engineering discoveries, methodologies, memory addresses, and empirical live validations.
All memory offsets entered into `profiles/*.toml` must be validated by at least three independent sessions here.

---

## 1. RPC Benchmarking & Latency

Tool: `mhdn-probe bench-rpc`

**Azahar Prerequisites:**
- *Enable RPC server* enabled on `127.0.0.1:45987` (UDP).
- `log_filter = *:Info RPC_Server:Warning` configured in `qt-config.ini`.

**Execution:**
```bash
cargo run -p mhdn-probe -- bench-rpc --read-addr 0x00100000 --seconds 10
```

**Acceptance Targets:**
- 4-byte read p99 latency < 1.0 ms on localhost.
- Sustained throughput $\ge$ 2000 requests/sec with zero measurable frame drops in Azahar.

**Observed Metrics (macOS / Apple Silicon):**
- 4-byte read p50: 0.13 ms, p90: 0.25 ms, p99: 0.92 ms.

---

## 2. Memory Regions in Guest 3DS Address Space

`mhdn-probe dump` streams guest memory over the RPC and writes raw binary files.

```bash
cargo run -p mhdn-probe -- dump 0x08000000 0x09000000 dumps/heap.bin
```

| Region | Base Address | Contents |
|---|---|---|
| Code (`.text`) | `0x00100000` | Executable machine code and static constants |
| Global Data / BSS | `0x00C00000–0x00E00000` | Global state, frame counters, scene flags |
| Heap | `0x08000000` | Dynamic game entities (monsters, hunter, camera) |
| Linear Heap | `0x14000000` / `0x30000000` | GPU surfaces and entity transform buffers |

---

## 3. Monster List Offsets (`mhxx-jp-v1.4-es`)

Profile: `profiles/mhxx-jp-v1.4-es.toml`.

| Field | Address / Offset | Verification Status |
|---|---|---|
| Static Base | `0x00D3A8E0` (derefs to `0x082CE730`) | ✅ Confirmed across 3 reboots |
| Slot Stride | `+0x14`, stride 4, 16 slots | ✅ Confirmed |
| Entity Pointer Chain | `+0x10A8` then `+0x360` | ✅ Resolves to Monster HP anchor |
| Current / Max HP | `+0x0` / `+0x4` (u32) | ✅ Confirmed (Bulldrome, Velocidrome, Rathian) |
| Species ID | `+0x5A18` (u16) | ✅ Confirmed (Bulldrome=30, Velocidrome=14, Rathian=1) |
| Monster Size Multiplier | `-0x1B0` (-432, f32) | ✅ Confirmed (e.g. 1.03, 0.95) |
| Poison Timer | `+0x54E4` (u16) | ✅ Confirmed |
| Visibility Byte | `-0x1408` (-5128, u8) | ✅ Confirmed (hidden when `0x7`) |
| **Monster Center Position** | **HP − 0x320** (vec3 f32) | ✅ Confirmed in 3 sessions |
| **Guest Frame Counter** | **`0x082C4BB8`** (u32) | ✅ Confirmed (ticks at 60/s, freezes on in-game pause) |
| **In-Quest Flag** | **`0x08142FE0`** (u32 = 7) | ✅ Confirmed (7 in hunt, 0 in village/loading) |
| **Loading / Reward Flag** | **`0x00DC5814`** (u8 = 1) | ✅ Confirmed (1 on loading & reward screens) |

---

## 4. Live Sessions & Historical Evidence

### Session 1: Low-Rank Bulldrome (Species 30, Max HP 720)
- **Static Base:** Only `0x00D3A8E0` is valid on this build (holds `0x082CE730`). Candidates `0xD2CAA0` and `0xD30AA0` hold garbage.
- **Chain:** `0x00D3A8E0 + 0x14 + 0x10A8 + 0x360` $\rightarrow$ `0x300D3268` (HP).
- **Position (`HP - 0x320`):** Remained static for 3s, then moved smoothly along straight line downhill (Y dropped 380 $\rightarrow$ 292).
- **Frame Counter:** Direct u32 at `0x082C4BB8` advances strictly during active gameplay and freezes upon in-game menu pause.

### Session 2: Velocidrome Combat (Max HP 920)
- **Chain:** Resolved HP at `0x300E0E38`, slot 0 at 830/920 HP. Small mob slots located at slots 1–4.
- **Species ID:** +0x5A18 evaluated from the chain anchor yielded 14 (`em014` = Velocidrome).
- **Position:** Confirmed large displacement spikes (~608 units) during combat leaps.

### Session 3: Game Reboot & Rathian Arena (Max HP 2205)
- **Chain:** Resolved HP at `0x300E54A8`, reading 2194/2205 HP.
- **Species ID:** +0x5A18 read 1 (`em001` = Rathian), size multiplier 0.95.
- **Anchor Rule:** Struct offsets in `[monster]` are strictly relative to the pointer-chain result (HP address), not the inner object at `HP - 0x360`.

---

## 5. Camera & Player Coordinates

### Camera Controller Struct (`0x3003B3D0`)
- **Static Pointer:** `0x0814CACC` (and `0x08379E2C`) $\rightarrow$ points to camera base `0x3003B3D0`.
- **Camera Eye Position:** base `+0x40` (`vec3 f32` at `0x3003B410`).
- **Camera Target Position:** base `+0x60` (`vec3 f32` at `0x3003B430`).
- **Aspect Ratio:** `1.666667` ($400 / 240 = 5/3$, top 3DS screen).
- **Vertical Field of View:** Exact $50^\circ$ at base `+0x3C` (`0x3003B40C`), verified against projection matrix scale factor $2.1445 = 1/\tan(25^\circ)$.

### Local Hunter Feet Position
- **Static Slot:** `0x0814E620` $\rightarrow$ entity object base `+0x40` gives feet `(X, Y, Z)`.
- Hunter XZ matches the camera look-at target exactly, with Y offset by 170 units (look-at target is at chest/head height).

---

## 6. Dynamic Damage Tap Validation

### Hook Site Verification (`0x008D03E8`)
Inspected in guest memory on title version 4224 (Spanish update patch):
- `0x008D03E8`: `E593C0A8` (`ldr r12, [r3, #0xA8]`) — r12 = monster entity, r1 = signed HP delta.
- `0x008D03EC`: `E59C0360` (`ldr r0, [r12, #0x360]`) — loads current HP.
- `0x008D03FC`: `E58C0360` (`str r0, [r12, #0x360]`) — stores new HP.
- Code cave at `0x00BF2D00`: 768 bytes of contiguous zeros.
- Mailbox ring buffer at `0x00D32000`: 256 bytes of unallocated zero memory.

### Live Combat Verification
Installed Tap stub into code cave via single 4-byte atomic branch rewrite.
- Target Boss: 774 HP.
- Combat Result: Fight ran to 0 HP with **0 crashes**.
- Total Events: 126 events logged; boss took exactly 111 hits summing **775 damage** (774 HP + 1 overkill clamp on killing blow).
- Outcome: **GO** (100% stable, zero dropped hits).

---

## 7. Caller Hierarchy, Hitzone Transform, and Part Health

Analysis of the caller hierarchy leading to `lr = 0x008BA260` at `0x008D03E8`:

```arm
0x008BA248:  ldr  r2, [r4, #0xA8]    ; r2 = monster entity
0x008BA24C:  ldr  r1, [sp, #0xC]     ; r1 = raw positive damage
0x008BA254:  ldr  r2, [r2, #0x364]   ; r2 = max_hp
0x008BA258:  rsb  r1, r1, #0         ; r1 = 0 - r1 (negates damage)
0x008BA25C:  bl   0x008D03E4         ; call to damage application
0x008BA260:  ldr  r0, [r4, #0xA8]    ; return address (LR)
```

### Call Chain
1. `0x008EA9A8` (Attack / collision logic): calls `0x008ABF80`.
2. `0x008ABF80` (Monster damage dispatcher): calls `0x008AC038` (`bl 0x008B948C`).
3. `0x008B948C` (Damage calculation with VFP/NEON math): stores damage to `[sp, #0xC]` and calls `0x008BA25C` (`bl 0x008D03E4`).
4. `0x008D03E8` (HP write hook): intercepted by Tap stub.

### Decoded Stack Context at Hook (`sp[0..4]`)
Validated in a live 31-hit combat session:
- **`sp[1]` — Part Health / Stagger Pool:** Decrements exactly by damage dealt when hitting the same part:
  `142 -> 139 -> 136 -> 133 -> 130 -> 120 -> 116 -> 109 -> 106 -> 101`.
- **`sp[2]` — Struck Bone / Hitbox Object Pointer:** Pointer to C++ hitzone component (`vtable 0x0152A294`):
  - `+0x14` / `+0x18`: `prev` / `next` bone pointers in doubly-linked list.
  - `+0x28`: Bone identifier string (e.g. `BM_NOMIP`, Capcom MT Framework bone tag).
  - **`+0x40`: Struck bone 3D world position (`vec3 f32`)!**
  - `+0x60`: Bone scale `(1.2, 1.2, 1.2)`.
  - `+0x70`: Bone rotation quaternion.
- **`sp[3]` — Raw Damage:** Exact positive damage value matching `damage` 1:1.

> **Correction (2026-10-02, 166-hit live session, `mhdn-probe diag`):** `sp[2]` is *not* the struck bone.
> Its values match entries of the monster-list slot table (`0x082CE730 + 0x14`), and the same `sp[2]`
> appears for hits on different monsters. `sp[2] + 0x40` read `(120.3, 0, -0.1)` for three different
> targets in one area. The overlay now anchors tap hits on the monster position (`HP − 0x320`) plus the
> species height and ignores `sp[2]`.

## Tap callers

The hook at `0x008D03E8` stores `lr`, the return address of whoever applied the HP change. Seven solo quests showed three callers:

- `0x008BA260` (~97%): normal hits. `sp[3]` is the raw damage.
- `0x008BA870`: fixed damage when a mounted monster is toppled. The amount is always 150 in high rank or 100 in low rank. All 12 events of amount 100 or 150 came from this caller.
- `0x008BA214`: status damage. Poison ticks are amount 5 every 2.0 s exactly, with `sp[0] = 0` and `sp[3] = 0` (no raw damage word).

The overlay classifies the status caller as poison when the matched monster is already poisoned, and as generic status otherwise. The mount-topple caller is its own kind. Any other `lr`, including an unknown one, stays a normal hit.

Open:

- The status caller also emits a burst of ticks about every 0.5 s, faster than the 2.0 s poison cadence.
- Two 40-damage events on a small monster came from the status caller. That may be another status, or monster-vs-monster damage.

## 8. Scene Detection (correction)

Same session, 5 minutes of continuous hunting with the hunter moving every sample:
- `in_quest` (`0x08142FE0 == 7`) was true only in short bursts. Most of the hunt read 0.
- `loading` (`0x00DC5814 == 1`) was 1 for stretches of up to 11 s mid-hunt.
- The monster-list slot table is empty in the village and populated during the hunt.

The pipeline treats a resolved, non-empty monster list as the hunt. Leaving needs 30 consecutive
empty samples (about 0.5 s at 60 Hz). The two flags stay in the profile for `record` and further RE.


