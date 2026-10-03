# PLAN_HIT.md — Punto exacto de contacto para los números de daño

> Plan posterior a `PLAN.md`. Las fases 0–6 del plan original están terminadas y no se reabren aquí.
> Este documento empieza por el comportamiento que se puede publicar hoy y organiza el trabajo de
> reverse engineering necesario para que cada número nazca en el punto real donde el arma toca al monstruo.

---

## 0. Objetivo y criterio de éxito

**Objetivo principal:** obtener, para cada `DamageEvent`, una posición mundial `vec3` finita que represente
el contacto arma/monstruo y usarla como `Anchor::World`. El resultado esperado es un número que nazca en la
cabeza, cola, ala, pata u otra zona realmente golpeada, al estilo HunterPie, y no en un punto genérico del
monstruo.

El trabajo se divide en tres rutas, en orden:

1. **Hipótesis A — contacto del efecto visual:** encontrar en el contexto del golpe el `vec3` empleado para
   generar la chispa, sangre o efecto de impacto.
2. **Hipótesis B — parte y hueso:** usar `DamageEvent.part_hp` para identificar la parte dañada y resolver la
   matriz mundial del hueso asociado.
3. **Contingencia de producto:** si A y B no son fiables, mantener como comportamiento definitivo el ancla
   cercana al cazador descrita en la Fase 0.

### Regla de precedencia del ancla

La selección debe respetar siempre este orden:

1. Un `Anchor::World(pos)` recibido del Tap gana si sus tres componentes son finitos.
2. Sin contacto válido, `style.anchor = "hunter"` usa el ancla cercana al cazador.
3. `style.anchor = "monster"` conserva el comportamiento anterior: posición del monstruo más la altura de
   especie sobre el cuerpo.

Nunca se debe proyectar un `vec3` con `NaN`, infinito o valores fuera del rango plausible del área. Un contacto
inválido cae al estilo configurado sin perder el evento de daño.

---

## 1. Hechos medidos que no se deben regresar

Estas observaciones ya fueron verificadas en vivo y son restricciones del diseño:

- El Tap en `0x008D03E8` registra el daño y un puntero al slot del monstruo.
- `sp[2]` **no es un hueso** ni un punto de contacto. Coincide con entradas de la tabla de slots; reutilizar
  `sp[2] + 0x40` como posición produce el mismo punto para monstruos diferentes.
- La posición del monstruo está en `HP - 0x320`, es un `vec3` vivo y debe seguir leyéndose.
- Los pies del cazador local ya están en el perfil: `[hunter] base = 0x0814E620`, posición `+64`. El valor ya
  llega como `Snapshot.hunter_pos`.
- `DamageEvent.part_hp` ya existe y conserva la pista necesaria para la ruta de partes.
- Una cacería se detecta por una lista de monstruos resuelta y no vacía. La palabra `in_quest` no es fiable.
- El cliente UDP debe descartar respuestas atrasadas antes de aceptar la respuesta de la petición actual;
  una respuesta con ID viejo nunca puede contaminar el siguiente muestreo.
- El overlay se oculta cuando otra aplicación está en primer plano.
- Para aparecer en pantalla completa nativa de macOS, la aplicación debe arrancar con política de activación
  `Accessory`.
- Bajo macOS Game Mode, los `sleep` de un hilo de presentación en segundo plano llegan aproximadamente
  **116 ms tarde**. El muestreo que alimenta la presentación se hace mediante `Pump`; no se vuelve a depender
  de esos `sleep` para la cadencia visual.

Toda propuesta que contradiga uno de estos hechos es **NO-GO** hasta aportar una medición reproducible que lo
refute.

---

## 2. Convenciones de trabajo

- Una fase de RE produce evidencia antes de tocar el camino vivo.
- Todo candidato a posición se valida en varias especies, partes y sesiones.
- Los spikes pueden añadir captura diagnóstica, pero no reemplazan silenciosamente el formato activo del Tap.
- Un cambio se divide en commits pequeños y verificables con Conventional Commits.
- Los datos capturados deben ser numéricos; no se versionan dumps de código o contenido del juego.
- Cada fase termina con una decisión explícita **GO/NO-GO**, evidencia enlazable y una Definition of Done.
- Las pruebas automatizadas usan estructuras sintéticas o trazas numéricas; las pruebas en vivo se marcan y
  documentan por separado.

### Métricas de aceptación

