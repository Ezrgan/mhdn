# Contributing to mhdn

Thank you for considering a contribution! Below you'll find everything you need to get started.

---

## Code of Conduct

Be respectful and constructive. Discussions should stay focused on the project.

---

## Getting Started

### Prerequisites

- **Rust** stable toolchain (version pinned in `rust-toolchain.toml`).
- **Azahar** emulator with RPC enabled (see [`docs/SETUP_AZAHAR.md`](docs/SETUP_AZAHAR.md)).
- A legitimate copy of *Monster Hunter XX* (JP, v1.4).

### Build & Test

```bash
git clone https://github.com/Ezrgan/mhdn.git
cd mhdn
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

All four commands must pass before opening a pull request.

---

## How to Contribute

### Reporting Bugs

1. Check that the issue hasn't already been reported.
2. Open an issue with:
   - OS and Azahar version.
   - Steps to reproduce.
   - Expected vs. actual behaviour.
   - Relevant log output (no game ROMs or copyrighted assets).

### Proposing Features

Open an issue first to discuss the idea. Large changes should be preceded by a brief design note (see `docs/adr/` for examples of the ADR format we use).

### Submitting a Pull Request

1. Fork the repository and create a feature branch from `main`.
2. Keep commits small and focused; use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`).
3. Add or update tests for any changed behaviour.
4. Run the full check suite locally (build, test, clippy, fmt).
5. Fill in the pull request template when you open the PR.
6. All PRs require at least one review from a maintainer before merging.

---

## Project Layout

```
crates/
  mhdn-rpc/       UDP RPC client for Azahar memory access
  mhdn-game/      Game model: profiles, snapshots, damage events
  mhdn-probe/     RE & diagnostics CLI (mhdn-probe)
  mhdn-proj/      3D camera-to-screen projection
  mhdn-platform/  OS-specific overlay window management
  mhdn-fx/        Particle physics and easing
  mhdn-render/    wgpu GPU renderer
  mhdn-app/       Main overlay application entry point
docs/
  TECHNICAL_DESIGN.md   Architecture and data flow
  RE_NOTES.md           Reverse-engineering session log
  SETUP_AZAHAR.md       Emulator configuration guide
  adr/                  Architecture Decision Records
profiles/
  mhxx-jp-v1.4-es.toml  Offset profile for JP v1.4 + ES patch
```

---

## Style Guide

- **Rust:** `rustfmt` defaults (configured in `rustfmt.toml`). Run `cargo fmt` before committing.
- **Clippy:** fix all warnings (`-D warnings`). New `#[allow(...)]` attributes require a comment explaining why.
- **Comments:** English only. Prefer explaining *why* over *what*.
- **Unsafe:** forbidden (`#![forbid(unsafe_code)]`) in all crates except `mhdn-platform` where OS APIs require it. Every `unsafe` block must have a `// SAFETY:` comment.

---

## Commit Guidelines

Use [Conventional Commits](https://www.conventionalcommits.org/):

| Type | When to use |
|------|-------------|
| `feat` | New user-visible feature |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `test` | Tests only |
| `refactor` | Code restructuring, no behaviour change |
| `perf` | Performance improvement |
| `chore` | Build, CI, dependencies |

---

## What Not to Include

- **No game ROMs**, ISO images, or any copyrighted Capcom/Nintendo assets.
- **No raw memory dumps** of the game process (`.bin`, `.mhrec` files go in `dumps/` which is gitignored).
- **No personal API keys or credentials** of any kind.

---

## License

By submitting a pull request you agree that your contribution will be licensed under the **GNU General Public License v3.0 or later** as stated in the [`LICENSE`](LICENSE) file.
