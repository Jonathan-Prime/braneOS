# TEST_PLAN.md — Brane OS

> Documento derivado de `PROJECT_MASTER_SPEC.md` §18.  
> Estado: **Activo**.  
> Última actualización: **2026-08-27**

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
- ACPI descubre RSDP/FADT y publica su estado de inicialización.
- La inicialización de subsistemas ocurre en orden correcto.
- El proceso init se crea exitosamente.

**Herramientas:** Scripts Python + QEMU con timeout, análisis de salida serial.

---

### 2.4 ACPI Tests (`tests/acpi/`)

**Objetivo:** Validar la transición de energía S3 y la recuperación del kernel.

**Cobertura:**
- El kernel publica S3 cuando el firmware lo anuncia en AML.
- QEMU entra en suspensión y emite el evento QMP `SUSPEND`.
- `system_wakeup` reactiva el kernel y produce el evento QMP `WAKEUP`.
- El trampoline FACS devuelve la CPU al kernel y restaura interrupciones.
- El teclado y `brsh` vuelven a responder después del resume.

**Herramientas:** Python 3, QEMU y QMP sobre socket Unix.

---

### 2.5 Security Tests (`tests/security/`)

**Objetivo:** Validar modelo de seguridad.

**Cobertura:**
- Denegación de operaciones sin capacidad.
- Intentos de escalamiento de privilegios.
- Solicitudes IA fuera de scope (deben fallar).
- Consistencia del audit log tras operaciones.
- Integridad de tokens de capacidad.

---

### 2.6 End-to-End Tests (`tests/e2e/`)

**Objetivo:** Validar escenarios completos.

**Escenario tipo:**
1. Se simula una anomalía.
2. La IA detecta la anomalía.
3. El decision planner genera una propuesta.
4. La política evalúa la propuesta.
5. La acción se ejecuta o se rechaza.
6. El evento queda auditado.

---

### 2.7 Stress y mutation-fuzz (`kernel/src/tests.rs`)

**Objetivo:** Detectar panics, corrupción de estado y violaciones de invariantes
con cargas grandes y reproducibles.

**Cobertura:**
- 25 000 entradas binarias mutadas sobre parsers FAT32, BDP y Brane Session.
- 10 000 roundtrips de paquetes válidos Brane Session y BDP.
- 50 000 operaciones del frame allocator contrastadas con un modelo de referencia.
- 256 ciclos de saturación, backpressure, drenaje FIFO y wraparound de IPC.

Las semillas son fijas: un fallo produce la misma secuencia en local y CI sin
dependencias externas ni acceso a hardware.

---

## 3. Herramientas

| Herramienta | Uso |
|------------|-----|
| `cargo test` | Unit + integration tests en Rust |
| Python 3 | Boot tests, e2e harnesses, log parsing |
| QEMU | Ejecución del sistema para boot/e2e tests |
| Shell scripts | Orquestación de ejecución |
| Generador xorshift determinista | Mutation-fuzz y stress reproducible |

---

## 4. Estado actual de CI

GitHub Actions valida en cada `push` y `pull_request` hacia `main`:

| Check | Estado | Comando |
|-------|--------|---------|
| Kernel build | Activo | `cargo build` debug + release para `x86_64-unknown-none` |
| Formatting | Activo | `cargo fmt --all -- --check` |
| Clippy | Activo | Kernel bare-metal + runner host con `-D warnings` |
| Kernel unit tests | Activo | `cargo test -p brane_os_kernel --lib` |
| **Stress y mutation-fuzz** | **Activo** | `make stress-test` (parsers, allocator e IPC) |
| **Release artifact (ISO UEFI)** | **Activo** | `make iso-test VERSION=ci` (ISO, checksum y boot con OVMF) |
| **Boot test (QEMU)** | **Activo** | `python3 tests/boot/test_boot.py` (kernel release, timeout 60 s, TCG) |
| **ACPI S3 test (QEMU/QMP)** | **Activo** | `make acpi-test` (suspend, wake y shell post-resume) |
| **ACPI MADT parser** | **Activo** | `cargo test -p brane_os_kernel --lib madt` (firma, checksum, límites, xAPIC/x2APIC, override LAPIC e I/O APIC) |
| **Security tests (QEMU)** | **Activo** | `make security-test` (capability denial + privilege escalation) |
| **Integration tests (QEMU)** | **Activo** | `make integration-test` (syscall/service + capability broker) |
| **E2E tests (QEMU)** | **Activo** | `make e2e-test` (disponibilidad de brsh + secuencia completa de boot) |

La validación local equivalente recomendada está documentada en
[`RUNBOOK.md`](RUNBOOK.md).

---

## 5. Convenciones

- Todo módulo nuevo debe incluir tests unitarios.
- Los tests de seguridad son obligatorios para cambios en política/capacidades.
- Parsers de datos no confiables deben incorporarse a la suite mutation-fuzz.
- Los tests boot, ACPI, security, integration y e2e se ejecutan en cada PR mediante QEMU/TCG.
- Todos los harnesses QEMU usan el kernel release y comparten una imagen por suite.

---

## 6. Próximos pasos

1. ~~Crear primer boot test automatizado con QEMU y timeout.~~ ✅ **Completado** (`tests/boot/test_boot.py` + CI job `boot-test`)
2. ~~Verificar banner serial, ACPI y prompt `brane>` desde CI.~~ ✅ **Completado** (cadenas requeridas: `"Brane OS"`, `"[acpi] ACPI subsystem initialized"`, `"brane>"`)
3. ~~Crear test de denegación de capability.~~ ✅ **Completado** (`tests/security/test_capability_denial.py` + `security_capability_tests` en `tests.rs`)
4. ~~Agregar pruebas e2e mínimas sobre `brsh`.~~ ✅ **Completado** (`tests/e2e/test_brsh_commands.py` + `test_full_boot_flow.py`)
5. ~~Integration tests: syscall → servicio, proceso → capability broker.~~ ✅ **Completado** (`tests/integration/` + `integration_syscall_tests` en `tests.rs`)
6. ~~Agregar jobs de CI para los harnesses Python (security, integration, e2e).~~ ✅ **Completado** (matriz `runtime-tests` en `.github/workflows/ci.yml`)
7. ~~Agregar suspensión/reanudación ACPI S3 automatizada.~~ ✅ **Completado** (`tests/acpi/test_suspend_resume.py` + QMP `SUSPEND`/`WAKEUP` + verificación de shell post-resume)
8. ~~Stress tests y fuzzing de componentes críticos.~~ ✅ **Completado** (`fuzz_tests` + `stress_tests`, semillas deterministas y check dedicado de CI)
9. Release v1.0: ISO booteable + documentación API publicada.

## 7. Make targets disponibles

| Target | Descripción |
|--------|-------------|
| `make test` | Unit tests en host (sin QEMU) |
| `make stress-test` | Mutation-fuzz de parsers + stress de allocator e IPC |
| `make iso-test` | Construye y arranca ISO UEFI con OVMF |
| `make release-test` | Valida ISO, checksum, archive y catálogo El Torito |
| `make test-image` | Compila una imagen compartida con el kernel release |
| `make boot-test` | Boot test del kernel release en QEMU/TCG (60 s) |
| `make acpi-test` | Suspensión/reanudación ACPI S3 en QEMU/QMP (120 s) |
| `make security-test` | Security tests en QEMU |
| `make integration-test` | Integration tests en QEMU |
| `make e2e-test` | E2E tests en QEMU |
| `make test-all` | Suite completa (unit → stress/fuzz → boot → ACPI → security → integration → e2e) |
| `make docs` | Genera API docs con `cargo doc` |
