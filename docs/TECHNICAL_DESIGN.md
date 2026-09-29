# Diseño técnico — Damage Numbers Overlay para MHXX en Azahar

> Qué se va a construir, por qué de esta forma, y cómo funciona cada pieza.
> El orden de implementación y los commits están en [`../PLAN.md`](../PLAN.md).

---

## 1. El problema

MHXX (3DS, 2017) no muestra números de daño. Queremos el feedback visual de Monster Hunter World:
números que "saltan" desde el punto del golpe, suben, se desvanecen, y cambian de color/tamaño según la intensidad.

Para dibujar un número encima de un monstruo hay que resolver cuatro subproblemas independientes:

1. **Cuándo y cuánto daño:** detectar cada golpe y su valor.
2. **Dónde en el mundo:** la posición 3D del impacto o del monstruo.
3. **Dónde en la pantalla:** proyectar esa posición con la misma cámara que usa el juego.
4. **Dónde en el escritorio:** saber en qué rectángulo del monitor está pintando Azahar la pantalla superior del 3DS.

Cada subproblema tiene su fuente de datos y su módulo, y se pueden probar por separado.

---

## 2. Cómo funciona el entorno (lo que hay que entender primero)

### 2.1 El 3DS emulado

- CPU **ARM11 (ARMv6K)**, little-endian. El juego es un proceso con memoria virtual propia:
  - `0x00100000`: código (`.text`), `.rodata`, `.data`, `.bss` del ejecutable (región "estática": las direcciones no cambian entre sesiones de la misma versión).
  - `0x08000000`: heap de la aplicación (objetos dinámicos: monstruos, cazadores, cámara...).
  - `0x14000000` / `0x30000000`: linear heap (memoria contigua física, usada por GPU y algunos datos).
  - `0x06000000` / `0x07000000`: heap y ejecutable de plugins 3GX (cuando el plugin loader está activo).
- GPU **PICA200**. Pantalla superior de **400×240** (físicamente vertical, 240×400); pantalla inferior de 320×240.
- MHXX corre a **30 FPS** y usa el motor MT Framework Mobile de Capcom.

### 2.2 Azahar

Azahar es el fork activo de Citra (Citra se discontinuó en 2024). Lo que nos importa:

- **Servidor RPC** (`src/core/rpc/`): UDP en `127.0.0.1:45987`. Permite listar procesos, elegir PID, y leer/escribir
  memoria virtual del proceso elegido. Paquete = header de 16 bytes (`version, id, type, size`) + hasta 1024 bytes de datos.
  Está **desactivado por defecto** (opción *Enable RPC server*).
  - Se atiende en un hilo propio, **sin sincronizar con la CPU emulada** → una lectura puede ver un estado a medio actualizar.
  - Registra un `LOG_INFO` por paquete → hay que filtrar ese log para no penalizar al emulador.
- **Plugin Loader 3GX:** carga plugins en formato Luma3DS dentro del proceso del juego (`sdmc/luma/plugins/<TitleID>/*.3gx`).
- **GDB stub:** depuración remota con breakpoints y **watchpoints** de lectura/escritura (exactos con el JIT desactivado).
- **Mods:** `load/mods/<TitleID>/exefs/code.ips` para parches de código compatibles con Luma.
- **Layouts de pantalla** configurables y guardados en `qt-config.ini`.

---

## 3. Decisión de arquitectura y alternativas evaluadas

### 3.1 Alternativas