- El número usa el contacto real en al menos el 95 % de golpes directos cubiertos por la ruta elegida.
- Error visual mediano del ancla exacta: ≤ 25 unidades de juego respecto al efecto de impacto observado.
- No aparecen posiciones no finitas, saltos al origen mundial ni anclas de otro monstruo.
- `Σ DamageEvent.amount` no cambia por añadir contexto de contacto.
- Cero eventos perdidos o duplicados respecto al Tap actual en una sesión de 10 minutos.
- Sin regresiones en desconexión RPC, cambio de escena, foco de ventanas o pantalla completa.

---

## FASE 0 — Ancla cercana al cazador, comportamiento publicable

**Estado:** en implementación.  
**Objetivo:** ofrecer una ubicación legible y honesta mientras no existe el punto exacto de contacto.

### Comportamiento

Con `style.anchor = "hunter"` —valor por defecto— el número nace:

1. desde los pies del cazador local (`Snapshot.hunter_pos`);
2. sobre la línea horizontal que apunta al monstruo golpeado;
3. como máximo a **120 unidades de juego** delante del cazador, aproximadamente **1,2 m**;
4. sin pasar nunca de la mitad de la distancia horizontal cazador–monstruo;
5. a **130 unidades** sobre los pies del cazador.

En términos geométricos, usando solo XZ para la dirección:

```text
to_monster_xz = monster.pos.xz - hunter_pos.xz
forward = normalize(to_monster_xz)
distance = min(120, length(to_monster_xz) / 2)
anchor = hunter_pos + (forward.x, 0, forward.y) * distance + (0, 130, 0)
```

Si la distancia XZ es cero, no es finita o falta `hunter_pos`, se usa el ancla del monstruo. Antes de aplicar
esta regla, un `Anchor::World` finito procedente del Tap conserva prioridad absoluta.

`style.anchor = "monster"` selecciona explícitamente el comportamiento anterior: posición viva del monstruo
(`HP - 0x320`) más la altura de ancla de su especie/tamaño sobre el cuerpo.

### Método y archivos previstos

- Configuración: esquema y valor por defecto de `style.anchor`.
- Modelo/aplicación: función pura que resuelva `World → hunter → monster`.
- Documentación: guía de usuario y limitaciones, sin afirmar que existe una hitzone exacta.
- No se cambia el Tap ni el formato de su ring buffer en esta fase.

Archivos previstos al implementar el código:

- `crates/mhdn-app/src/config.rs`
- `crates/mhdn-app/src/run.rs`
- `crates/mhdn-app/src/numbers.rs`
- pruebas unitarias junto a la función de resolución correspondiente

### Commits previstos

| # | Commit | Contenido | Verificación |
|---|---|---|---|
| 0.1 | `feat(app): add configurable damage anchor style` | Añadir `"hunter"` y `"monster"`; `"hunter"` es el valor por defecto y valores desconocidos producen un error claro o un fallback documentado. | Tests de parseo, default y compatibilidad con config existente. |
| 0.2 | `feat(app): anchor damage numbers near local hunter` | Resolver la línea hacia el objetivo, límite 120, límite de mitad y altura 130; conservar prioridad de `Anchor::World` finito. | Tests geométricos de objetivo cercano, lejano, coincidente, ausente y no finito. |
| 0.3 | `test(app): cover damage anchor fallbacks` | Cubrir falta de cazador, monstruo desaparecido, contacto no finito y modo `"monster"`. | Ningún evento se pierde; toda salida usada para proyección es finita. |
| 0.4 | `docs: describe interim damage number anchors` | Explicar el comportamiento real en README/guía y eliminar afirmaciones de hitzone exacta. | Revisión manual de documentación y captura de ambos modos. |

### Pruebas en vivo

- Golpear desde contacto, a distancia y con el monstruo moviéndose: el número nunca pasa de la mitad.
- Probar monstruos pequeños y grandes: el tope sigue siendo 120 unidades.
- Rotar la cámara: el punto permanece en el mundo y se reproyecta correctamente.
- Cambiar a `"monster"` y confirmar la altura de especie anterior.
- Inyectar un `Anchor::World` finito de prueba y confirmar que gana.

### GO/NO-GO

- **GO:** el ancla permanece cerca del cazador, nunca cruza al monstruo, soporta datos ausentes y no cambia
  la contabilidad del daño.
- **NO-GO:** aparece detrás del cazador sin razón, pasa de 120 unidades o de la mitad, proyecta valores no
  finitos, o anula un `Anchor::World` válido.

**Definition of Done F0:** `"hunter"` es el default publicable, `"monster"` conserva la opción anterior,
la precedencia está probada y la documentación describe con precisión que todavía no es el contacto exacto.

---

## FASE 1 — Captura diagnóstica y búsqueda del contacto visual

