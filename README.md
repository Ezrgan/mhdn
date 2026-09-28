# mhdn — MHXX damage numbers overlay for Azahar

External transparent overlay that draws Monster Hunter World–style floating damage numbers on top of
*Monster Hunter XX* (3DS) running in [Azahar](https://github.com/azahar-emu/azahar).

## Status

**Early development (Phase 0).** The workspace builds; the overlay is not usable yet.

| Milestone | Version | Scope |
|-----------|---------|--------|
| MVP (passive) | v0.1.0 | HP delta via Azahar RPC, projected numbers |
| Active mode | v0.2.0 | 3GX plugin with per-hit events |
| Polish | v0.3.0 | MSDF text, MHW-like motion |
| Release | v1.0.0 | macOS + Windows packages |

Implementation order and commits: [`PLAN.md`](PLAN.md). Architecture and rationale: [`docs/TECHNICAL_DESIGN.md`](docs/TECHNICAL_DESIGN.md).

## What this is (and is not)

- **Target game:** MHXX Japan (`0004000000197100`) with update v1.4 (including community translation patches).
  *Monster Hunter Generations Ultimate* is a **Switch** title and is **out of scope** for this repo.
- **Emulator:** Azahar (recommended). Citra-compatible forks may work with the same UDP RPC protocol.
- **Platform:** macOS first; Windows planned. Linux/X11 optional. Wayland is not supported for global overlays.

## Legal disclaimer

This project is **not affiliated** with Capcom or Nintendo. You must **own a legal copy** of the game and
obtain your own ROM/update files. This repository does **not** include ROMs, `code.bin`, or copyrighted game assets.
Offset profiles contain **numeric addresses only**, discovered via reverse engineering for interoperability.

Software is licensed under **GPL-3.0-or-later** (see [`LICENSE`](LICENSE)). Some design and layout logic is informed by
GPL-licensed community projects (Azahar, MH-HP-Overlay-For-3DS-Emulator, etc.).

## Building (developers)

Requirements: Rust stable (see `rust-toolchain.toml`), a recent Azahar build with RPC enabled.

```bash
cargo build --workspace
cargo test --workspace
```

Configure Azahar before live testing: [`docs/SETUP_AZAHAR.md`](docs/SETUP_AZAHAR.md).

## Architecture (short)

```
Azahar (RPC UDP :45987)  →  mhdn sampler  →  game model  →  animation  →  wgpu overlay window
```

Passive mode derives damage from monster HP deltas. Active mode (later) adds a 3GX plugin that publishes exact hit events.

## Contributing

Follow [Conventional Commits](https://www.conventionalcommits.org/) with scopes from `PLAN.md` (`rpc`, `game`, `app`, …).
Do not commit dumps, `.bin` files, or large `.mhrec` traces without Git LFS.
