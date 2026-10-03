# mhdn — números de daño para MHXX en Azahar

[![CI](https://github.com/Ezrgan/mhdn/actions/workflows/ci.yml/badge.svg)](https://github.com/Ezrgan/mhdn/actions/workflows/ci.yml)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

---

## Español

### Qué es

**mhdn** es una aplicación aparte que dibuja números de daño flotantes encima de la ventana de [Azahar](https://github.com/azahar-emu/azahar) mientras juegas a **Monster Hunter XX** (3DS). Se conecta al emulador por su servidor de memoria RPC en tu propio ordenador (`127.0.0.1:45987`); no modifica Azahar ni el juego.

Descarga la [última release en GitHub](https://github.com/Ezrgan/mhdn/releases). No hace falta instalar Rust ni compilar nada.

### Qué no es (limitaciones actuales)

- Los números aparecen **cerca de tu cazador**, en dirección al monstruo que recibió el golpe (aproximadamente a distancia de alcance del arma). **No** aparecen en la hitzone ni en el hueso exacto del impacto; eso es trabajo futuro.
- **No** filtra solo tus golpes: en multijugador pueden verse golpes de otros cazadores.
- **No** muestra porcentaje de daño del equipo ni identifica críticos o daño elemental.
- El overlay se oculta cuando otra aplicación está en primer plano; vuelve a Azahar para verlo.

### Compatibilidad

| Requisito | Detalle |
|---|---|
| Juego | *Monster Hunter XX* **Japón**, actualización **v1.4**, Title ID `0004000000197100` |
| Parche ES/EN | Compatible; la versión de título esperada suele ser **4224** |
| Emulador | Azahar **2121.2 o posterior** |
| macOS | App universal (Apple Silicon e Intel) en un ZIP |
| Windows | ZIP x64 con `mhdn.exe` — **beta**: solo se ha comprobado que arranca y muestra el estado cuando Azahar no está; no está probada en un PC Windows real. La inicialización de GPU usa los límites del adaptador gráfico del sistema |

### Configurar Azahar (obligatorio)

1. Abre la configuración de Azahar (**Emulation → Configure → System**, o el equivalente en tu idioma).
2. Activa **Enable RPC server**.
3. Pon el filtro de log **exactamente** así (en la configuración o en `qt-config.ini`, sección `[Core]`):

   ```text
   *:Info RPC_Server:Warning
   ```

   Sin este filtro, el registro RPC puede subir mucho el uso de CPU y provocar tirones en Azahar.
4. Desactiva el **3D estereoscópico** (3D en **Off**).
5. **Reinicia Azahar** después de cambiar RPC, el filtro o el 3D.
6. Inicia MHXX y carga tu partida.

Comprueba el Title ID y la versión si la app indica juego no soportado: deben coincidir con la tabla de arriba.

### Instalar en macOS

1. En [Releases](https://github.com/Ezrgan/mhdn/releases), descarga el ZIP **universal para macOS**.
2. Descomprímelo y mueve la app donde quieras (por ejemplo, Aplicaciones).
3. La **primera vez**: clic derecho en la app → **Abrir** → confirma **Abrir** en el aviso de Gatekeeper.
4. Abre Azahar y MHXX, luego abre **mhdn**. No hace falta Terminal.

### Instalar en Windows

1. Descarga el ZIP para **Windows x64** desde la [última release](https://github.com/Ezrgan/mhdn/releases).
2. Descomprime **todo** el contenido del ZIP.
3. Ejecuta `mhdn.exe`.

Trata esta build como **beta** hasta que se valide en hardware real.

### Estado en la barra de menús (macOS)

El texto de la barra de menús indica si todo está bien:

| Texto | Significado |
|---|---|
| `mhdn: Waiting for Azahar` | Azahar no está visible o MHXX aún no está en ejecución |
| `mhdn: RPC off` | El servidor RPC no responde; revisa **Enable RPC server** y reinicia Azahar |
| `mhdn: Active` | Conexión y perfil correctos; el overlay puede mostrarse sobre Azahar |

(Otras variantes pueden indicar juego o versión no compatible.)

### Ajustes y ventana de configuración

La ventana de **ajustes** (secciones **Dashboard**, **Numbers**, **Corner** y **Setup**) sirve para cambiar tamaño y colores de los números, qué bandas de daño se muestran, el total/DPS en la esquina, y para iniciar o detener el overlay.

### Cómo salir

- **macOS:** en la barra de menús, haz clic en el texto de estado de mhdn y elige **Quit** / **Salir**.

### Más ayuda

Guía paso a paso y solución de problemas: [`docs/GUIA.md`](docs/GUIA.md). Detalle técnico de Azahar: [`docs/SETUP_AZAHAR.md`](docs/SETUP_AZAHAR.md).

### Aviso legal

Proyecto comunitario independiente; **no** está afiliado a Capcom ni Nintendo. Necesitas una copia legal del juego. Este repositorio no incluye ROMs ni assets del juego. Licencia: [GPL-3.0-or-later](LICENSE).

---

## English

### What it is

**mhdn** is a separate app that draws floating damage numbers on top of your [Azahar](https://github.com/azahar-emu/azahar) window while you play **Monster Hunter XX** (3DS). It talks to the emulator through the localhost memory RPC server (`127.0.0.1:45987`); it does not patch Azahar or the game.

Get the [latest release on GitHub](https://github.com/Ezrgan/mhdn/releases). You do not need Rust or a source build.

### What it is not (current limits)

- Numbers appear **near your hunter**, toward the monster that was hit (roughly weapon reach). They do **not** appear on the exact hitzone or bone contact point; that is future work.
- It does **not** show only your hits: in multiplayer you may see other hunters’ damage numbers.
- It does **not** show team damage percentage, and it does not label crits or elemental damage.
- The overlay hides when another app is in front; bring Azahar to the foreground to see it.

### Compatibility

| Requirement | Detail |
|---|---|
| Game | *Monster Hunter XX* **Japan**, **v1.4** update, Title ID `0004000000197100` |
| ES/EN patch | Supported; expected title version is usually **4224** |
| Emulator | Azahar **2121.2 or newer** |
| macOS | Universal app in a ZIP (Apple Silicon and Intel) |
| Windows | x64 ZIP with `mhdn.exe` — **beta**: verified only to launch and show status when Azahar is absent; not tested on a real Windows PC. GPU init uses your graphics adapter’s own limits |

### Configure Azahar (required)

1. Open Azahar settings (**Emulation → Configure → System**, or equivalent).
2. Turn on **Enable RPC server**.
3. Set the log filter **exactly** to (in the UI or in `qt-config.ini`, `[Core]` section):

   ```text
   *:Info RPC_Server:Warning
   ```

   Without this filter, RPC logging can drive high CPU use and stutter in Azahar.
4. Turn **stereoscopic 3D Off**.
5. **Restart Azahar** after changing RPC, the filter, or 3D.
6. Start MHXX and load your save.

If the app reports an unsupported game, check Title ID and version against the table above.

### Install on macOS

1. From [Releases](https://github.com/Ezrgan/mhdn/releases), download the **universal macOS** ZIP.
2. Extract it and move the app wherever you like (e.g. Applications).
3. **First launch:** right-click the app → **Open** → confirm **Open** in the Gatekeeper dialog.
4. Start Azahar and MHXX, then open **mhdn**. No terminal needed.

### Install on Windows

1. Download the **Windows x64** ZIP from the [latest release](https://github.com/Ezrgan/mhdn/releases).
2. Extract the **entire** ZIP.
3. Run `mhdn.exe`.

Treat this build as **beta** until it is validated on real hardware.

### Menu bar status (macOS)

The menu bar text tells you whether things are wired up:

| Text | Meaning |
|---|---|
| `mhdn: Waiting for Azahar` | Azahar is not visible or MHXX is not running yet |
| `mhdn: RPC off` | RPC server is not responding; check **Enable RPC server** and restart Azahar |
| `mhdn: Active` | Connection and profile OK; the overlay can show over Azahar |

(Other messages may indicate an unsupported game or version.)

### Settings window

The **settings** window (**Dashboard**, **Numbers**, **Corner**, and **Setup**) is where you change number size and colors, which damage bands appear, the corner total/DPS, and where you start or stop the overlay.

### How to quit

- **macOS:** click the mhdn status text in the menu bar and choose **Quit**.

### More help

Step-by-step guide and troubleshooting: [`docs/GUIA.md`](docs/GUIA.md). Azahar technical setup: [`docs/SETUP_AZAHAR.md`](docs/SETUP_AZAHAR.md).

### Legal

Independent community project; **not** affiliated with Capcom or Nintendo. You must own a legal copy of the game. This repo contains no ROMs or game assets. License: [GPL-3.0-or-later](LICENSE).
