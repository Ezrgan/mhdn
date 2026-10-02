# mhdn — Monster Hunter XX Damage & Combat Overlay for Azahar

[![CI](https://github.com/Ezrgan/mhdn/actions/workflows/ci.yml/badge.svg)](https://github.com/Ezrgan/mhdn/actions/workflows/ci.yml)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

An external, transparent, and non-intrusive floating damage numbers and combat telemetry overlay for **Monster Hunter XX** (3DS) running on the [Azahar](https://github.com/azahar-emu/azahar) emulator (Citra fork).

`mhdn` projects Monster Hunter World–style floating damage numbers in 3D space directly above monster hitzones and provides an in-game hunt breakdown (DPS / total damage contribution meter), rendered at your monitor's native resolution.

---

## Highlights & Features

- **Exact Per-Hit Detection:** Utilizes an in-memory dynamic ARM tap (patching `0x008D03E8`) to capture single-frame hits accurately—no aggregated damage or missed multi-hits from fast weapons like Dual Blades.
- **Accurate Hitzone Origins:** Extracts the exact bone transform matrix (`vec3` world position) of the struck body part so numbers spawn from the actual point of impact (head, tail, legs) rather than a generic center of mass.
- **Status Effects Feedback:** Visual indicators for poison damage-over-time ticks and blast/nitro detonations.
- **Clean Personal Combat View:** By default, floating numbers only render for your own hunter's hits to eliminate visual clutter during multiplayer hunts.
- **In-Hunt Combat Recount:** An optional lightweight on-screen widget displaying hunt DPS, total damage dealt, and team damage percentage.
- **Native Resolution GPU Render:** Built on Rust and `wgpu` (Metal on macOS, DirectX 12 / Vulkan on Windows), rendering crisp vector text and animations on top of the emulator window.
- **Non-Invasive:** Interacts with the emulator purely through Azahar's localhost UDP memory RPC (`127.0.0.1:45987`). No emulator fork or modified builds required.

---

## Project Status

The project is under active development. Core reverse engineering, memory structures, 3D projection parameters, and the dynamic damage tap have all been discovered and verified live in-game.

| Milestone | Target | Status | Highlights |
|---|---|---|---|
| **Phase 1: RPC Engine** | v0.1.0 | ✅ Complete | UDP packet pipeline, batched reads, memory sources |
| **Phase 2: Reverse Engineering** | v0.2.0 | ✅ Complete | Monster list, camera, frame counter, hitzone transforms, damage tap |
| **Phase 3: Game Pipeline** | v0.3.0 | ✅ Complete | Snapshots, scene machine, monster identity, passive and tap events, sampler |
| **Phase 4: 3D Projection** | v0.4.0 | ✅ Complete | World-to-screen math, camera lerp, Azahar layout resolver |
| **Phase 5: Overlay Window** | v0.5.0 | ✅ Complete | Click-through overlay, Azahar tracking, debug HUD, calibration |
| **Phase 6: Visuals & MVP** | v0.6.0 | 📅 Planned | Floating text particles, hit animations, combat recount widget |

Detailed technical specifications and findings are available in [`docs/TECHNICAL_DESIGN.md`](docs/TECHNICAL_DESIGN.md) and [`docs/RE_NOTES.md`](docs/RE_NOTES.md).

---

## Compatibility

- **Target Game:** *Monster Hunter XX* (Japan, title ID `0004000000197100`) update v1.4, including community English and Spanish translation patches (title version 4224 / `0x1080`).
- **Emulator:** [Azahar](https://github.com/azahar-emu/azahar) (recommended version $\ge$ 2121.2). Other Citra forks supporting the standard memory RPC may also be compatible.
- **Operating Systems:**
  - **macOS:** Primary platform (Apple Silicon & Intel). Click-through transparent overlay via AppKit/Cocoa window tracking.
  - **Windows:** Secondary platform. Planned support via Win32 layered click-through window.
  - **Linux:** X11 supported; Wayland is currently not supported due to protocol restrictions on global window placement.

---

## Building from Source

### Prerequisites

- [Rust](https://www.rust-lang.org/) stable (see `rust-toolchain.toml`).
- Azahar emulator configured with RPC enabled (see [Azahar Setup Guide](docs/SETUP_AZAHAR.md)).

### Compilation

```bash
# Clone the repository
git clone https://github.com/Ezrgan/mhdn.git
cd mhdn

# Build all workspace crates
cargo build --workspace

# Run automated tests
cargo test --workspace
```

### RE & Probe CLI

The repository includes a dedicated CLI tool (`mhdn-probe`) used for live memory analysis, monitoring, and verification:

```bash
# Verify connection to running Azahar instance
cargo run -p mhdn-probe -- attach

# Check game version and verify profile fingerprint
cargo run -p mhdn-probe -- game-info --profile profiles/mhxx-jp-v1.4-es.toml

# Install dynamic damage tap and monitor live combat hits
cargo run -p mhdn-probe -- tap install
cargo run -p mhdn-probe -- tap follow
```

---

## System Architecture

```
┌──────────────────────────────────────────────┐
│ Azahar Emulator Process                      │
│                                              │
│  Guest ARM11 (MHXX v1.4)                     │
│    └─ Hook at 0x008D03E8 ──▶ ARM Stub Cave   │
│                                │             │
│  UDP RPC Server (:45987) ◀─────┘ (Ring Buf)  │
└───────────────────────▲──────────────────────┘
                        │ UDP Localhost
┌───────────────────────▼──────────────────────┐
│ mhdn Process (Rust)                          │
│                                              │
│  mhdn-rpc      Pipelined UDP memory client   │
│  mhdn-game     Snapshot builder & seqlock    │
│  mhdn-proj     3D camera-to-screen matrix    │
│  mhdn-platform Window follower & bounds sync │
│  mhdn-fx       Easing particle physics pool  │
│  mhdn-render   wgpu GPU text & HUD renderer  │
└──────────────────────────────────────────────┘
```

---

## Disclaimer & Legal

This project is an independent community development and is **not affiliated with, endorsed by, or associated with Capcom or Nintendo**. 

You must own a legitimate copy of *Monster Hunter XX* and extract your own game files. This repository contains **no copyrighted code, ROMs, or assets**. All offset profiles contain numeric memory addresses discovered solely for software interoperability.

Distributed under the **GNU General Public License v3.0 or later** (see [`LICENSE`](LICENSE)).
