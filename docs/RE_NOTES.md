# RE notes — MHXX / Azahar overlay

Bitácora de reverse engineering: métodos, evidencias y validaciones.
Los offsets que entren en `profiles/*.toml` deben tener entrada aquí.

---

## RPC bench

Herramienta: `mhdn-probe bench-rpc` (Fase 1.6).

**Requisitos de Azahar antes de medir:**

- *Enable RPC server* activo (`127.0.0.1:45987`).
- `log_filter = *:Info RPC_Server:Warning` en `qt-config.ini` (reduce spam de log por paquete).

**Comando:**

```bash
cargo run -p mhdn-probe -- bench-rpc --read-addr 0x00100000 --seconds 10
```

**Criterio de aceptación (PLAN § F1.6):**

| Métrica | Objetivo |
|---|---|
| p99 lectura 4 B en localhost | < 1 ms |
| 2500 req/s sostenidos 10 s | Sin caída visible de FPS en Azahar (comparar barra de título / contador de FPS) |

**Resultados (rellenar en tu máquina):**

| Fecha | SO / CPU | p50 4 B | p99 4 B | p99 1 KiB | p99 batch 32×4 B | req/s 10 s | FPS Azahar antes | FPS durante |
|---|---|---|---|---|---|---|---|---|
| _pendiente_ | macOS | — | — | — | — | — | — | — |

---

## Memory dumps

`mhdn-probe dump` reads a guest range through the RPC and writes raw bytes (no header).
Pass the start address back in as the base when loading the file.

```bash
cargo run -p mhdn-probe -- dump 0x08000000 0x09000000 dumps/heap.bin
```

Typical guest regions:

| Region | Start |
|---|---|
| code (`.text`) | `0x00100000` |
| heap | `0x08000000` |
| linear heap | `0x14000000` or `0x30000000` |

RPC dumps are reliable and slow. For a large region, the GDB stub is faster
(enable it only while doing RE; default port **24689**):

```bash
arm-none-eabi-gdb -batch \
  -ex "target remote :24689" \
  -ex "dump memory dumps/heap.bin 0x08000000 0x09000000" \
  -ex "detach" \
  -ex "quit"
```

The file is the same flat image: guest address = base + file offset.
Do not commit `*.bin` or `dumps/`.

---

## Value scanner

`mhdn-probe scan` is a Cheat Engine style search over a guest range. The candidate
list lives in `scan.mhscan` (gitignored). Float compares use an absolute epsilon of `1e-4`.

```bash
# HP drops on hit. Repeat `next lt` or `next dec` after each hit.
cargo run -p mhdn-probe -- scan new u32 unknown --range 0x08000000-0x0A000000
cargo run -p mhdn-probe -- scan next dec
cargo run -p mhdn-probe -- scan list

# Position: walk forward, then stand still.
cargo run -p mhdn-probe -- scan new f32 unknown --range 0x08000000-0x0A000000
cargo run -p mhdn-probe -- scan next inc
cargo run -p mhdn-probe -- scan next unchanged
```

A first `unknown` pass stores every aligned value in the range. Narrow `--range`
before doing that on a full heap.

---

## Monster list offsets (mhxx-jp-v1.4-es)

Profile: `profiles/mhxx-jp-v1.4-es.toml`.

The static candidates and the in-struct offsets below are **ported from the GPLv3
reference** *MH-HP-Overlay-For-3DS-Emulator* (`modules/mhxx.py`), not yet confirmed
on this machine's build (MHXX JP + Spanish patch, title version 4224). A translation
patch can move static data even when struct layouts stay put. Do not treat them as
validated until `ptrverify` succeeds on three large monsters, three small monsters,
three quests, and after a game restart.