| Opción | Descripción | Ventajas | Desventajas | Veredicto |
|---|---|---|---|---|
| **A. Overlay externo + RPC** | Proceso propio lee RAM por el RPC oficial y dibuja en una ventana transparente encima de Azahar | Sin modificar nada; API oficial; multiplataforma; no necesita permisos especiales del SO (en macOS, leer memoria de otro proceso con `task_for_pid` requiere desactivar protecciones o entitlements) | Solo ve el *estado*, no los *eventos*: el daño se deduce de ΔHP; lecturas no sincronizadas | ✅ **Base (Modo Pasivo)** |
| **B. Plugin 3GX (dentro del juego)** | Código C++ que corre dentro del juego emulado y hookea funciones | Acceso a eventos exactos (cada hit, crítico, elemento, autor, punto de impacto); puede capturar estado consistente en el límite de frame | Hay que hacer RE de la función de daño; si falla, puede colgar el juego; dibujar desde aquí sería a 400×240 y con problemas de sincronía con el render por hardware del emulador | ✅ **Como productor de datos (Modo Activo)**, nunca para dibujar |
| **C. Fork de Azahar** | Modificar el emulador: overlay ImGui dentro de su render, acceso directo a memoria y uniforms del PICA | Sincronía perfecta; matrices exactas de la GPU; sin tracking de ventanas | Mantener un fork C++ enorme (GPL, rebase continuo); el usuario debe usar tu build; contradice el requisito "no invasivo" del PRD | ❌ Descartada |
| **D. Proxy DLL / inyección en el proceso del emulador** | Inyectar en Azahar y hookear su presentación (Metal/Vulkan/OpenGL) | Dibuja dentro del swapchain del emulador | Hay que buscar a mano dónde está la RAM emulada dentro del proceso del host (cambia en cada build); frágil ante actualizaciones; en macOS es inviable por SIP/hardened runtime; distinto por backend gráfico | ❌ Descartada |
| **E. Cheats Gateway/Action Replay** | Códigos de trucos soportados por Azahar | Sencillo | No pueden emitir eventos ni exponer datos estructurados | ❌ Descartada |
| **F. Parche ARM a mano (stub + salto)** | Un stub de ~20 instrucciones en una *code cave*, instalado **por el propio overlay vía escritura RPC** (o como `code.ips`) | No depende del plugin loader ni de cheats; instalable/desinstalable en caliente; el sitio exacto ya se conoce gracias a un cheat público (§3.3) | Ensamblador a mano; solo captura lo que haya en registros/stack en ese punto | ✅ **Modo Tap (nivel intermedio)** |

### 3.2 Decisión: híbrido en tres niveles (A → F → B)

1. **Modo Pasivo (A)** llega primero porque da valor sin el RE más difícil (el de la función de daño) y
   porque su parte de HP ya está documentada por proyectos previos. Es el *fallback* permanente. Es un
   **detector de cambios de HP**, no de golpes: varios golpes en el mismo frame salen como un solo número.
2. **Modo Tap (F)** da golpes exactos (valor y monstruo) enganchando el sitio de aplicación del daño que ya
   se conoce (§3.3). Si su spike da GO, entra en el MVP.
3. **Modo Activo (B)** añade la semántica completa (autor, crítico, elemento, punto de impacto) si el plugin
   loader es fiable en tu build; si no, esa semántica se busca ampliando el Tap.
4. Los tres producen el mismo `DamageEvent` (con un campo `confidence`: `Exact`, `HpDelta` o `AggregatedHpDelta`),
   así que el resto del overlay no cambia. Prioridad automática: Plugin > Tap > Pasivo.
5. **El render siempre es externo**, a resolución nativa del host, con texto vectorial (MSDF): es la única
   forma de obtener un aspecto "moderno" sobre un juego de 400×240.

Esta separación hace que el proyecto **nunca quede bloqueado**: si el Tap o el plugin resultan inviables, el MVP ya está entregado.

### 3.3 El atajo: el cheat "Last Damage" de MHXX v1.4

Un cheat público ("Hit Monster Display Last Damage v1.4") muestra el daño del último golpe. Al decodificar su
ARM se ve que engancha una función del juego en `0x008D03E8`–`0x008D03FC`:

```
0x8D03E8  ldr r12, [r3, #0xA8]    ; r12 = objeto del monstruo      (aquí r1 = −daño)
0x8D03EC  ldr r0,  [r12, #0x360]  ; r0  = HP actual
   ...                              ; HP nuevo = HP + r1 (con límites)
0x8D03FC  str r0,  [r12, #0x360]  ; escribe el HP nuevo
```

`objeto+0x360` es exactamente el HP que ya leemos por la cadena de punteros conocida, lo que valida ambas fuentes.
Eso da, sin buscar a ciegas, el punto donde cada golpe se aplica con su valor y su objetivo. El Tap copia `r1`,
`r12`, `r3`, `lr` y unas palabras del stack a un ring buffer, y desde `lr`/stack se sube a los *callers*, donde
deberían estar el atacante y los datos del golpe. Las direcciones son de la v1.4 oficial y se verifican en el
build con parche en español antes de parchear nada.

---

## 4. Flujo de datos detallado

### 4.1 Modo Pasivo

```
cada ~16 ms (60 Hz) en misión:
  f0 ← frame_counter
  lecturas agrupadas (pipelined):
      lista de monstruos (1 bloque ≤ 1 KB)
      por monstruo visible: [HP, HP máx, ID] + posición (fusionadas si distan < 1 KB)
      cámara (eye, target, fov) o matriz
      flags de escena
  f1 ← frame_counter
  si f0 ≠ f1: reintentar (máx 2) / descartar
  por monstruo: ΔHP = hp_prev − hp  →  DamageEvent si ΔHP > 0
```

