# RUNBOOK.md - Build, CI y ejecucion local

> Estado: operativo para desarrollo local. Este documento describe el flujo
> actual del repositorio, no el release final instalable.

---

## 1. Que se puede ejecutar hoy

Brane OS ya puede compilar el kernel `x86_64-unknown-none`, generar imagenes
BIOS/UEFI con el crate `bootloader` y arrancar en QEMU mediante el runner Rust.

El arranque actual inicializa:

- serial y framebuffer,
- GDT, IDT y PIC,
- memoria, paging y heap,
- scheduler cooperativo,
- syscalls e IPC,
- capacidades, auditoria y loader de modulos,
- Brane Protocol, IA observadora, VFS, RamFS, TTY, shell, red, sockets y DNS.

El sistema entra en `brsh` y queda esperando entrada por TTY.

---

## 2. Requisitos

### Herramientas

- Rust nightly. El repo incluye `rust-toolchain.toml`; no hace falta cambiar el
  toolchain global con `rustup default nightly`.
- Componentes Rust: `rust-src`, `llvm-tools-preview`, `rustfmt`, `clippy`.
- QEMU con `qemu-system-x86_64`.
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
make test
```

`make clippy` valida:

- kernel bare-metal con `-D warnings`,
- runner host con `-D warnings`.

`make test` ejecuta los unit tests host-side del kernel.

---

## 6. CI actual

GitHub Actions ejecuta `.github/workflows/ci.yml` en `push` y `pull_request`
hacia `main`.

Jobs actuales:

| Job | Comando principal |
|-----|-------------------|
| Build Kernel | `cargo build -p brane_os_kernel --target x86_64-unknown-none` |
| Build Kernel release | `cargo build -p brane_os_kernel --target x86_64-unknown-none --release` |
| Formatting | `cargo fmt --all -- --check` |
| Clippy Lints | `cargo clippy -p brane_os_kernel --target x86_64-unknown-none -- -D warnings` |
| Runner Lints | `cargo clippy -p runner --all-targets -- -D warnings` |
| Unit Tests | `cargo test -p brane_os_kernel --lib` |

---

## 7. Pendiente para ejecucion tipo release

Para pasar de ejecucion de desarrollo en QEMU a release instalable todavia
faltan:

- boot test automatizado con QEMU y timeout en CI,
- artefacto versionado de imagen booteable,
- ISO o layout de distribucion instalable,
- documentacion de flags de QEMU para BIOS vs UEFI,
- pruebas e2e sobre `brsh`,
- transiciones reales a user mode,
- proceso de release con checksums y notas de version.

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
