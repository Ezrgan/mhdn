# Azahar Configuration Guide for Development (`mhdn`)

This guide explains how to prepare Azahar for memory inspection, reverse engineering, and overlay usage: RPC server setup, logging, window layouts, and GDB debugging.

---

## Prerequisites

- [Azahar](https://github.com/azahar-emu/azahar) **$\ge$ 2121.2** (the RPC server is disabled by default starting from this build).
- Legal copy of **Monster Hunter XX Japan** (`Title ID` `0004000000197100`) with **v1.4 update** installed.
  - In builds with the community Spanish/English translation patch, the title version in the TMD is typically **4224** (`0x1080`) instead of the base 4160.
- **Stereoscopic 3D disabled** (`render_3d = Off`, `factor_3d = 0`).

---

## Configuration File Paths

| OS | Default Path |
|---|---|
| macOS | `~/Library/Application Support/Azahar/config/qt-config.ini` |
| Windows | `%APPDATA%\Azahar\config\qt-config.ini` |
| Linux (portable) | `<Azahar directory>/user/config/qt-config.ini` |

*Close Azahar before editing the INI file, or restart the emulator after saving changes.*

---

## Required Settings for `mhdn`

You can set these in **Emulation → Configure → System** (or directly in `qt-config.ini`):

### 1. RPC Memory Server

- Enable **Enable RPC server** (`enable_rpc_server=true`).
- Azahar listens on **`127.0.0.1:45987`** via UDP. Never expose this port outside of localhost.

Verify responsiveness while the game is running:

```bash
cargo run -p mhdn-probe -- attach
```

### 2. RPC Logging Filter (Critical for Performance)

Azahar logs a `LOG_INFO` message for **every single RPC datagram**. At ~2000 requests/sec during combat, unfiltered logging will choke the host CPU and cause emulator stutter.

Add or update the logging filter in `qt-config.ini`:

```ini
[Core]
log_filter=*:Info RPC_Server:Warning
```

### 3. Screen Layout

**Recommended for overlay alignment:** **Separate Windows** — the top 3DS screen occupies its own dedicated host window, significantly simplifying screen-space projection.

**Alternative (common single-window setup):** **Large Screen** with large screen proportion and the bottom touch screen placed in a corner. The overlay's projection module (`mhdn-proj`) replicates Azahar's internal layout calculations.

Typical keys in `qt-config.ini`:

```ini
[Layout]
layout_option=2          # 2 = LargeScreen
large_screen_proportion=4
small_screen_position=2  # BottomRight
swap_screen=false
upright_screen=false
screen_top_stretch=false
singleWindowMode=true
showStatusBar=true       # Status bar takes vertical space; overlay accounts for this inset
fullscreen=false
```

### 4. Internal Resolution

Setting `resolution_factor=4` (or any scaling factor) scales Azahar's rasterization but **does not alter** the logical 400×240 projection coordinates.

---

## Reverse Engineering & Debugging Settings

### GDB Stub

- Enable **GDB stub** (`use_gdbstub=true`), listening on port **24689**.
- **Important:** Disable CPU JIT while using watchpoints. Watchpoints may not trigger reliably when JIT is active (Azahar issue #2199). Re-enable JIT when done, as the interpreter is substantially slower.

Example:

```bash
arm-none-eabi-gdb
(gdb) target remote :24689
(gdb) watch *(int*)0x082C4BB8
```

Fast memory dump (alternative to RPC dumping):

```bash
arm-none-eabi-gdb -batch \
  -ex "target remote :24689" \
  -ex "dump memory dumps/heap.bin 0x08000000 0x09000000" \
  -ex "detach" -ex "quit"
```

---

## Pre-Session Checklist

- [ ] RPC server enabled (`enable_rpc_server=true`)
- [ ] Log filter configured (`RPC_Server:Warning`)
- [ ] 3D stereoscopy disabled
- [ ] Title ID `0004000000197100` visible in `mhdn-probe attach`
- [ ] Quest loaded with target monster active

---

## Troubleshooting

| Issue | Resolution |
|---|---|
| RPC connection refused / times out | Verify `enable_rpc_server=true` and that Azahar is running version $\ge$ 2121.2. |
| High CPU usage / FPS drop in Azahar | Verify `log_filter=*:Info RPC_Server:Warning` in `qt-config.ini`. |
| Wrong process read | Azahar requires explicit PID selection via `SetGetProcess`. `mhdn-rpc` selects the MHXX title ID automatically. |
| Overlay offset in fullscreen | Check whether `showStatusBar=true` is enabled and account for the 22–26 pt bottom status bar inset. |