| Field | Candidate |
|---|---|
| static bases | `0x00D2CAA0`, `0x00D30AA0`, `0x00D3A8E0` (first whose value is in `expect` wins) |
| slot | `+0x14`, stride 4, 16 slots |
| chain | `+0x10A8` then `+0x360` |
| HP / max HP | `+0x0` / `+0x4` u32 |
| species | `+0x5A18` u16 |
| size | `-0x1B0` f32 |
| poison | `+0x54E4` u16 |
| hidden flag | `-0x1408` u8, hidden when `0x7` |
| position | **HP − 0x320** vec3 f32 (confirmed sessions 1–3; profile `off = -800`) |
| frame counter, scene, hunter, camera | unresolved (`TBD`; vblank tick `0x00D4BD8C` documented above, not in profile yet) |

```bash
cargo run -p mhdn-probe -- peek u32 0x00D2CAA0
cargo run -p mhdn-probe -- ptrverify 0x00D2CAA0+0x14+0x10A8+0x360
```

### Live session 1 — 2026-09-28, low-rank Bulldrome (species 30, max HP 720)

Confirmation **1 of 3**. The profile is not updated until two more sessions agree.

| Finding | Evidence |
|---|---|
| Static base | Only `0x00D3A8E0` is valid on this build: it holds `0x082CE730`, which is **not** in the profile's `expect` list (`0x082D0760`). `0x00D2CAA0` = `0x5E605E61` and `0x00D30AA0` = `0xFFE40194` are garbage here. |
| Chain | `0x00D3A8E0 +0x14 +0x10A8 +0x360` ⇒ `0x300D3268` (HP). Slot 0 is the fought monster; slots 1–2 had HP 0 / max 58; empty from slot 3. |
| In-struct offsets | HP (+0x0), max HP (+0x4), species (+0x5A18 = 30), size (−0x1B0 = 1.03) all plausible ⇒ the Spanish patch kept the struct layout. Visibility byte (−0x1408) = 15 while visible. |
| **Monster position** | **vec3 f32 at HP − 0x320** (`0x300D2F48`). Stood still ~3 s (unchanged to 0.1), then moved ~1000 units in 2 s along a straight line while Y went 380 → 292 (walking downhill). **HP − 0x360** is the same vector exactly one game frame behind (previous position). HP − 0x428 mirrors the current value (probably a transform copy). Found with `dumps/sample_obj.py` + `dumps/analyze_vec3.py` (40 samples of the whole object, then 60 samples of the candidates). |
| Frame counter candidates | `0x00D4BD8C` and `0x00D4BF5C`, u32, always equal, **+60/s** while playing. `scan new u32 unknown` over `0x00C00000–0x00D80000` then 3× `scan next inc`. **In-quest pause** (Start → opción pausar): counter **still +~59/s** for 4 s (80940→81176); treat as **display/vblank tick**, not “game logic frame”. Usable for timing/jitter, not for detecting pause. Still to check: same address after leaving quest / restart. |
| RPC latency | 4-byte read p50 0.13 ms, p90 0.25 ms, max 0.92 ms (Python client, `log_filter` still `*:Info`). |

### 2.18 hook site check — 2026-09-28 (same session)

**GO.** The Spanish patch did not touch the damage function.

| Address | Read | Expected (v1.4 cheat) | Instruction |
|---|---|---|---|
| `0x008D03E8` | `E593C0A8` | `E593C0A8` | `ldr r12,[r3,#0xA8]` (r12 = monster; r1 = signed HP delta) |
| `0x008D03EC` | `E59C0360` | `E59C0360` | `ldr r0,[r12,#0x360]` (current HP) |
| `0x008D03F0` | `E1500002` | — | `cmp r0,r2` |
| `0x008D03F4` | `C1A02000` | — | `movgt r2,r0` |
| `0x008D03F8` | `E0900001` | — | `adds r0,r0,r1` (HP += delta) |
| `0x008D03FC` | `E58C0360` | `E58C0360` | `str r0,[r12,#0x360]` (write HP) |
| `0x008D0400` | `E59310A8` | — | `ldr r1,[r3,#0xA8]` |
| `0x008D0404` | `43A00000` | — | `movmi r0,#0` |
| `0x008D0408` | `45810360` | — | `strmi r0,[r1,#0x360]` (clamp HP at 0) |

