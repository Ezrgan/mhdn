# Guía de instalación / Installation Guide

## Español

### Antes de empezar

Necesitas:

- **Azahar 2121.2 o posterior**.
- **Monster Hunter XX japonés v1.4**, Title ID `0004000000197100`.
- Si usas el parche de traducción, la versión de título esperada es **4224**.

Estado actual:

- Los números aparecen **cerca de tu cazador**, en dirección al monstruo golpeado. Todavía no aparecen en la
  hitzone o punto exacto de contacto.
- El overlay todavía no puede mostrar solo tus golpes ni distinguir críticos o daño elemental.
- Windows es una **beta** y todavía no se ha probado en un PC real.

Los archivos estarán en la [última release de GitHub](https://github.com/Ezrgan/mhdn/releases/latest). No
necesitas Rust, compilar el proyecto ni usar una terminal.

### 1. Configurar Azahar

1. Abre Azahar y entra en su configuración.
2. Activa **Enable RPC server**.
3. Configura el filtro de log exactamente así:

   ```text
   *:Info RPC_Server:Warning
   ```

   Es importante: sin este filtro, el tráfico RPC puede causar uso alto de CPU y tirones.
4. Desactiva el **3D estereoscópico**: 3D debe estar en **Off**.
5. Reinicia Azahar si cambiaste estas opciones.
6. Inicia MHXX y carga tu partida.

### 2. Instalar en macOS

1. Descarga el ZIP **universal para macOS** de la última release.
2. Descomprímelo y mueve la app donde prefieras, por ejemplo a Aplicaciones.
3. La primera vez, haz clic derecho sobre la app y elige **Abrir**; después confirma **Abrir** en Gatekeeper.
4. Inicia Azahar y MHXX, y luego abre la app. No hace falta usar Terminal.

### 3. Instalar en Windows

1. Descarga el ZIP para Windows de la última release.
2. Descomprímelo completo.
3. Ejecuta `mhdn.exe`.

Esta versión está marcada como **beta**: todavía no se ha validado en un PC Windows real.

### Estados de la app

- **`mhdn: esperando Azahar`:** Azahar no está visible o MHXX todavía no está abierto.
- **`mhdn: RPC desactivado`:** Azahar no responde en su servidor RPC; comprueba **Enable RPC server** y
  reinícialo.
- **`mhdn: juego no soportado (...)`:** el juego, región o versión no coincide con MHXX JP v1.4 compatible.
- **`mhdn: activo`:** la conexión y el perfil son correctos; el overlay está listo.

El overlay se oculta cuando otra aplicación está en primer plano. Vuelve a Azahar para verlo.

### Cómo salir

- **macOS:** abre el menú de `mhdn` en la barra de menús y elige **Salir**.
- **Windows:** abre el icono de `mhdn` en la bandeja del sistema y elige **Exit/Salir**.

### Solución de problemas

**La app dice “RPC desactivado”**

- Confirma que usas Azahar 2121.2 o posterior.
- Activa **Enable RPC server**.
- Reinicia Azahar después de cambiar la opción.
- Comprueba que MHXX está abierto.

**Azahar consume mucha CPU o da tirones**

- Revisa que el filtro sea exactamente `*:Info RPC_Server:Warning`.
- Reinicia Azahar después de guardarlo.

**La app dice “juego no soportado”**

- Comprueba el Title ID: debe ser `0004000000197100`.
- Instala la actualización v1.4.
- Con el parche de traducción, comprueba que la versión de título sea 4224.

**El overlay no aparece**

- Pon Azahar en primer plano; el overlay se oculta sobre otras aplicaciones.
- Confirma que el estado sea **activo**.
- Desactiva el 3D estereoscópico.
- Si usas pantalla completa, prueba primero con Azahar en una ventana para aislar el problema.

**Los números están desplazados**

- Mantén el 3D en Off.
- Evita cambiar el layout de pantallas mientras la app está abierta; reinicia la app si lo cambias.
- En pantalla completa, prueba el mismo layout en ventana y vuelve a abrir el overlay.

**Los números no están sobre la parte golpeada**

- Es una limitación conocida. En esta versión aparecen cerca del cazador, no en el punto exacto de contacto.

---

## English

### Before you start

You need:

- **Azahar 2121.2 or newer**.
- **Monster Hunter XX Japan v1.4**, Title ID `0004000000197100`.
- If you use a translation patch, the expected title version is **4224**.

Current limitations:

- Numbers appear **near your hunter**, toward the monster that was hit. They do not yet appear on the exact
  hitzone or contact point.
- The overlay cannot yet filter only your hits or identify critical and elemental damage.
- Windows is a **beta** and has not yet been tested on a real PC.

Downloads will be available on the [latest GitHub release](https://github.com/Ezrgan/mhdn/releases/latest).
You do not need Rust, a source build, or a terminal.

### 1. Configure Azahar

1. Open Azahar settings.
2. Enable **Enable RPC server**.
3. Set the log filter exactly to:

   ```text
   *:Info RPC_Server:Warning
   ```

   This matters: without the filter, RPC logging can cause high CPU usage and stutter.
4. Turn stereoscopic **3D Off**.
5. Restart Azahar after changing these settings.
6. Start MHXX and load your save.

### 2. Install on macOS

1. Download the **universal macOS ZIP** from the latest release.
2. Extract it and move the app wherever you prefer, such as Applications.
3. On first launch, right-click the app, choose **Open**, then confirm **Open** in Gatekeeper.
4. Start Azahar and MHXX, then open the app. No terminal is required.

### 3. Install on Windows

1. Download the Windows ZIP from the latest release.
2. Extract the entire ZIP.
3. Run `mhdn.exe`.

This build is marked **beta** because it has not yet been validated on a real Windows PC.

### App status

- **`mhdn: esperando Azahar` (waiting for Azahar):** Azahar is not visible or MHXX is not running yet.
- **`mhdn: RPC desactivado` (RPC off):** Azahar's RPC server is not responding; enable it and restart Azahar.
- **`mhdn: juego no soportado (...)` (unsupported game):** the game, region, or version does not match a
  supported MHXX JP v1.4 build.
- **`mhdn: activo` (active):** the connection and profile are correct; the overlay is ready.

The overlay hides while another application is frontmost. Return to Azahar to show it.

### How to quit

- **macOS:** open the `mhdn` menu-bar item and choose **Quit**.
- **Windows:** open the `mhdn` system-tray icon and choose **Exit**.

### Troubleshooting

**The app says “RPC off”**

- Confirm that Azahar is version 2121.2 or newer.
- Enable **Enable RPC server**.
- Restart Azahar after changing the setting.
- Make sure MHXX is running.

**Azahar uses too much CPU or stutters**

- Check that the filter is exactly `*:Info RPC_Server:Warning`.
- Restart Azahar after saving it.

**The app says “unsupported game”**

- Check that the Title ID is `0004000000197100`.
- Install the v1.4 update.
- With a translation patch, confirm that the title version is 4224.

**The overlay does not appear**

- Bring Azahar to the front; the overlay hides over other applications.
- Confirm that the status is **active**.
- Turn stereoscopic 3D off.
- If you use fullscreen, try Azahar in a window first to isolate the issue.

**Numbers are offset**

- Keep 3D off.
- Avoid changing the screen layout while the app is open; restart the app if you change it.
- If fullscreen is misaligned, test the same layout in a window and reopen the overlay.

**Numbers are not on the part that was hit**

- This is a known limitation. This build places them near the hunter, not at the exact contact point.