Presupuesto típico: ~15–25 peticiones UDP por muestra → ~1000–1500 req/s. Una petición local tarda decenas de µs.

### 4.2 Modo Tap

```
instalación (una vez por arranque del título):
  verificar palabras originales en 0x8D03E8/EC/FC y que cave (0xBF2Dxx) y ring (0xD320xx) están vacíos
  escribir stub en la cave → escribir salto en 0x8D03E8 (4 bytes alineados) → Azahar invalida el JIT
juego emulado:  golpe → 0x8D03E8 → stub: instrucción original, ring[seq % 64] = {seq, r1, r12, r3, lr, sp[0..4]}
                → barrera → write_seq = seq → vuelve a 0x8D03EC
overlay:        lee write_seq y solo las entradas nuevas → DamageEvent { confidence: Exact }
                ΔHP − Σ Tap > 0 ⇒ daño por otra ruta ⇒ evento HpDelta con el residuo
cierre:         restaurar la palabra original en 0x8D03E8
```

### 4.3 Modo Activo (plugin)

```
juego emulado:  hit → función de daño → [hook] → escribe MhdnEvent en ring[seq % 256] → dmb → write_seq = seq
                fin de frame → [callback] → copia cámara al bloque con seqlock
overlay:        lee write_seq → lee solo eventos nuevos → DamageEvent exactos
                lee bloque de cámara (reintento si frame_seq impar o cambió)
                (el ΔHP se sigue leyendo como verificación cruzada)
```

### 4.4 De evento a píxel

```
DamageEvent.anchor (mundo) ─▶ cámara interpolada (t_render = now − 1 frame del juego)
    ─▶ clip/NDC ─▶ píxel 400×240 ─▶ rect de pantalla superior (layout de Azahar) ─▶ punto en el monitor
    + offset de animación en espacio de pantalla (subida, dispersión, apilado)
```

El número queda **anclado al mundo**: si la cámara gira mientras el número flota, este sigue sobre el punto del golpe
(igual que en MHW), porque se re-proyecta en cada frame.

---

## 5. Perfiles de offsets (datos por versión del juego)

Los offsets **no** van en el código: van en `profiles/*.toml`. Así, soportar otra versión o región consiste en
añadir un archivo, no en recompilar.

Esquema propuesto (los valores de monstruo/HP provienen de la investigación previa; el resto se completa en la Fase 2):

```toml
[meta]
game = "MHXX"
region = "JP"
version = "1.4-es"            # actualización v1.4 con parche de traducción al español
title_id = 0x0004000000197100
update_title_version = 4224   # 0x1080, leído del TMD de 0004000E00197100 (v1.4 oficial = 4160)
fingerprint = [
  { addr = 0x00140000, len = 4096, xxh3 = 0x0000000000000000 },  # se rellena en F2 (2.8)
]

[monster_list]
# Primer puntero estático cuyo valor pertenezca a `expect` gana (varían entre v1.0–1.4)
base_candidates = [
  { addr = 0x00D2CAA0, expect = [0x082B7720, 0x082B9660] },
  { addr = 0x00D30AA0, expect = [0x082B98B0] },
  { addr = 0x00D3A8E0, expect = [0x082D0760] },
]
slots = 16                 # a validar (max_monsters en la referencia)
slot_stride = 4
slot_offset = 0x14
chain = [0x10A8, 0x360]    # deref(base + slot) + 0x10A8 → deref → + 0x360 = monster

[monster]                  # relativos a la dirección `monster` resuelta
hp       = { off = 0x0,     ty = "u32" }
max_hp   = { off = 0x4,     ty = "u32" }
species  = { off = 0x5A18,  ty = "u16" }
size     = { off = -432,    ty = "f32" }    # -0x1B0 (TOML no admite hex negativo)
pos      = { off = "TBD",   ty = "vec3" }   # F2 (2.13)
poison   = { off = 0x54E4,  ty = "u16" }
visible_flag = { off = -5128, ty = "u8", hidden_value = 0x7 }   # -0x1408

[frame_counter]            # F2 (2.10)
chain = "TBD"

[scene]                    # F2 (2.11)
chain = "TBD"

[hunter]                   # F2 (2.12)
pos = "TBD"

[camera]                   # F2 (2.14) — una de las dos formas
mode = "params"            # "params" | "matrix"
eye = "TBD"
target = "TBD"
fov_y = "TBD"
fov_unit = "rad"

[species]                  # altura del ancla por especie (offset vertical en unidades del juego)
default_anchor_height = 150.0
# 1 = { anchor_height = 220.0 }  # ejemplo, se completa por especie

[damage_tap]               # opcional (PLAN 3.1); valores de la v1.4 oficial, a verificar en 2.18
hook_addr = "0x008D03E8"
return_addr = "0x008D03EC"
expected_words = [
  { addr = "0x008D03E8", word = "0xE593C0A8" },   # ldr r12,[r3,#0xA8]
  { addr = "0x008D03EC", word = "0xE59C0360" },   # ldr r0,[r12,#0x360]
  { addr = "0x008D03FC", word = "0xE58C0360" },   # str r0,[r12,#0x360]
]
cave = { addr = "TBD", len = "TBD" }             # dentro de 0x00BF2D00–0x00BF3000 si 2.18 confirma que está libre
ring = { addr = "TBD", capacity = 64, entry_size = 40 }   # .bss libre verificada (zona 0x00D32000+)
```