- Code cave `0x00BF2D00–0x00BF3000`: all 768 bytes zero.
- `.bss` mailbox `0x00D32000–0x00D32100`: all zero for **10 min** (1173 RPC samples every 0.5 s, 0 nonzero) while playing the Bulldrome quest ⇒ safe for Tap ring buffer (plan §2.20).

### Live session 2 — 2026-09-28, in-quest boss fight (max HP 920)

Confirmation **2 of 3**.

| Finding | Evidence |
|---|---|
| Static base | Unchanged: only `0x00D3A8E0` = `0x082CE730`; other two bases still garbage. |
| Chain | Same path ⇒ HP `0x300E0E38`, slot 0 **830/920**; slots 1–4 look like small mobs (26/26, 46/49, …). |
| Monster position | **HP − 0x320** again (`0x300E0B18`), large movement (avg ~67 units/sample, spikes ~608) during combat. |
| Species @ +0x5A18 | **14** = Velocidrome (`em014`) when read from the **chain anchor (HP address)**, as `mhdn-probe` does. Reading from inner object `p2` (HP − 0x360) at +0x5A18 wrongly gives 0 — same mistake as confusing anchors for session 1 notes. MHGU modding wiki: Bulldrome = 30, Velocidrome = 14. |

### Live session 3 — 2026-09-28, after **game restart**, Rathian room

Confirmation **3 of 3** ⇒ `profiles/mhxx-jp-v1.4-es.toml` updated (`expect` + `pos`).

| Finding | Evidence |
|---|---|
| Static base | Still only `0x00D3A8E0` = `0x082CE730`. |
| Chain | ⇒ HP `0x300E54A8`, **2194/2205**, species **1** (Rathian, `em001`), size 0.95. |
| Monster position | **HP − 0x320** (`0x300E5188`), coords plausible; motion avg ~3.6 (arena idle/walk). |

**Anchor rule:** offsets in `[monster]` are relative to the **pointer-chain result** (HP address), not the inner object at HP − 0x360.

Credits: the hook site comes from the public "Hit Monster Display Last Damage v1.4" cheat
(HeadPanda and reizamv gists; CTRPF-AR-CHEAT-CODES).

---

## Game version fingerprint

Checked on 2026-09-27 against the Azahar install on this machine.

| Source | Value |
|---|---|
| Base title | MHXX JP `0004000000197100` |
| Update title | `0004000E00197100` |
| TMD | `~/Library/Application Support/Azahar/sdmc/.../title/0004000e/00197100/content/00000001.tmd` |
| Title version at TMD `0x1DC` (u16 big-endian) | **4224** (`0x1080`) |

4224 is the v1.4 update (4160) plus 64, which matches the Spanish patch raising the version so it replaces the official update. There is no official untranslated v1.4 dump on this machine, so `.text` hashes cannot be compared against that build. `mhdn-probe game-info` hashes eight 4 KiB windows at `0x00100000 + k*0x40000` with xxh3. Those hashes are still `0` in the profile until a session with the RPC server fills them in. Until then, profile selection is provisional on the title id.

```bash
cargo run -p mhdn-probe -- game-info --profile profiles/mhxx-jp-v1.4-es.toml
```

---

## Live sessions still required

The probe can run these checks. The offsets are not in the profile yet, because they have to be found in a running game and confirmed more than once. Filling them in without that evidence would make the overlay read the wrong memory.

