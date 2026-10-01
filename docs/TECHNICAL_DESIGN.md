# Technical Design — Monster Hunter XX Combat Overlay for Azahar

> Architectural specification, memory model, and reverse engineering rationale.

---

## 1. Problem Statement

*Monster Hunter XX* (3DS, 2017) does not display damage feedback numbers. The objective of `mhdn` is to provide the modern visual feedback seen in *Monster Hunter World* and *Rise*:
- Damage numbers that pop dynamically at the exact point of impact (hitzone), float upward, and fade out smoothly.
- Visual distinction for critical hits, part breaks, poison ticks, and blast detonations.
- An in-hunt recount/DPS telemetry widget showing damage distribution across the party.

Rendering 3D floating damage on an external window on top of an emulator requires solving four decoupled problems:

1. **When & How Much Damage:** Intercepting each discrete damage event and its signed/unsigned magnitude.
2. **Where in the 3D World:** Obtaining the exact 3D coordinates $(X, Y, Z)$ of the struck bone/hitzone.
3. **Where on the Virtual Screen:** Projecting world coordinates into normalized device coordinates (NDC) using the guest camera parameters.
4. **Where on the Physical Desktop:** Mapping the 3DS top-screen viewport within Azahar's window to desktop screen coordinates.

---

## 2. Environment & Constraints

### 2.1 Emulated 3DS System
- **CPU:** ARM11 (ARMv6K), little-endian. Memory layout:
  - `0x00100000`: Static process image (`.text`, `.rodata`, `.data`, `.bss`). Addresses remain constant for a given build version.
  - `0x08000000`: Application heap (dynamic entities: monsters, players, camera controller).
  - `0x14000000` / `0x30000000`: Linear heap (GPU buffers and entity transform arrays).
- **GPU:** PICA200. Physical display layout: top screen is $400 \times 240$ (aspect ratio $5:3$).
- **Engine:** Capcom MT Framework Mobile, running at 30 FPS logic ticks (or 60 FPS with patch).

### 2.2 Azahar Emulator
- **Memory RPC Server:** Exposes memory read/write via UDP on `127.0.0.1:45987`. Packets feature a 16-byte binary header (`version, id, type, size`) and up to 1024 bytes payload. Reads are handled asynchronously relative to emulation CPU execution.
- **Log Filtering:** The RPC server emits a `LOG_INFO` for every packet by default. Configuring `log_filter = *:Info RPC_Server:Warning` prevents CPU saturation.
- **Layout Math:** Emulates layout presets (`SeparateWindows`, `LargeScreen`, `Default`) defined in `qt-config.ini`.

---

## 3. Architecture Strategy

### 3.1 Evaluated Alternatives

| Approach | Architecture | Advantages | Drawbacks | Verdict |
|---|---|---|---|---|
| **A. External Memory Polling** | Read HP through UDP RPC, calculate $\Delta\text{HP}$ | Zero game modification; completely external | Multi-hits in the same frame collapse into 1 number; no hitzone point | **Fallback Mode** |
| **B. In-Memory ARM Tap** | Patch 1 instruction at `0x008D03E8` to an ARM cave stub; write events to free `.bss` | Exact per-hit events, bone transforms, zero host injection, hot-installable | Requires static code cave and free RAM buffer | **Selected Primary Engine** |
| **C. 3GX Plugin Loader** | C++ CTRPluginFramework plugin in guest RAM | Full hook capability | Unstable across certain Azahar builds and platforms | Optional enhancement |
| **D. Emulator Fork** | Fork Azahar to render internally with ImGui | Direct memory & uniform access | Massive maintenance overhead; breaks portability | Discarded |

### 3.2 Decision: The Dynamic Tap Architecture

We employ **Approach B (The Damage Tap)**.
1. When entering a quest, `mhdn` inspects `0x008D03E8`. It verifies that the original instruction is `ldr r12, [r3, #0xA8]`.
2. It writes a lightweight 40-byte ARM stub into an unused code cave (`0x00BF2D00`) and overwrites `0x008D03E8` with an atomic branch (`B 0x00BF2D00`).
3. Whenever damage is dealt, the stub runs the displaced load, copies `{seq, r1, r12, r3, lr, sp[0..4]}` to a ring buffer in unused `.bss` (`0x00D32000`), applies a memory barrier, publishes `write_seq`, and returns to `0x008D03EC`.
4. The host `mhdn` process polls `write_seq` at 60 Hz and consumes newly emitted events.
5. On shutdown or quest exit, the original instruction is restored cleanly.