**Hipótesis A:** el pipeline que crea el efecto de golpe recibe o construye un `vec3` de contacto que sigue
vivo en registros, stack o estructuras accesibles cuando se aplica el daño.

**Regla de seguridad:** no se modifica el hook vivo hasta que un spike aislado termine en **GO**.

### Método

1. Mapear callers del Tap y registrar, por evento, más contexto inmediato: registros preservados, una ventana
   acotada del stack, `lr` y punteros plausibles.
2. Agrupar capturas por caller y tipo de daño para distinguir golpes directos, proyectiles, bombas y ticks.
3. Buscar tripletas de `f32` finitas y plausibles:
   - cerca de la posición del monstruo;
   - dentro de un radio coherente con su tamaño;
   - distintas al centro del monstruo y al cazador;
   - variables entre golpes a cabeza, cola y patas.
4. Seguir hacia arriba cada caller en desensamblado hasta la rutina que genera el hit spark. Identificar
   copia, transformación o argumento de posición.
5. Correlacionar cada candidato con vídeo/captura cuadro a cuadro y con golpes deliberados a partes separadas.
6. Solo después de validar el campo, diseñar el formato mínimo de captura definitivo y evaluar su coste.

La búsqueda empieza en contexto ya existente; no se asume que cualquier puntero alineado sea válido. Toda
desreferencia diagnóstica debe validar rango, alineación y legibilidad antes de leer.

### Archivos previstos

- `crates/mhdn-probe/src/` para comandos de captura y análisis del spike
- `crates/mhdn-game/src/tap/` para un stub experimental separado, solo si hace falta
- `docs/RE_NOTES.md` para direcciones, firmas, sesiones y descartes
- `tests/traces/` para capturas numéricas reducidas, si son aptas para versionar

El hook y stub usados por la aplicación permanecen intactos durante el spike.

### Commits previstos

| # | Commit | Contenido | Verificación |
|---|---|---|---|
| 1.1 | `feat(probe): capture extended damage caller context` | Captura diagnóstica versionada con registros, `lr` y ventana acotada de stack; límites estrictos. | Decodificación roundtrip y ausencia de lecturas fuera de rango. |
| 1.2 | `feat(probe): rank contact vec3 candidates` | Analizador offline que filtra tripletas finitas y las compara con cazador/monstruo. | Dataset sintético recupera el candidato conocido y rechaza NaN/origen. |
| 1.3 | `docs(re): map hit effect callers and candidates` | Tabla por caller, arma, parte y candidato; incluye hipótesis descartadas. | Tres sesiones y al menos tres especies con resultados reproducibles. |
| 1.4 | `spike(game): validate hit spark contact capture` | Spike aislado que publica el candidato sin sustituir el Tap vivo. | Contacto acompaña visualmente el efecto y no altera daño/eventos. |
| 1.5 | `feat(game): publish validated world contact from tap` | **Solo tras GO:** ampliar el Tap vivo con el campo mínimo y producir `Anchor::World` finito. | Compatibilidad de versión, cero pérdidas y fallback automático. |

### Pruebas

- Unitarias: validación de `vec3`, rangos, versión del registro y fallback.
- Replay: eventos viejos sin contacto siguen funcionando.
- En vivo: cabeza/cola/pata de ≥ 3 especies; arma cuerpo a cuerpo y proyectil; 10 minutos de multihit.
- Conservación: suma, orden y objetivo de eventos iguales antes y después.
- Seguridad: build/fingerprint incorrecto se niega a instalar; contexto desconocido usa F0.

### GO/NO-GO

- **GO de spike:** un mismo campo o cadena estable coincide con el efecto visual en ≥ 95 % de golpes directos,
  funciona tras reiniciar el juego y no depende de una dirección heap fija.
- **NO-GO:** el candidato es centro del monstruo, cambia de significado por caller, solo funciona en una
  especie/arma, llega demasiado tarde o desestabiliza el Tap.

Si el spike da NO-GO, no se toca el hook vivo y se pasa a Fase 2.

**Definition of Done F1:** existe evidencia reproducible y una decisión registrada. Si hay GO, el Tap entrega
un `Anchor::World` validado con fallback; si hay NO-GO, el producto sigue exactamente con F0.

---

## FASE 2 — Parte dañada → hueso → matriz mundial

**Hipótesis B:** `DamageEvent.part_hp` permite identificar la parte golpeada; la parte referencia un hueso o
índice cuyo transform mundial ofrece un ancla suficientemente próxima al contacto.

Esta ruta produce el centro animado de la parte, no necesariamente el punto superficial exacto. Se acepta
solo si mejora de forma clara y estable el ancla de F0.

### Método