| Plan | What to do once Azahar is in a quest with the RPC server on |
|---|---|
| 2.18 hook site (do first) | `peek u32` at `0x008D03E8`, `0x008D03EC`, `0x008D03FC`; expect `E593C0A8`, `E59C0360`, `E58C0360` (from the public "Last Damage v1.4" cheat). `hexdump 0x00BF2D00 0x300` should be empty; `watch 0x00D32000 0x100` should stay zero for 10 min |
| 2.19 damage semantics | Enable the "Last Damage v1.4" cheat in Azahar, hit, and `watch 0x00D32004 4`; classify which damage types go through `0x8D03E8`. Disable the cheat afterwards |
| 2.20 damage tap | Needs RPC writes (2.20a). `tap install`, 10 min of Dual Blades, compare the event sum with HP lost, `tap uninstall` |
| 2.21 3GX loader | Minimal plugin with an `MHDN` block; read it over RPC at `0x07000000+`; restart the title once |
| 2.10 frame counter | `scan new u32 unknown`, then `scan next inc` until a value climbs by 1 at 30 Hz and freezes in a menu |
| 2.11 scene flags | Scan values that change only on village → loading → quest → village, plus the reference visibility byte |
| 2.12 hunter position | In the village, `scan new f32 unknown`, walk, then `inc` / `dec` / `unchanged`, and keep the contiguous `vec3` |

### Live session — 2026-09-28, village walk/stop loop (~2 min)

**Provisional only** (heap address; no static chain yet — do not add to profile).

| Finding | Evidence |
|---|---|
| Hunter world `vec3` | **`entity + 0x40`** (f32 XYZ). During this session the local player blob sat around **`0x300C0108`**, so position at **`0x300C0148`**. Same layout repeats every **0x140 bytes** in the `0x300C0xxx` cluster (NPCs / other actors). |
| Walk / stop | While the user walked forward/back on a 3 s / 3 s loop, **`0x300C0148`**: 174 samples with movement, 64 nearly still; coords moved in the XZ plane (village-scale values, e.g. Y ≈ 455–545). |
| Torn reads | Occasional 1000+ unit jumps on the same address (two buffers); prefer **median of 3 reads** or pin once a static chain exists. |
| Static pointer | No u32 in `0x00C00000–0x00EFFFFF` pointing at `0x300C0108` yet — needs `ptrscan` / deeper chain (2.12 continued). |

### Live session — 2026-09-28, quest camp, no palicos, 5 s walk / 5 s stop

**Provisional.** Heap address for this load only.

| Finding | Evidence |
|---|---|
| Hunter world `vec3` | **`0x30092460`** = `(x, y, z)` with **Y as height**. During the loop it reversed direction (span ~750). When the user stopped it **froze** at about `(1921, -18.6, 430)`. A second copy sits at **`+0x10`** (`0x30092470`). |
| Not the hunter | **`0x300638f0`** kept moving hundreds of units **after the user had stopped** (same session). Ignore it. |
| Nearby copies | Other frozen world `vec3`s in `0x30090xxx–0x30092xxx` (bones / attachments), tens to ~150 units from the root. |
| Static chain | No pointer in `0x00D00000–0x00E80000` or `0x08200000–0x08400000`. One heap link: `0x30091264 → 0x30092440` (position is that address **+0x20**). |

### Live session — 2026-09-28, quest camp, camera yaw (hunter still)

**Provisional.** Same quest load as the hunter position above. Hunter at `0x30092460` stayed at `(2043, -20.6, 469)` for the whole sample.

| Field | Address this load | Evidence |
|---|---|---|
| Camera eye | `0x3003b410` (struct `0x3003b3e0` **+0x30**, copy at **+0x60**) | World `vec3`. Distance to the hunter stayed ~550–630 while XZ swung (travel ~660 in well under a second of held turn). Y was the height of the boom (~125–227), not the hunter's feet. |
| Camera target | `0x3003b430` (same struct **+0x50**, copy at **+0x70**) | `vec3` about **190** units from the hunter feet, e.g. `(1949, 142, 486)` while feet were `(2043, -21, 469)` — look-at above the body, not the feet. |
| Unit vector | struct **+0x40** | Length ~1, e.g. `(-0.17, 0.99, 0)`. Not identified (up vs forward). |
| Other copies | `0x3003b8a0`, `0x3003b8d8`, `0x3003bb40`, `0x3003bd70` | Same eye `vec3` mirrored elsewhere. |

FOV not taken from the nearby floats (`1.67`, `50`) until a second angle confirms which one is `fov_y`. No static pointer yet, so `[camera]` stays `TBD`.

