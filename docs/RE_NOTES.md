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