Las ventanas de `fingerprint` nunca pueden solaparse con `hook_addr`, `cave` ni `ring`: la validación del perfil lo rechaza.

Los valores de `[monster]`, `[monster_list]` y `poison` salen del proyecto GPLv3 *MH-HP-Overlay-For-3DS-Emulator*
(`modules/mhxx.py`) y **deben revalidarse** en la Fase 2 antes de darlos por buenos.

---

## 6. Técnicas de reverse engineering (resumen del porqué)

| Qué buscamos | Técnica | Por qué funciona |
|---|---|---|
| HP del monstruo | Escáner de valores (igual/menor) + cadena de punteros conocida | El HP es un entero que baja exactamente al golpear. |
| Contador de frames | Escáner `inc` repetido | Un u32 que sube a ritmo constante es fácil de aislar. Da el "reloj" del juego para el seqlock. |
| Posición del cazador | Escáner de floats `inc/dec/unchanged` al caminar | La posición cambia de forma monótona al moverse en una dirección y se queda fija al estar quieto. |
| Posición del monstruo | Exploración ±0x2000 alrededor del struct del monstruo + distancia al cazador | En MT Framework los datos de transformación viven dentro del objeto; la distancia al cazador valida el candidato sin ver la pantalla. |
| Cámara (parámetros) | `eye` se mueve en una esfera alrededor del cazador; `fov` cambia al apuntar | Firma geométrica muy distintiva. |
| Cámara (matriz) | Buscador de bases ortonormales (filas unitarias y ortogonales) | Una matriz de vista siempre contiene una rotación pura; es raro que datos aleatorios cumplan eso. |
| Función de daño | Partir del sitio del cheat "Last Damage" (§3.3) + `lr`/stack capturados por el Tap → Ghidra hacia arriba por los `BL`. Confirmación con watchpoint GDB en el HP (**CPU JIT desactivado, obligatorio**) y búsqueda estática de `str #0x360` para rutas alternativas | La instrucción que escribe el HP está al final de la cadena de daño; subiendo se llega a la función que tiene todos los datos del golpe. El cheat ahorra la búsqueda del punto de partida. |
| Estabilidad | Escáner de cadenas de punteros desde la región estática + matriz de validación | Solo las cadenas que parten de `.data/.bss` sobreviven a reinicios. |

---

## 7. Matemática de proyección (lo esencial)

- **Vista:** `V = lookAt(eye, target, up=(0,1,0))`.
- **Proyección:** `P = perspective(fov_y, aspect = 400/240, near, far)`. Para proyectar texto, `near/far` solo afectan a `z`: no influyen en x/y.
- **Clip:** `c = P·V·[p, 1]`. Si `c.w ≤ ε` → detrás de la cámara → no dibujar.
- **NDC:** `n = c.xy / c.w`. **Píxel 3DS:** `sx = (n.x+1)/2·400`, `sy = (1−n.y)/2·240`.
- **Monitor:** `X = rect.x + sx·rect.w/400`, `Y = rect.y + sy·rect.h/240` (rect = pantalla superior según layout).
- **Si se usa la matriz real de GPU:** suele incluir rotación de 90° (pantallas verticales) → intercambiar/negar ejes tras detectar la orientación.
- **Escalado de resolución interna de Azahar (2×, 3×…):** **no afecta**, porque trabajamos en coordenadas normalizadas y mapeamos al rect.

---

## 8. Architecture Decision Records iniciales