### Live session — 2026-09-28, camera/hunter address across a quest re-entry (load A)

Title had been rebooted. User standing still inside a quest. Monster list at `0x082CE730+0x14` was **null** (no slot-0 monster). Old hunter address `0x30092460` read `(0,0,0)`.

| Field | Address this load | Value |
|---|---|---|
| Camera eye | `0x3003B410` (struct `0x3003B3E0` +0x30, copy +0x60) | `(-958.9, 313.8, -1142.7)` |
| Camera target | `0x3003B430` (+0x50, copy +0x70) | `(-755.0, 228.8, -1580.0)` |
| Unit vector | struct +0x40 | `(0.073, 0.985, -0.157)`, length 1 |
| Hunter feet | `0x3004B950` (object `0x3004B910` +0x40) | `(-755.0, 58.8, -1580.0)` — same XZ as the look-at, **170** below it. Eye distance ~546 |

The camera struct came back at the **same address as the previous quest load** (`0x3003B410`), including the mirrors at `0x3003B8A0`, `0x3003B8D8`, `0x3003BB40`, `0x3003BD70`. The hunter did **not** (`0x30092460` → `0x3004B950`).

**Load B** — same quest, re-entered without rebooting the title. User still. Monster list still null.

| Field | Address | Load A | Load B |
|---|---|---|---|
| Camera eye | `0x3003B410` | `(-958.9, 313.8, -1142.7)` | `(-317.9, 274.1, -2300.5)` |
| Unit vector | `0x3003B420` | `(0.073, 0.985, -0.157)` | same to 1e-7 (default facing) |
| Camera target | `0x3003B430` | `(-755.0, 228.8, -1580.0)` | `(-114.0, 189.1, -2737.8)` |
| Hunter feet | `0x3004B950` | `(-755.0, 58.8, -1580.0)` | `(-114.0, 19.1, -2737.8)` |

Copies at eye+0x30 and target+0x20 still match. Hunter Y is again **170** below the look-at, same XZ. Quest re-entry kept both addresses and wrote new coordinates in place. A full title reboot had already moved the hunter (`0x30092460` → `0x3004B950`) while the camera address matched once. One reboot is not a static chain: do not put these addresses in the profile.

**Pointer candidates, this same load (not confirmed across a reboot).** Direct scan of `0x00C00000–0x00F00000` and `0x08000000–0x09000000`.

| Candidate | Points at | Then |
|---|---|---|
| `0x0814CACC` | `0x3003B3D0` | eye at **+0x40** (`0x3003B410`). Also `0x08379E2C` → the same object. No other static pointer hits the eye address itself. |
| `0x0814E620` | `0x3004B910` | feet at **+0x40** (`0x3004B950`). Sits in a repeating record (stride `0x18`) next to other heap objects, so it may be one entity slot rather than "the player". Many other static slots also point at `0x3004B910` (`0x082C47xx`, `0x0836Axxx`). |

Confirm both by rebooting the title, entering a quest, and resolving `+0x40` again before writing them into the profile.

**Reboot check 2 — 2026-09-28 (`lista`).**

| Static | This boot | +0x40 | Check |
|---|---|---|---|
| `0x0814CACC` (and `0x08379E2C`) | still `0x3003B3D0` | eye `0x3003B410` = `(-35.0, 286.2, -117.8)` | Unit vector at +0x50 has length 1. Target copy matches. |
| `0x0814E620` | **`0x300520A0`** (was `0x3004B910`) | feet `0x300520E0` = `(-124.3, 31.2, -592.0)` | Same XZ as camera target `(-124.3, 201.2, -592.0)`, Y exactly **170** below. Old feet address `0x3004B950` is garbage. |

**Reboot check 3 — 2026-09-28, different mission (`dentro`). Closed.**