1. Registrar `part_hp` antes/después junto con objetivo, daño, caller y parte golpeada manualmente.
2. Identificar pools de stagger/part HP dentro de la estructura del monstruo y construir la relación:
   `part_hp observado → entrada de parte`.
3. Localizar en la entrada de parte un ID/índice de hueso o un enlace a hitbox.
4. Resolver la jerarquía de esqueleto desde el objeto del monstruo:
   `part → bone index → local transform → world matrix`.
5. Extraer la traslación mundial y validar que sigue animaciones reales: cabeza girando, cola moviéndose,
   ala batiendo y parte rota.
6. Diseñar una tabla por especie solo si la estructura común no basta. Las excepciones deben ser datos de
   perfil verificables, no heurísticas ocultas.
7. Si varias partes comparten `part_hp` o un hueso, registrar la ambigüedad y usar F0 en vez de inventar una
   precisión inexistente.

### Archivos previstos

- `crates/mhdn-probe/src/` para inspección de partes, listas y matrices
- `crates/mhdn-game/src/` para resolución segura de parte/hueso
- `profiles/mhxx-jp-v1.4-es.toml` para offsets validados, si son comunes al build
- `docs/RE_NOTES.md` para evidencia y matriz especie/parte
- tests de modelo con memoria sintética y trazas numéricas

### Commits previstos

| # | Commit | Contenido | Verificación |
|---|---|---|---|
| 2.1 | `feat(probe): correlate part hp with monster parts` | Capturar cambios de `part_hp` y buscar entradas de parte asociadas. | Golpes repetidos a la misma parte siguen el mismo pool; cambiar de parte cambia de entrada. |
| 2.2 | `docs(re): map part entries to skeleton bones` | Documentar estructuras, índices y transformaciones para varias especies. | ≥ 3 especies con morfologías distintas y reinicio entre sesiones. |
| 2.3 | `feat(game): resolve monster bone world transforms` | Resolver matrices con validación de punteros, índice y componentes finitos. | Tests con jerarquías sintéticas, escalas y matrices inválidas. |
| 2.4 | `feat(game): anchor part damage on resolved bones` | Convertir el hueso validado en `Anchor::World`; ambiguo o inválido cae a F0. | Replay compatible y cero saltos al hueso de otro monstruo. |
| 2.5 | `test(game): validate part anchors across species` | Suite de regresión por especie/parte y prueba en vivo prolongada. | ≥ 90 % de golpes probados quedan sobre la parte correcta. |

### Pruebas

- Cabeza, cola y dos extremidades en al menos tres especies.
- Parte intacta, tambaleada y rota.
- Monstruo quieto y durante animaciones amplias.
- Dos monstruos de la misma especie simultáneos para detectar cruces de identidad.
- Desaparición/reaparición de slot y reutilización de estructuras.
- Matrices no finitas, índices fuera de rango y huesos ausentes siempre caen a F0.

### GO/NO-GO

- **GO:** la relación parte→hueso es estable entre sesiones y especies probadas, sigue la animación y coloca
  ≥ 90 % de muestras sobre la parte correcta sin regresiones de eventos.
- **NO-GO:** `part_hp` no identifica una parte de forma unívoca, la jerarquía cambia de forma no perfilable,
  las matrices no son mundiales o el centro del hueso resulta visualmente peor que F0.

**Definition of Done F2:** la ruta parte/hueso está integrada detrás de validación estricta o queda descartada
con evidencia. Nunca se presenta como “contacto exacto” si solo representa el centro de una parte.

---

## FASE 3 — Cerrar investigación y fijar el comportamiento de producto

**Condición de entrada:** Hipótesis A y B terminaron en NO-GO, o su cobertura/fiabilidad no alcanza los
criterios de aceptación.

**Objetivo:** convertir el ancla cercana al cazador en el comportamiento definitivo y documentar el límite
sin dejar deuda experimental activa en el camino de producción.

### Método

- Conservar `style.anchor = "hunter"` como default y `"monster"` como alternativa.
- Retirar o aislar formatos diagnósticos que no aporten valor al producto.
- Mantener en `RE_NOTES.md` los candidatos descartados para no repetir búsquedas.
- Actualizar toda afirmación pública: el overlay muestra daño cerca del cazador, no en la hitzone exacta.
- Crear una decisión técnica breve con las pruebas y el motivo del cierre.
- Dejar una futura reapertura condicionada a evidencia nueva concreta: símbolo, caller, estructura o captura
  que exponga un contacto estable.

### Archivos previstos

