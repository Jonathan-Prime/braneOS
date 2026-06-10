# TEST_PLAN.md — Brane OS

> Documento derivado de `PROJECT_MASTER_SPEC.md` §18.  
> Estado: **Activo**.

---

## 1. Estrategia general

Brane OS utiliza una estrategia de testing multinivel que cubre desde unidades aisladas hasta escenarios end-to-end. Las pruebas deben ser automatizables y ejecutables en CI desde las fases más tempranas.

---

## 2. Niveles de testing

### 2.1 Unit Tests (`tests/unit/`)

**Objetivo:** Validar componentes lógicos aislados.

**Cobertura:**
- Estructuras de datos del kernel (listas, colas, árboles).
- Scheduler (algoritmo de selección, prioridades).
- Parser de políticas del policy engine.
- Validador de capacidades.
- Safety filter (clasificación de riesgo).
- Componentes lógicos del decision planner.

**Herramientas:** `cargo test`, test modules en Rust (`#[cfg(test)]`).

---

### 2.2 Integration Tests (`tests/integration/`)

**Objetivo:** Validar interacción entre subsistemas.

**Cobertura:**
- Syscall → servicio del sistema.
- Proceso → capability broker → resultado.
- AI agent → policy engine → aprobación/denegación.
- Capability broker → audit service → registro.
- Inicialización secuencial de servicios.

**Herramientas:** Tests de integración en Rust, Python harnesses.

---

### 2.3 Boot Tests (`tests/boot/`)

**Objetivo:** Validar arranque del sistema.

**Cobertura:**
- El kernel arranca sin panic.
- Los logs seriales contienen el banner esperado.
- La inicialización de subsistemas ocurre en orden correcto.
- El proceso init se crea exitosamente.

**Herramientas:** Scripts Python + QEMU con timeout, análisis de salida serial.

---

### 2.4 Security Tests (`tests/security/`)

**Objetivo:** Validar modelo de seguridad.

**Cobertura:**
- Denegación de operaciones sin capacidad.
- Intentos de escalamiento de privilegios.
- Solicitudes IA fuera de scope (deben fallar).
- Consistencia del audit log tras operaciones.
- Integridad de tokens de capacidad.

---

### 2.5 End-to-End Tests (`tests/e2e/`)

**Objetivo:** Validar escenarios completos.

**Escenario tipo:**
1. Se simula una anomalía.
2. La IA detecta la anomalía.
3. El decision planner genera una propuesta.
4. La política evalúa la propuesta.
5. La acción se ejecuta o se rechaza.
6. El evento queda auditado.

---

## 3. Herramientas

| Herramienta | Uso |
|------------|-----|
| `cargo test` | Unit + integration tests en Rust |
| Python 3 | Boot tests, e2e harnesses, log parsing |
| QEMU | Ejecución del sistema para boot/e2e tests |
| Shell scripts | Orquestación de ejecución |

---

## 4. Estado actual de CI

GitHub Actions valida en cada `push` y `pull_request` hacia `main`:

| Check | Estado | Comando |
|-------|--------|---------|
| Kernel debug build | Activo | `cargo build -p brane_os_kernel --target x86_64-unknown-none` |
| Kernel release build | Activo | `cargo build -p brane_os_kernel --target x86_64-unknown-none --release` |
| Formatting | Activo | `cargo fmt --all -- --check` |
| Kernel clippy | Activo | `cargo clippy -p brane_os_kernel --target x86_64-unknown-none -- -D warnings` |
| Runner clippy | Activo | `cargo clippy -p runner --all-targets -- -D warnings` |
| Kernel unit tests | Activo | `cargo test -p brane_os_kernel --lib` |

La validación local equivalente recomendada está documentada en
[`RUNBOOK.md`](RUNBOOK.md).

---

## 5. Convenciones

- Todo módulo nuevo debe incluir tests unitarios.
- Los tests de seguridad son obligatorios para cambios en política/capacidades.
- Los boot tests deberán ejecutarse en cada PR cuando exista el harness QEMU.
- Los e2e tests se ejecutan antes de cada release.

---

## 6. Próximos pasos

1. Crear primer boot test automatizado con QEMU y timeout.
2. Verificar banner serial y prompt `brane>` desde CI.
3. Crear test de denegación de capability.
4. Agregar pruebas e2e mínimas sobre `brsh`.