| Static | This boot | +0x40 | Check |
|---|---|---|---|
| `0x0814CACC` (and `0x08379E2C`) | still `0x3003B3D0` | eye `0x3003B410` = `(1861.8, 210.3, 703.2)` | Unit `(-0.030, 0.985, -0.171)`, length 1. Copy at +0x70 matches. |
| `0x0814E620` | **`0x30055990`** (previous object `0x300520A0` is now 0) | feet `0x300559D0` = `(1778, -44.7, 228)` | Same XZ as target `(1778, 125.3, 228)`, Y exactly **170** below. |

Three sessions, two title reboots, the third in another mission. Both static slots resolved every time. The hunter heap object moved on every boot; the camera object was allocated at `0x3003B3D0` all three times. The profile schema only stores a relative `{ off, ty }`, not a static base, so `[hunter]` / `[camera]` stay `TBD` in the toml until that field exists. The addresses above are the confirmed ones.

**Aim vs zoom — 2026-09-28, same camera object.** User stood, then aimed with a bow or bowgun. Eye–target distance went from about **490** to **297** (the camera dollied in). Floats that did **not** move:

| Address | Value | Reading |
|---|---|---|
| `0x3003B408` (eye − 8) | `1.666667` | 400/240 = 5/3, top-screen aspect. Not FOV. |
| `0x3003B40C` (eye − 4) | `50` | Unchanged, so aiming did not prove it is `fov_y`. |
| `0x3003BDF8` / `0x3003BDFC` | `1.333333` / `40` | 4/3 and 40, same pattern on another block (bottom screen is 320×240). Also unchanged. |

A 1° change at `0x3003BD50` (`87.2` → `88.4`) tracks the small aim rotation, not the frustum. `fov_y` stays unconfirmed. A scope zoom (ballista/bowgun sight), if the weapon has one, is a different test from plain aiming.

**Bowgun scope — same session, camera object still `0x3003B3D0`.** Unaimed distance ~490. While the sight was held, `0x3003B408` (`1.666667`) and `0x3003B40C` (`50`) stayed put. New values in the same object:

| Address | Unaimed | Scope held |
|---|---|---|
| `0x3003B5C4` | `0` | **`20`** (exact). Still `20` on a second read while held. |
| `0x3003B5AC` | `0` | `12.68`, with XZ `(-538.7, -2662.8)` copied beside it (the pre-scope target XZ) |
| `0x3003B5CC` | `0` | `3.0` |
| `0x3003B5A0` | `0` | u32 `7` |

After release, eye–target distance returned to ~**490** and the target matched the pre-scope value `(-538.7, 182.7, -2662.8)`, but `0x3003B5C4` stayed **20**, `0x3003B5AC` stayed `12.68`, and `0x3003B5A0` stayed `7`. Those fields latch on. They are not a live frustum: the camera had already returned. The constant `50` at `0x3003B40C` never moved during aim or scope. That 16 KB window also has no projection matrix.

**Normal frustum, same session, wider heap scan (`0x30000000–0x30400000`).** Column-major matrix at `0x30057450`:

```
1.2867  0      0      0
0       2.1445 0      0
0       0     -1     -1
0       0    -16.0001 0
```

`2.1445 = 1/tan(25°)`, so vertical FOV is **50°**. `1.2867 = 2.1445 / (5/3)`, so horizontal FOV is **75.7°**, matching the `1.666667` aspect at `0x3003B408`. The `50` beside the eye is that same angle, not a coincidence. Scope held again, same session. The matrix did **not** change: `0x30057450` stayed `1.2867 / 2.1445` (50° vertical, 75.7° horizontal) and `0x3003B40C` stayed `50`. The eye and target moved to the scoped pose `(-524.3, 150.3, -2640.6)` / `(-541.4, 407.8, -3606.7)`, the same pose as the previous scope sample. The sight moves the camera and latches `20` at `0x3003B5C4`. It does not change the frustum. Normal and scoped projection both use **50°**.

### Live session — 2026-09-28, Last Damage cheat (plan 2.19), one boss

Cheat **Hit Monster Display Last Damage v1.4** enabled in Azahar (hooks `0x008D03EC`, not `0x008D03E8`). User landed about four hits. Mailbox read once after the flurry, so only the **last** hit is stored.