- **ADR-0001 — Overlay externo en lugar de fork del emulador.** Motivo: no invasivo, mantenible, sin depender de un build propio de Azahar. Coste: tracking de ventana y lecturas asíncronas (mitigado).
- **ADR-0002 — Rust + wgpu.** Motivo: rendimiento predecible sin GC (sin tirones como en Python/PyQt, que usan los overlays existentes), un solo código para Metal/DX12/Vulkan, seguridad de memoria, buen valor de portfolio. Alternativas: C++/ImGui/GLFW (válida, menos segura), Swift nativo (solo macOS), Python/PyQt (rápido de prototipar pero jitter de GC/GIL y más CPU), Electron (demasiado pesado para un overlay).
- **ADR-0003 — Dos niveles Pasivo/Activo con la misma interfaz `DamageEvent`.** Motivo: entregar pronto y no bloquearse por el RE de la función de daño.
- **ADR-0004 — Proyección reconstruida desde parámetros de cámara.** Motivo: evita la ambigüedad de la rotación de la matriz de GPU y de su *layout* en memoria; la matriz queda como contingencia.
- **ADR-0005 (se escribe en F5.1)** — NSWindow de winit vs. NSPanel propio en macOS, según el resultado del spike.
- **ADR-0006 (se escribe en 2.18, se completa en 2.20/2.21)** — Modo Tap mediante parche de código por RPC; amplía la ADR-0003 a tres niveles y registra el GO/NO-GO del Tap y del plugin 3GX.

---

## 9. Stack tecnológico

| Área | Elección | Motivo |
|---|---|---|
| Lenguaje overlay | Rust stable (workspace) | Sin GC, rápido, seguro, multiplataforma |
| Ventana/eventos | `winit` (+ `objc2`, `objc2-app-kit`, `objc2-core-graphics` en macOS; `windows` en Windows) | Estándar de facto; acceso nativo donde haga falta |
| GPU | `wgpu` | Metal en macOS, DX12/Vulkan en Windows; alpha premultiplicado para transparencia |
| Matemática | `glam` | SIMD, API simple |
| Texto | `fontdue` (MVP) → atlas **MSDF** con `msdf-atlas-gen` (polish) | MSDF: nítido a cualquier escala, contorno y sombra en shader, 1 draw call |
| Concurrencia | `crossbeam-channel`, `triple_buffer` | Canal acotado para eventos; último snapshot sin locks |
| Config | `serde` + `toml`, `notify` | Perfiles y estilo con hot-reload |
| Logs/métricas | `tracing`, `tracing-subscriber` | Estructurado, niveles |
| CLI de RE | `clap`, `xxhash-rust` | Probe/scan/dump/record |
| Tests | `insta`, `proptest`, `criterion`, `dhat` | Snapshots de eventos, propiedades, benchmarks, allocations |
| Plugin | C++17, devkitARM, libctru, **CTRPluginFramework**, `3gxtool` | Toolchain estándar de plugins 3GX; clase `Hook` con modos MITM/WRAP_SUB |
| RE | Ghidra + `ghidra-ctr-loader` + `3ds-Ghidra-Scripts`; `arm-none-eabi-gdb` contra el GDB stub de Azahar; `mhdn-probe` | Gratis y suficiente; el probe trabaja con direcciones del *guest* directamente (a diferencia de Cheat Engine/Bit Slicer, que ven direcciones del host) |

---

## 10. Estilo visual previsto

| Tipo de golpe | Color | Tamaño | Extra |
|---|---|---|---|
| Normal | Blanco | 1.0× | Contorno negro 2 px |
| Alto (> p85 en modo pasivo) | Naranja | 1.25× | Pop mayor |
| Crítico (modo activo) | Amarillo | 1.3× | "!" y pop con muelle |
| Punto débil / hitzone ≥ 45 (activo) | Naranja intenso | 1.2× | — |
| Elemento (activo) | Color del elemento | 0.75×, secundario | Se dibuja pegado al número principal |
| Veneno / estado | Morado | 0.8× | Sin pop |
| Felyne / otros cazadores (activo) | Gris | 0.8× | Filtrable |

Animación: pop (90 ms) → subida con ease-out (≈ 0.6 s) → fade (350 ms); dispersión lateral aleatoria; apilado
anti-solapamiento para ráfagas; escala por distancia a la cámara.

---

## 11. Limitaciones conocidas y aceptadas

- Solo MHXX (3DS) en Azahar/Citra; MHGU (Switch) fuera de alcance.
- Sin 3D estereoscópico.
- Wayland no soportado (los overlays globales no se pueden posicionar); usar X11/XWayland.
- Modo pasivo: hits simultáneos agrupados y sin atribución de autor.
- En macOS, si el usuario cambia de layout con atajo sin que Azahar guarde la config, hace falta calibración manual.
- Multijugador como no-host: el daño puede no reflejarse con exactitud (limitación del propio juego).
