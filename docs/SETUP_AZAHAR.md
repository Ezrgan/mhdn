# Configuración de Azahar para desarrollo (mhdn)

Guía para dejar Azahar listo para reverse engineering y pruebas del overlay: RPC, logs, layout, GDB y
plugin loader. Ajusta según tu SO.

## Requisitos

- [Azahar](https://github.com/azahar-emu/azahar) **≥ 2121.2** (el servidor RPC viene desactivado por defecto desde esa versión).
- Copia legal de **MHXX Japón** (`Title ID` `0004000000197100`) con actualización **v1.4** instalada.
  En builds con parche de traducción al español, la versión de título del update suele ser **4224** (`0x1080`) en lugar de 4160.
- **3D estereoscópico apagado** (`render_3d = Off`, `factor_3d = 0`).

## Rutas del archivo de configuración

| SO | Ruta |
|----|------|
| macOS | `~/Library/Application Support/Azahar/config/qt-config.ini` |
| Windows | `%APPDATA%\Azahar\config\qt-config.ini` |
| Linux (portable) | `<carpeta Azahar>/user/config/qt-config.ini` |

Cierra Azahar antes de editar el INI, o reinicia el emulador tras guardar.

## Opciones obligatorias para mhdn

En **Emulation → Configure → System** (o equivalente en el INI):

### 1. Servidor RPC (Modo Pasivo y lectura del plugin)

- Activar **Enable RPC server** (`enable_rpc_server=true`).
- Escucha en **`127.0.0.1:45987`** (UDP). No expongas este puerto fuera de localhost.

Comprueba que responde (con el juego en marcha):

```bash
# Tras implementar mhdn-probe (Fase 2):
mhdn-probe attach
```

Mientras tanto, puedes usar el script oficial `citra.py` del repo de Azahar (`dist/scripting/citra.py`).

### 2. Filtro de log del RPC (importante)

El servidor RPC registra **un `LOG_INFO` por paquete**. A ~2000 req/s el log crece y penaliza CPU.

En el INI, ajusta el filtro global:

```ini
[Core]
log_filter=*:Info RPC_Server:Warning
```

(Sintaxis exacta puede variar; busca la clave `log_filter` en tu `qt-config.ini`.)

### 3. Layout de pantalla

**Recomendado para alinear el overlay:** **Separate Windows** — la pantalla superior ocupa su propia ventana,
lo que simplifica el rect de proyección.

Alternativa válida (desarrollo actual): **Large Screen** con proporción grande y pantalla táctil en una esquina.
El overlay reimplementará la matemática de layout de Azahar (`mhdn-proj`).

Claves típicas en `qt-config.ini`:

```ini
[Layout]
layout_option=2          # 2 = LargeScreen (ver enum en Azahar)
large_screen_proportion=4
small_screen_position=2  # BottomRight
swap_screen=false
upright_screen=false
screen_top_stretch=false
singleWindowMode=true
showStatusBar=true       # resta altura al área de juego; el overlay debe tener inset inferior
fullscreen=false
```

Si cambias layout con atajos **sin** guardar config, el overlay puede desalinear hasta recalibrar (Fase 5).

### 4. Resolución interna

`resolution_factor=4` (u otro) **no cambia** la proyección lógica 400×240; solo escala el render del emulador.

## Opciones solo para reverse engineering

### GDB stub (Fase 2 / 7)

- Activar **GDB stub** (`use_gdbstub=true`), puerto por defecto **24689**.
- Para usar watchpoints **desactiva el JIT de CPU** mientras depuras. Es obligatorio: con el JIT activado los watchpoints pueden no dispararse nunca (issue #2199 de Azahar). Los breakpoints de ejecución sí funcionan con JIT. Vuelve a activarlo al terminar, porque sin JIT el juego va mucho más lento.

Ejemplo:

```bash
arm-none-eabi-gdb
(gdb) target remote :24689
(gdb) watch *(int*)0x........   # dirección guest del HP tras encontrarla con mhdn-probe
```

Volcado rápido de regiones (alternativa al dump por RPC):

```text
(gdb) dump memory heap.bin 0x08000000 0x09000000
```

### Plugin loader 3GX (Fase 7 — Modo Activo)

- Activar **Enable 3GX plugin loader** (`plugin_loader=true`).
- Instalar el plugin en la SD virtual:

```text
sdmc/luma/plugins/0004000000197100/mhdn.3gx
```

(Crea carpetas si no existen; el Title ID es MHXX JP.)

Reinicia el juego tras copiar el `.3gx`. El plugin **no** modifica partidas guardadas; solo parchea en RAM.

Si el juego crashea al arrancar con el loader activado (issue #1381), desactívalo: el Modo Tap no lo necesita.

### Cheats de Azahar (solo para la validación 2.19)

- *Emulation → Cheats*: añadir el cheat "Hit Monster Display Last Damage v1.4" para confirmar la semántica del sitio de daño.
- **Desactívalo antes de usar el Modo Tap**: ambos usan la misma *code cave* y el mismo punto de enganche (`0x8D03E8`).

## Instalación del juego y parches

1. Instala la base MHXX JP en Azahar (CIA/3DS según tu flujo habitual).
2. Instala la **actualización oficial v1.4** (`0004000E00197100`).
3. Si usas **parche de traducción** (ES/EN), instálalo como update compatible; verifica versión de título en el TMD
   (`content/00000001.tmd`, offset `0x1DC`, u16 big-endian). Documenta el valor en `docs/RE_NOTES.md` (Fase 2).

**No subas ROMs, CIAs ni `code.bin` a este repositorio.**

## Checklist antes de una sesión de RE

- [ ] RPC activo; log con `RPC_Server:Warning`
- [ ] 3D off; juego en misión con monstruo visible
- [ ] Title ID `0004000000197100` visible en lista de procesos RPC
- [ ] (Opcional) GDB stub / plugin loader según la tarea del PLAN

## Multijugador local

Cada jugador usa **su propio** Azahar y **su propio** overlay. Valida host vs. cliente en F2.17: el HP en clientes
puede llegar agrupado o con retraso; el Modo Activo (plugin) es la vía para filtrar golpes propios.

## Problemas frecuentes

| Síntoma | Qué revisar |
|---------|-------------|
| RPC no responde | `enable_rpc_server`, firewall local, Azahar ≥ 2121.2 |
| Azahar muy lento con overlay | `log_filter` del RPC; reducir req/s en reposo (fases posteriores) |
| Proceso incorrecto leído | Desde PR #956 hay que **seleccionar PID** (`SetGetProcess`); `mhdn-rpc` lo hará en Fase 1 |
| Plugin no carga | Ruta `sdmc/luma/plugins/<TitleID>/`, loader activado, juego reiniciado |

## Referencias

- [`PLAN.md`](../PLAN.md) — fases y commits
- [`TECHNICAL_DESIGN.md`](TECHNICAL_DESIGN.md) — RPC, memoria guest, arquitectura
- Azahar: `src/core/rpc/`, `dist/scripting/citra.py`, `src/common/settings.h`