| Address | Value | Meaning |
|---|---|---|
| `0x008D03EC` | `0xEA0C8AFE` | Cheat branch installed (original word was `0xE59C0360`) |
| `0x008D03E8` | `0xE593C0A8` | Untouched by this cheat |
| `0x00D32004` | `0xFFFFFFF8` | Signed **−8**. The cheat stores `r1` (HP delta). Last hit = **8** damage |
| `0x00D32000`, `0x00D32008` | `0` | Not used by this cheat |

Same moment, slot 0 HP `0x30072518` = **1368 / 1419** (51 missing). That fits several hits with only the last one kept in the mailbox. Species u16 at HP`+0x5A18` = **19** on this load. Element, poison, bomb, Felyne, and LAN hits are not classified yet.

Unchecking the cheat does **not** restore the hook. After the box was cleared, `0x008D03EC` stayed `0xEA0C8AFE` and the cave at `0x00BF2FD4` stayed in place. The mailbox then moved on its own (`−8` → `−7` → `−18`) while the user was not attacking, and HP fell **1368 → 1332**. Those later writes are other HP changes through the same function, not extra button presses. A quest restart also leaves the patch: new monster at full HP (**1617/1617**, same chain address `0x30072518`) while `0x008D03EC` was still `0xEA0C8AFE` and `0x00D32004` still `−18`. Rebooting the title inside Azahar (Azahar itself left open) reloaded the code: `0x008D03EC` = `0xE59C0360`, cave `0x00BF2FD4`/`0x00BF2FFC` = 0, mailbox `0x00D32004` = 0. `0x00243830`/`0x00243A08`/`0x00243E8C` back at `E3500000`/`E7D00005`/`E3E00000`, the words the cheat writes on R+Down, so those are the originals. Do this reboot, with the cheat unchecked, before any Tap install.

| 2.13 monster position | `watch` ±0x2000 around the monster struct; the `vec3` moves when the monster walks and matches distance to the hunter |
| 2.14 camera | Rotate the camera and look for an eye on a sphere around the hunter, or `findmat` on two dumps |
| 2.15 stability | `ptrverify` on 5 cold boots × 3 quests × 2 zones, with and without a save state |
| 2.16 traces | `record --profile profiles/mhxx-jp-v1.4-es.toml --out tests/traces/<name>.mhrec --seconds 120` for the six sessions in the plan |
| 2.17 multiplayer | Record the same quest on the host and on a LAN client and compare HP timing |

Definition of done for those rows: each offset has an entry here with the method, the date, the game version, and at least three separate confirmations.

### Live session — 2026-09-28, damage tap GO (plan 2.20)

Title reboot, cheat absent. Before any hit: `0x008D03E8` = `E593C0A8`, `0x008D03EC` = `E59C0360`, `0x008D03FC` = `E58C0360`, cave `0x00BF2D00` = 0. Monster HP chain `0x00D3A8E0+0x14+0x10A8+0x360` → `0x300658B8` = **774/774**. Object at HP−0x360 = `0x30065558`.

`mhdn-probe tap install`, then `tap follow` (one process, one socket). The fight ran to the kill. HP ended at **0**. The ring produced **126** events, none lost. The boss accounts for **111** hits summing **775**. The extra 1 is the killing blow: the bar was at 5 and the hit was 6, and the store at `0x008D0408` clamps HP at 0. Other objects `0x3008BD08` and `0x300974A8` summed 63 and 61. `lr` was `0x008BA260` on 124 hits and `0x008BA870` on the two hits of 100. `tap uninstall` restored the original load. Fixture: `crates/mhdn-game/tests/fixtures/tap-boss-2026-09-28.txt`.

An earlier poll that spawned a new process every sample ran while the machine switched desktops and the Mac locked up; that log has no hits. The successful read is the single-process follow. The tap does not name the attacker. Frame counter, scene, traces, and LAN remain open.
