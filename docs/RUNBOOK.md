# RUNBOOK.md - Build, CI y ejecucion local

> Estado: operativo para desarrollo local. Última actualización: 2026-08-27.
> Este documento describe el flujo
> actual del repositorio, no el release final instalable.

---

## 1. Que se puede ejecutar hoy

Brane OS ya puede compilar el kernel `x86_64-unknown-none`, generar imagenes
BIOS/UEFI con el crate `bootloader` y arrancar en QEMU mediante el runner Rust.

El arranque actual inicializa:

- serial y framebuffer,
- GDT, IDT y PIC,
- memoria, paging, heap y ACPI,
- scheduler cooperativo,
- syscalls e IPC,
- capacidades, auditoria y loader de modulos,
- Brane Protocol, IA observadora, VFS, RamFS, TTY, shell, red, sockets y DNS.

El sistema entra en `brsh` y queda esperando entrada por TTY.

---

## 2. Requisitos

### Herramientas

- Rust nightly fijado a `nightly-2026-03-11` en `rust-toolchain.toml`; no hace
  falta cambiar el toolchain global con `rustup default nightly`.
- Componentes Rust: `rust-src`, `llvm-tools-preview`, `rustfmt`, `clippy`.
- QEMU con `qemu-system-x86_64`.
- `xorriso` y OVMF para construir/probar la ISO UEFI (`brew install xorriso`
  en macOS; `apt install xorriso ovmf` en Debian/Ubuntu).
- `make`.

### Instalacion rapida en macOS

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup toolchain install nightly
rustup component add rust-src llvm-tools-preview rustfmt clippy --toolchain nightly
brew install qemu
```

### Verificacion de herramientas

```bash
rustup show
cargo --version
qemu-system-x86_64 --version
make help
```

---

## 3. Compilar

```bash
make build
```

Equivalente directo usado por CI:

```bash
cargo build -p brane_os_kernel --target x86_64-unknown-none
```

Build release:

```bash
make build-release
```

Equivalente directo usado por CI:

```bash
cargo build -p brane_os_kernel --target x86_64-unknown-none --release
```

---

## 4. Ejecutar en QEMU

```bash
make run
```

Este target hace tres cosas:

1. Compila el kernel debug.
2. Usa el crate `bootloader` para crear imagenes de disco BIOS y UEFI.
3. Arranca QEMU con la imagen BIOS por compatibilidad.

La ejecucion es interactiva y queda en primer plano. Para detenerla durante
desarrollo, use `Ctrl-C` en la terminal que lanzo `make run`.

Build release + QEMU:

```bash
make run-release
```

Build y validación de la ISO UEFI:

```bash
make iso-test VERSION=dev
```

El target genera `dist/brane_os_v<VERSION>.iso`, su checksum SHA-256 y un
archivo `.tar.gz`, y luego arranca la ISO con OVMF mediante `-cdrom`.

### Salida esperada

En el log serial deberia aparecer:

```text
Brane OS v0.1 — Kernel Booting
...
Brane OS v0.1 — Boot Complete
...
Welcome to Brane OS v0.1
Type 'help' for available commands.
brane>
```

---

## 5. Validacion local

Antes de abrir un PR, ejecute:

```bash
cargo fmt --all -- --check
make clippy
make test-all
make release-test VERSION=dev
```

`make clippy` valida:

- kernel bare-metal con `-D warnings`,
- runner host con `-D warnings`.

`make test-all` ejecuta unit, stress/mutation-fuzz, boot, ACPI S3, security,
integration y E2E. Los targets QEMU comparten una imagen del kernel release —el
ELF debug excede el timeout de carga del bootloader BIOS bajo TCG— y detectan el
prompt `brane>` aunque no termine con salto de línea.

`make release-test` valida además los artefactos versionados y arranca la ISO
UEFI con OVMF.

`make stress-test` usa semillas fijas para mutar los parsers FAT32, BDP y Brane
Session, comparar el frame allocator con un modelo de referencia y saturar las
colas IPC. No requiere QEMU y sus fallos son reproducibles.

El test ACPI controla QEMU mediante QMP: ordena `suspend` desde `brsh`, espera
los eventos `SUSPEND` y `WAKEUP`, envía `system_wakeup` y confirma que el shell
y el teclado siguen operativos tras restaurar la plataforma.

---

## 6. CI actual

GitHub Actions ejecuta `.github/workflows/ci.yml` en `push` y `pull_request`
hacia `main`.

Jobs actuales:

| Job | Comando principal |
|-----|-------------------|
| Build Kernel | Build debug + release para `x86_64-unknown-none` |
| Formatting | `cargo fmt --all -- --check` |
| Clippy Lints | Kernel bare-metal + runner host con `-D warnings` |
| Unit Tests | `cargo test -p brane_os_kernel --lib` |
| Stress and Fuzz Tests | `make stress-test` |
| Release Artifact (ISO) | `make iso-test VERSION=ci` |
| Boot Test (QEMU) | `python3 tests/boot/test_boot.py` |
| ACPI S3 Test (QEMU/QMP) | `make acpi-test` |
| Security Tests (QEMU) | `make security-test` |
| Integration Tests (QEMU) | `make integration-test` |
| E2E Tests (QEMU) | `make e2e-test` |

---

## 7. Pendiente para ejecucion tipo release

Para pasar de ejecución de desarrollo en QEMU a release instalable todavía
faltan:

- publicar un artefacto versionado de la imagen booteable mediante tags,
- completar la matriz BIOS frente a UEFI (la ISO actual usa UEFI El Torito),
- ampliar la validación ACPI S3 a hardware físico adicional,
- cerrar el proceso v1.0 con checksums y notas de versión.

---

## 8. Problemas comunes

### `cargo fmt --all -- --check` falla

Ejecute:

```bash
cargo fmt --all
```

Despues repita el check.

### `qemu-system-x86_64` no existe

Instale QEMU y confirme que el binario queda en `PATH`.

```bash
brew install qemu
qemu-system-x86_64 --version
```

### Primer `make run` tarda demasiado

El crate `bootloader` puede compilar piezas BIOS/UEFI la primera vez. Las
siguientes ejecuciones usan cache de Cargo y suelen ser mas rapidas.