- `docs/RE_NOTES.md`
- `docs/PLAN_HIT.md`
- `docs/GUIA.md`
- `README.md` y notas de release, solo para corregir expectativas
- ADR de cierre si el repositorio mantiene esa convención

### Commits previstos

| # | Commit | Contenido | Verificación |
|---|---|---|---|
| 3.1 | `docs(re): record contact anchor no-go results` | Evidencia de A/B, cobertura y razones del cierre. | Otro desarrollador puede reproducir cada descarte. |
| 3.2 | `refactor(game): remove unused contact diagnostics` | Solo si quedaron rutas experimentales en producción; preservar parsers necesarios para compatibilidad. | Replays y eventos idénticos antes/después. |
| 3.3 | `docs: establish hunter-near anchor as product behavior` | Guía, README y limitaciones coherentes. | Búsqueda global no encuentra afirmaciones de hitzone exacta. |

### Pruebas

- Regresión completa de F0.
- Configuraciones antiguas siguen cargando.
- La aplicación no espera campos experimentales para mostrar eventos.
- Revisión manual de todos los textos públicos y de release.

### GO/NO-GO

- **GO:** el comportamiento de F0 es estable, comprensible y está descrito sin exageraciones.
- **NO-GO:** queda alguna ruta que puede ocultar números, documentación que promete contacto exacto o datos
  experimentales obligatorios para ejecutar.

**Definition of Done F3:** se publica una experiencia honesta y estable cerca del cazador; A y B quedan
cerradas con evidencia y sin bloquear futuras investigaciones.

---

## FASE 4 — Entrega de producto por otros responsables

**Propiedad:** esta fase pertenece a otras personas/equipos. Este plan define resultados y validación, no
asigna archivos ni prescribe cambios de código.

### Líneas de entrega

1. **Windows beta overlay**
   - Paquete descargable y marcado claramente como beta.
   - Prueba en un PC Windows real antes de retirar la advertencia.
   - Verificación de foco, seguimiento de ventana, escalado DPI y cierre limpio.

2. **Aplicación universal de macOS**
   - `.app` universal para Apple Silicon e Intel.
   - Apertura sin Terminal y flujo claro de Gatekeeper.
   - Verificación en ventana y pantalla completa nativa, incluida la política `Accessory` y el muestreo
     mediante `Pump` bajo Game Mode.

3. **GitHub Releases**
   - Artefactos de macOS y Windows en la release más reciente.
   - Checksums y notas con compatibilidad/limitaciones reales.
   - No publicar una versión como estable basándose solo en compilación cruzada.

4. **Guía bilingüe de instalación**
   - Español primero e inglés después.
   - Configuración mínima de Azahar, estados del título, cierre y solución de problemas.
   - Debe afirmar que los números aparecen cerca del cazador hasta que una fase anterior entregue contacto
     real validado.

### Hitos y verificación

| # | Entrega | Verificación |
|---|---|---|
| 4.1 | Beta de Windows disponible | Instalación y misión completa en hardware Windows real; limitaciones visibles. |
| 4.2 | `.app` universal de macOS disponible | Abre en Intel y Apple Silicon sin Terminal; overlay correcto en fullscreen. |
| 4.3 | Release de GitHub completa | Descargas, checksums, notas y guía apuntan a los artefactos correctos. |
| 4.4 | Guía bilingüe revisada | Un usuario no desarrollador instala y cierra la app sin consultar el repositorio. |

### GO/NO-GO

- **GO:** cada paquete fue probado en su plataforma, la release contiene los artefactos correctos y la guía
  no promete funciones todavía inexistentes.
- **NO-GO:** Windows no se probó en PC real, la app de macOS requiere Terminal, faltan artefactos o la
  documentación afirma contacto exacto, filtro de golpes propios, críticos o elemento sin soporte.

**Definition of Done F4:** beta de Windows, `.app` universal de macOS, GitHub Release y guía bilingüe están
disponibles, probados por sus responsables y alineados con las capacidades reales del producto.

---

## 3. Matriz final de decisión

| Resultado F1 | Resultado F2 | Comportamiento final |
|---|---|---|
| GO | No necesaria | Contacto visual exacto de Hipótesis A; fallback a F0. |
| NO-GO | GO | Centro mundial de la parte/hueso, descrito como aproximación; fallback a F0. |
| GO parcial | GO parcial | Elegir por tipo de evento solo con reglas explícitas y medibles; fallback a F0. |
| NO-GO | NO-GO | Ancla cercana al cazador de F0 como producto definitivo. |

En todos los casos, `Anchor::World` finito conserva prioridad, los eventos de daño no se pierden por falta de
posición y la documentación pública refleja exactamente el nivel de precisión alcanzado.
