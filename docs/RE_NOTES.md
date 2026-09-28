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
| position, frame counter, scene, hunter, camera | unresolved (`TBD`) |

```bash
cargo run -p mhdn-probe -- peek u32 0x00D2CAA0
cargo run -p mhdn-probe -- ptrverify 0x00D2CAA0+0x14+0x10A8+0x360
```
