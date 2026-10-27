# Punto 3D de la hitzone

Title `0x0004000000197100`, perfil `mhxx-jp-v1.4-es` (title version 4224). No hay volcado ni código del juego aquí. El dibujo y el medidor no cambian.

`mon` es la palabra de vida. En un `dmg` emparejado, `mon=` es `MonsterKey.struct_addr`, esa palabra. El objeto del gancho es otra dirección: `objeto = mon - 0x360`.

## Hecho (escrito en este repo, con traza)

| Qué | Dónde | Traza |
| --- | --- | --- |
| Tap del golpe | `0x008D03E8` `ldr r12, [r3, #0xA8]`; vida en `r12 + 0x360` | `docs/RE_NOTES.md` §6, perfil `[damage_tap]` |
| Cave / ring | `0x00BF2D00` (768 bytes), `0x00D32000` (256 bytes) | misma sección |
| `lr` del golpe | `0x008BA260` | `docs/evidence/00-traces.md`, `03a-damage-paths.md` |
| `lr` derribo de montura | `0x008BA870` | las mismas |
| `lr` de estados | `0x008BA214` | las mismas |
| Centro del monstruo | `vec3` f32 en `mon - 0x320` (12 bytes) | `docs/RE_NOTES.md` §3, tres sesiones; perfil `pos.off = -800` |
| Cadena hasta `mon` | `0x00D3A8E0` → `+0x14` (stride 4) → `+0x10A8` → `+0x360` | `docs/RE_NOTES.md` §3–4 |
| `r3` en el tap | `r3 == mon - 0x418` en el 100 % de los taps | `docs/evidence/00-traces.md` |
| `sp[2]` | entrada de la tabla de slots, no un hueso. `sp[2] + 0x40` dio el mismo punto para monstruos distintos | corrección 2026-10-02 en `docs/RE_NOTES.md` §7 |
| `sp[1]` | pool de parte / stagger, entero, no un `vec3` | misma sección, sesión de 31 golpes |
| Atacante | no está en `sp[0..4]` ni en `r3` | `docs/evidence/00-traces.md` |
| Cámara | `*(u32*)0x0814CACC` es la base. Ojo `+0x40` (12 bytes), blanco `+0x60` (12 bytes), `fov_y` `+0x3C` (f32, grados, 50) | `docs/RE_NOTES.md` §5, perfil `[camera]`, tres arranques |
| Pies del cazador | `*(u32*)0x0814E620` luego `+0x40` (12 bytes) | `docs/RE_NOTES.md` §5 |

No hay, en el repo, una dirección de matriz view ni projection. La proyección se reconstruye: `look_at` con arriba `(0, 1, 0)` y perspectiva `fov_y` con aspecto `400/240` (`docs/adr/0004-projection-from-camera-params.md`). `0x08379E2C` también apuntó a la base de la cámara en las notas; el perfil lee `0x0814CACC`.

Algunas quests online no pasan por `0x008D03E8` (`source=passive`, frames 0–2). Este punto se busca en el camino que sí registra el golpe (`source=tap`). Eso no dice que el camino online no tenga el campo.

## Medido, y no es un XYZ

El byte en `mon + 0x5958` es un índice de zona. Elige una halfword de la tabla en `mon + 0x468`:

```text
zona   = *(u8*)(mon + 0x5958)
valor  = *(u16*)(mon + 0x468 + 2 * zona)
```

Ese valor dice qué zona (cabeza, pata). No es una coordenada. La base es `mon`, la misma de `species` en `mon + 0x5A18` y de `poison` en `mon + 0x54E4` (`docs/RE_NOTES.md`: los offsets de `[monster]` cuelgan de la palabra de vida, no de `objeto`).

## Fuentes públicas (no son medidas de este binario)

- [Setsu-BHMT/MHXX-Monster-Info-NTR-Plugin](https://github.com/Setsu-BHMT/MHXX-Monster-Info-NTR-Plugin) `source/monster.h`, MHXX v1.1. La vida está a `+0x1418` de su `Monster*`. Respecto de esa vida, el tamaño cae en `-0x1B0`, el byte de visible en `-0x1408` y el veneno en `+0x54E4`. Esos tres coinciden con el perfil de v1.4-es. Las 8 partes empiezan en vida `+0x46`, 12 bytes cada una (stagger y break). No hay un `vec3` de parte. Dos punteros en `Monster* + 0x14` (vida `-0x1404`); el autor no dice que sean huesos. El plugin avisa de que la barra de parte no cubre todas las zonas.
- [EXTER7/MHGEN-Monster-HP-NTR-Plugin](https://github.com/EXTER7/MHGEN-Monster-HP-NTR-Plugin) `source/main.c` es MHGen, otro title. Vida en `+0x1318`, partes también en vida `+0x46`. No se copia a este binario.
- [ponpoko094/Colorful-MHX3gx](https://github.com/ponpoko094/Colorful-MHX3gx) es MHX JP `0004000000155400`. El centro del monstruo está en `+0xFF8`, que es vida `+0x1318` menos `0x320`. Mismo desplazamiento que `mon - 0x320` aquí. Es el centro, no la hitzone. El cazador de ese juego usa `+0x40`, igual que los pies de este perfil.
- [Silvris/MH-Tools-and-Scripts](https://github.com/Silvris/MH-Tools-and-Scripts) `MHGU MOD.bt` describe huesos y matrices de un archivo de modelo. Son offsets de fichero, no de la RAM del invitado.
- mhff nombra tipos `rCnsJointOffset`, `rCnsMatrix` y `rCollision` para XX/X. No publica el layout en RAM.

Ninguna de esas fuentes da, para este title y esta versión, el puntero al hueso ni la traslación mundial de la zona golpeada.

## Hipótesis

El `vec3` de la parte no está en la halfword de `mon + 0x468`, ni en `sp[2] + 0x40`, ni en el centro `mon - 0x320`.

La hipótesis es que el juego deja el contacto, o la traslación del hueso de esa zona, en los 12 bytes alineados que siguen al índice. En el tap (`lr = 0x008BA260`, quest offline, un grande):

```text
leer 16 bytes en mon + 0x5958
byte 0        índice de zona (el u8 ya medido)
bytes 4..15   candidato XYZ, tres f32 en mon + 0x595C
```

Se sostiene solo si, con la cámara quieta, cabeza y cola cambian el byte y cambian esos tres floats, los floats son finitos, y no son el centro (`mon - 0x320`). Si son el centro, no son floats, o no cambian con la parte, la hipótesis cae. No hay un segundo candidato público.

La cámara para proyectar ese punto, si el candidato vale, ya está leída: base `*(u32*)0x0814CACC`, ojo `+0x40`, blanco `+0x60`, `fov_y` `+0x3C`. No hace falta una matriz view en RAM para esta prueba.

## Cómo confirmarlo sin depurador

Una línea nueva de diag, solo en `source=tap` y `lr=0x008BA260`:

```text
hit_xyz zone=<u8 mon+0x5958> x,y,z=<f32 mon+0x595C> mon_pos=<f32 mon-0x320>
```

Quest offline, un grande, sin mover la cámara: tres golpes a la cabeza y tres a la cola. `mon_pos` tiene que quedarse en el cuerpo. `hit_xyz` tiene que separarse de `mon_pos` y cambiar entre cabeza y cola. Veneno (`0x008BA214`) y derribo (`0x008BA870`) no entran: no son un punto de arma.

Si no se puede añadir la línea: watchpoint de 4 bytes sobre `mon + 0x5958`, en esa quest offline, al pasar de la cabeza a la cola. No está ejecutado.