---

## 4. Pipeline & Data Flow

```
Combat Impact
     │
     ▼
[0x008EA9A8] Collision / Attack Logic
     │
     ▼
[0x008ABF80] Monster Damage Dispatcher
     │
     ▼
[0x008B948C] Damage Calculation (VFP/NEON math)
     │   Loads raw damage from [sp, #0xC]
     │   Negates damage with `rsb r1, r1, #0`
     ▼
[0x008D03E8] Life Reduction Hook  ──▶  ARM Tap Stub
                                            │
                                            ▼
                                  Ring Buffer @ 0x00D32000
                                            │
                                            ▼ (UDP RPC :45987)
                                  mhdn Sampler Loop (Rust)
                                            │
                                            ├──▶ 3D Projection (glam)
                                            │         │
                                            │         ▼
                                            │    Floating Text Particles
                                            │
                                            └──▶ Recount / Telemetry Widget
```

### 4.1 Captured Event Structure
Each event captures the exact state of the registers and stack:
- `damage`: Absolute damage integer.
- `monster`: Target entity address in guest RAM.
- `sp[1]`: Part stagger/break health pool (decrements per hit).
- `sp[2]`: Struck bone transform component (`BM_...` node).
- `sp[2] + 0x40`: World 3D coordinates `(X, Y, Z)` of the impacted bone.

---

## 5. 3D-to-2D Projection Geometry

1. **View Transform:** 
   $$V = \text{lookAt}(eye, target, up = (0, 1, 0))$$
2. **Projection Transform:** 
   $$P = \text{perspective}(fov_y = 50^\circ, aspect = \frac{5}{3}, near, far)$$
3. **Clip & NDC:** 
   $$c = P \cdot V \cdot [P_{world}, 1]^T$$
   $$n = \frac{c_{xy}}{c_w} \in [-1, 1]^2 \quad (\text{discard if } c_w \le 0)$$
4. **Virtual 3DS Screen:**
   $$x_{3ds} = \frac{n_x + 1}{2} \cdot 400, \quad y_{3ds} = \frac{1 - n_y}{2} \cdot 240$$
5. **Desktop Host Mapping:**
   $$X = Rect_{top}.x + x_{3ds} \cdot \frac{Rect_{top}.w}{400}$$
   $$Y = Rect_{top}.y + y_{3ds} \cdot \frac{Rect_{top}.h}{240}$$

---

## 6. Visual Design & Presentation

| Hit Category | Color | Scale | Behavior |
|---|---|---|---|
| **Standard Hit** | Crisp White | 1.0× | Quick pop (90ms) $\rightarrow$ upward float $\rightarrow$ fade out |
| **High Damage Hit** | Warm Orange | 1.25× | Enhanced spring easing pop |
| **Critical Hit** | Bright Yellow | 1.35× | Bold "!" suffix, enhanced pop |
| **Poison DoT Tick** | Vivid Purple | 0.85× | Gentle float, no pop |
| **Blast / Nitro Detonation** | Explosive Crimson | 1.4× | Flash burst popup |

### Combat Recount Widget
A semi-transparent 2D HUD pinned to a configurable corner of the screen:
- Individual hunter damage contribution bar and percentage.
- Real-time hunt DPS.
- Status triggers counter (Poison / Blast procs).

---

## 7. Technology Stack

- **Core Engine:** Rust stable (multi-crate workspace).
- **GPU Rendering:** `wgpu` (Metal on macOS, DX12/Vulkan on Windows).
- **Window Management:** `winit` with platform-specific Cocoa / Win32 bindings.
- **Math:** `glam` (SIMD-accelerated linear algebra).
- **Text & Glyphs:** Vector glyph atlas caching.
- **Memory Protocol:** Custom non-blocking UDP client (`mhdn-rpc`).

