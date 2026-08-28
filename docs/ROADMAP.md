# ROADMAP.md — Brane OS

> Documento derivado de `PROJECT_MASTER_SPEC.md` §19.  
> Estado: **Activo** — se actualiza conforme el proyecto avanza.  
> Última actualización: **2026-08-28**

---

## Visión general

```text
 ✅ BASE DEL SISTEMA COMPLETADA                         🔄 SIGUIENTE CICLO
 ═════════════════════════════════════════════════════  ══════════════════════════════════════════════════
 Fases 1–5              Fases 6–9          Fase 10   │ Fase 11      Fase 12     Fase 13      Fase 14
 Kernel, memoria,       Boot real, VFS,    Producción│ Release v1.0 SMP/APIC     Hardware I/O Plataforma
 seguridad, IA y        red y Brane v2     y calidad │ y artefactos multicore    USB/storage  y ecosistema
 protocolo base                                      │
 ─────────────────────────────────────────────────────┼─────────────────────────────────────────────────▶
```

**Foco actual:** Fase 12 — activación incremental de APIC y preparación SMP.

---

## ✅ Fase 1 — Boot y kernel mínimo (COMPLETADA)

**Objetivo:** Arrancar en QEMU con salida serial funcional.

| Componente | Estado | Notas |
|-----------|--------|-------|
| Estructura del repositorio | ✅ | `kernel/`, `services/`, `drivers/`, `userland/`, `ai/`, `tests/`, `tools/` |
| Cargo workspace (`no_std`) | ✅ | Target: `x86_64-unknown-none`, nightly toolchain |
| Serial output (UART 16550) | ✅ | COM1, macros `serial_print!`/`serial_println!` |
| GDT + TSS + IST | ✅ | Double fault stack aislado |
| IDT (7 excepciones) | ✅ | Breakpoint, Double Fault, Page Fault, GPF, Invalid Opcode, Segment NP, Stack Fault |
| PIC 8259 | ✅ | IRQs remapeados a vectores 32–47 |
| Keyboard (PS/2) | ✅ | Scancode decoding con `pc-keyboard` |
| Timer interrupt | ✅ | PIT ~18.2 Hz |
| Makefile + QEMU runner | ✅ | `build`, `run`, `test`, `clean` |
| GitHub Actions CI | ✅ | 10 checks: build, calidad, unit, stress/fuzz y cinco suites QEMU |
| Documentación base | ✅ | ARCHITECTURE, SECURITY_MODEL, AI_SUBSYSTEM, ROADMAP, TEST_PLAN |

---

## ✅ Fase 2 — Memoria y Scheduler (COMPLETADA)

**Objetivo:** Gestión de memoria física e inicio del scheduler.

| Componente | Estado | Notas |
|-----------|--------|-------|
| Frame allocator (bitmap) | ✅ | Soporta hasta 1 GiB, trait `FrameAllocator<Size4KiB>` |
| Heap allocator | ✅ | `linked_list_allocator`, 1 MiB, `#[global_allocator]` |
| Scheduler (round-robin) | ✅ | 6 prioridades (Idle→System), 64 tasks max |

---

## ✅ Fase 3 — Syscalls e IPC (COMPLETADA)

**Objetivo:** Interfaz kernel/user space y comunicación entre procesos.

| Componente | Estado | Notas |
|-----------|--------|-------|
| Syscall dispatcher | ✅ | 28 syscalls, 7 subsistemas (incl. Brane), 10 error codes |
| Handlers implementados | ✅ | `exit`, `yield`, `getpid`, `write`, `ipc_send`, `ipc_recv`, `get_time`, `get_system_info` |
| IPC Core | ✅ | Message passing: ring buffer 16 msgs × 4 KiB, 4 tipos (Request, Response, Notification, BraneRelay) |

---

## ✅ Fase 4 — Seguridad, Auditoría e IA (COMPLETADA)

**Objetivo:** Sistema de capacidades, auditoría transversal e IA observadora.

| Componente | Estado | Notas |
|-----------|--------|-------|
| Capability Manager | ✅ | 9 permisos (incl. `BRANE_CONNECT`), 4 risk levels, 4 scopes, 256 entries |
| Audit Hooks | ✅ | 14 event types, ring buffer 512, secuenciación monotónica |
| Module Loader | ✅ | Hot-swap, 32 módulos, dependency tracking |
| AI Engine | ✅ | 4 modos (Disabled→ActRestricted), 6 categorías, actuación con audit |
| Process Table | ✅ | PCB, 128 procesos, 7 estados, memory map |
| Unit Tests | ✅ | 35 tests en 9 módulos |

---

## ✅ Fase 5 — Brane Protocol (COMPLETADA)

**Objetivo:** Interconexión segura con dispositivos externos.

| Componente | Estado | Notas |
|-----------|--------|-------|
| Brane Discovery | ✅ | 16 branes descubribles |
| Session Manager | ✅ | 8 sesiones simultáneas, autenticación |
| Message Protocol | ✅ | 11 tipos de mensaje, 2 KiB payload |
| 3 tipos de brane | ✅ | Companion (móvil), Peer (PC), IoT |
| 5 transportes | ✅ | TCP/IP, Bluetooth, BLE, USB Direct, Local |
| Audit integration | ✅ | Conexiones y desconexiones loggeadas |

---

## ✅ Fase 6 — Bootloader Real y Paging (COMPLETADA)

**Objetivo:** Bootear en hardware real con paging completo.

| Componente | Estado | Notas |
|-----------|--------|-------|
| Integrar crate `bootloader` v0.11 | ✅ | UEFI boot con OVMF |
| Memory map del bootloader | ✅ | Parseo real de `boot_info.memory_regions` |
| Page Table Manager | ✅ | OffsetPageTable desde CR3 con `physical_memory_offset` |
| Heap init real | ✅ | 1 MiB heap, `linked_list_allocator` mapeado con page tables |
| Framebuffer output | ✅ | Texto 160×50 via framebuffer BGR, font bitmap 8×16 |
| UEFI boot | ✅ | OVMF pflash + HVF aceleración |

---

## ✅ Fase 7 — Filesystem, Shell y TTY (COMPLETADA)

**Objetivo:** Sistema de archivos virtual, terminal y shell interactiva.

| Componente | Estado | Prioridad | Notas |
|-----------|--------|-----------|-------|
| VFS (Virtual Filesystem) | ✅ | ALTA | Trait `FileSystem`, mount table, path resolution |
| RamFS (in-memory FS) | ✅ | ALTA | 256 inodes, /dev, /proc, /tmp |
| TTY driver | ✅ | ALTA | Input ring buffer + dual output (serial+fb) |
| `brsh` (Shell mínima) | ✅ | ALTA | 20 comandos + alias `sleep` y `poweroff` |
| `initramfs` | ✅ | MEDIA | Imagen de boot dinámica en RamFS (/etc/motd, etc.) |
| FAT32 (base) | ✅ | BAJA | Stub VFS + parseo de MBR/BootSector; lectura real pasa a Fase 13 |

---

## ✅ Fase 8 — Networking y Clustering (COMPLETADA)

**Objetivo:** Stack de red para comunicación brane real.

| Componente | Estado | Notas |
|-----------|--------|-------|
| Network driver (virtio-net) | ✅ | PCI scan + legacy I/O init, MAC discovery |
| Ethernet frame parsing | ✅ | smoltcp wire types integrados |
| ARP + IPv4 | ✅ | Configuración estática 10.0.2.15/24 |
| TCP/UDP | ✅ | smoltcp 0.11 (socket-tcp, socket-udp) |
| Socket API (32 slots) | ✅ | create/bind/listen/connect/close |
| DNS resolver | ✅ | Tabla estática de hosts (4 entradas) |
| Session crypto | ✅ | Completado en Fase 9 con X25519 + ChaCha20-Poly1305 |
| Brane Protocol over TCP | ✅ | Completado en Fase 9 |
| Cluster discovery (mDNS) | ↪ | Replanificado para Fase 14 |

---

## ✅ Fase 9 — Brane Protocol v2 (COMPLETADA)

**Objetivo:** Protocolo brane real para interconexión segura con dispositivos.

| Componente | Estado | Prioridad | Notas |
|-----------|--------|-----------|-------|
| State machine de sesiones | ✅ | ALTA | Init → WaitResponse → WaitCapability → Established → Closed |
| Handshake X25519 (ECDH) | ✅ | ALTA | Key exchange de 32 bytes, derivación de shared secret |
| Session encryption (ChaCha20-Poly1305) | ✅ | ALTA | Cifrado E2E con nonce counter de 64bits (12-byte format) |
| Capability negotiation protocol | ✅ | ALTA | `CapabilityNegotiation` struct con serialización binary-safe |
| TCP session management | ✅ | ALTA | Integración en `brane_discovery.rs` con sesión registry |
| Packet types (6 tipos) | ✅ | ALTA | HandshakeInit, Response, CapabilityExchange, EncryptedData, Alert, Disconnect |
| Error handling | ✅ | MEDIA | `SessionError` enum con 6 tipos de error específicos |
| Unit tests (14 tests) | ✅ | MEDIA | State machine, serialization, encryption/decryption |
| Mobile companion bridge | ↪ | MEDIA | Replanificado para Fase 14 |
| Brane resource sharing | ↪ | MEDIA | Replanificado para Fase 14 |
| IoT lightweight protocol | ↪ | BAJA | Replanificado para Fase 14 |

**Dependencias:** Fase 8 (TCP/IP stack), crypto.rs (X25519, ChaCha20).

**Nuevos módulos:**
- `brane_session.rs` (500+ líneas): Máquina de estados, cifrado, serialización
- `CapabilityOffer` y `CapabilityNegotiation` structs
- Métodos en `DiscoverySubsystem` para gestionar sesiones TCP

---

## ✅ Fase 10 — Producción y Estabilidad (COMPLETADA)

**Objetivo:** Dejar una base estable, observable y validada que pueda convertirse
en un release versionado.

| Componente | Estado | Prioridad | Notas |
|-----------|--------|-----------|-------|
| Context switching real | ✅ | ALTA | Coop: save/restore registers (r12-r15, rbx, rbp, rsp) |
| **Boot test automatizado (QEMU)** | ✅ | ALTA | Kernel release en QEMU/TCG; valida banner, ACPI y prompt en 60 s |
| **Empaquetado ISO base** | ✅ | ALTA | `tools/make_iso.sh` + UEFI El Torito, `make iso` / `make release` |
| **User mode transitions** | ✅ | ALTA | `syscall`/`sysret` via `usermode::init_syscall_msrs()` — activo en boot |
| **Señales POSIX** | ✅ | ALTA | `signal.rs`: `Kill`, `SigAction`, `SigReturn` syscalls + `SIGNAL_MANAGER` |
| **Security tests** | ✅ | ALTA | `tests/security/`: capability denial + privilege escalation; job QEMU en CI |
| **Integration tests** | ✅ | ALTA | `tests/integration/`: syscall→service + capability broker; job QEMU en CI |
| **E2E tests** | ✅ | ALTA | `tests/e2e/`: disponibilidad de brsh + boot flow (20 fases); job QEMU en CI |
| **Documentación de API** | ✅ | MEDIA | `make docs` → `cargo doc -p brane_os_kernel` |
| ACPI power management | ✅ | MEDIA | S3 suspend/resume vía FACS + trampolín real→long mode; shutdown/reboot; test QEMU/QMP |
| Stress tests / fuzzing | ✅ | MEDIA | 35k casos parser/roundtrip + 50k ops allocator + 4k mensajes IPC; determinista en CI |

**Dependencias:** Todas las fases anteriores.

**Criterio de salida alcanzado:** build bare-metal, Clippy, 113 tests lógicos y
cinco suites QEMU automatizadas pasan; existen imágenes BIOS/UEFI y empaquetado ISO.

---

## 🔄 Fase 11 — Release Engineering v1.0 (EN PROGRESO)

**Objetivo:** Producir y publicar artefactos v1.0 verificables y reproducibles.

| Componente | Estado | Prioridad | Criterio de aceptación |
|-----------|--------|-----------|------------------------|
| Empaquetado portable | ✅ | ALTA | `sha256sum`/`shasum`, sin GRUB; validado en macOS y preparado para Linux CI |
| Boot de ISO en CI | ✅ | ALTA | Harness `--iso` con OVMF/TCG; alcanza `brane>` desde `-cdrom` |
| Matriz BIOS + UEFI | ✅ | ALTA | Boot test BIOS existente + release ISO UEFI automatizada |
| Workflow por tags | ✅ | ALTA | Tags `v*` construyen, prueban y publican artefactos automáticamente |
| Artefactos de release | ✅ | ALTA | ISO, imágenes BIOS/UEFI, checksum y archivo comprimido |
| Notas y changelog | ✅ | MEDIA | `CHANGELOG.md`, guía de release y notas automáticas de GitHub |
| Matriz de hardware físico | 🔲 | MEDIA | Registro reproducible de boot, teclado, red y ACPI |
| Gate v1.0 | 🔄 | ALTA | `make release-test` automatiza CI, artefactos verificados y boot UEFI |

**Criterio de salida:** un tag v1.0 produce automáticamente artefactos que
arrancan en BIOS y UEFI, con checksums y notas de versión.

**Orden de ejecución:** empaquetado portable → boot ISO → matriz BIOS/UEFI →
workflow de tags y artefactos → documentación/hardware → gate v1.0.

**Progreso:** 6/8 componentes completados. Pendientes: matriz de hardware
físico y ejecución del gate final v1.0.

---

## 🔲 Fase 12 — SMP, APIC y Concurrencia

**Objetivo:** Escalar el kernel de una CPU a múltiples cores.

| Componente | Estado | Prioridad | Dependencia |
|-----------|--------|-----------|-------------|
| Parser MADT | ✅ | ALTA | ACPI |
| Local APIC + I/O APIC | ✅ | ALTA | IDT, overrides MADT y routing IRQ0/IRQ1 |
| Arranque de Application Processors | 🔄 | ALTA | INIT/SIPI + GDT/TSS/IDT/MSR xAPIC; dispatcher IPI acotado integrado |
| Estado per-CPU | 🔄 | ALTA | GDT/TSS/IST, stacks, MSRs y contadores runtime por CPU; falta contexto aislado |
| Scheduler multicore | 🔄 | ALTA | Run queues, menor carga, steal y dispatch/complete integrados; falta cambio de contexto |
| Sincronización y stress SMP | 🔲 | ALTA | Spinlocks, atomics y pruebas de carrera |

**Criterio de salida:** QEMU arranca con al menos 4 vCPU, ejecuta tareas en
múltiples cores y supera stress tests sin deadlocks ni corrupción.

**Progreso:** parser MADT integrado en el descubrimiento ACPI (incluye entradas
x2APIC, sobrescritura de dirección LAPIC y `Interrupt Source Override`); ventanas
MMIO del LAPIC/I/O APIC con atributos uncached; hand-off controlado de IRQ0/IRQ1
al I/O APIC con EOI por LAPIC, restauración tras S3 y fallback automático al PIC;
y trampoline INIT/SIPI con timeout que arranca APs xAPIC en QEMU (4 vCPU),
incluyendo GDT/TSS/IST, IDT y MSRs de syscall por AP antes del ACK.
El plan SMP valida APIC IDs/UIDs, asigna el BSP y registra estados `Online` o
`Failed`. Las run queues por CPU distribuyen y roban tareas de forma
determinista. Una IPI dirigida a cada AP ejecuta y contabiliza un quantum de
dispatch/complete, dejando evidencia `Multicore dispatch active`; quedan el
cambio de contexto aislado por AP, la sincronización y la validación de
hardware físico.

---

## 🔲 Fase 13 — Hardware I/O y Almacenamiento

**Objetivo:** Ampliar compatibilidad con periféricos y almacenamiento real.

| Componente | Estado | Prioridad | Dependencia |
|-----------|--------|-----------|-------------|
| Enumeración PCI/PCIe robusta | 🔲 | ALTA | Config space y BAR mapping |
| MSI/MSI-X | 🔲 | MEDIA | APIC |
| Controlador xHCI | 🔲 | ALTA | PCIe + DMA |
| USB HID | 🔲 | ALTA | xHCI; teclado y ratón |
| USB mass storage | 🔲 | MEDIA | xHCI + block layer |
| Block layer | 🔲 | ALTA | Discos virtio/USB |
| FAT32 de lectura real | 🔲 | MEDIA | Block layer; reemplaza el stub actual |

**Criterio de salida:** teclado USB y almacenamiento masivo funcionan en QEMU
y en al menos una máquina física soportada.

---

## 🔲 Fase 14 — Plataforma y Ecosistema Brane

**Objetivo:** Convertir el kernel estable en una plataforma extensible para
aplicaciones y dispositivos Brane.

| Componente | Estado | Prioridad | Dependencia |
|-----------|--------|-----------|-------------|
| Package manager (`bpkg`) | 🔲 | ALTA | VFS persistente + firmas |
| Formato de paquetes y repositorio | 🔲 | ALTA | `bpkg` + capability manifests |
| Mobile companion bridge | 🔲 | MEDIA | Brane Protocol v2 |
| Brane resource sharing | 🔲 | MEDIA | Sesiones cifradas + políticas |
| IoT lightweight protocol | 🔲 | MEDIA | Transporte Brane reducido |
| GPU driver básico | 🔲 | BAJA | PCIe; framebuffer acelerado |
| SDK y ejemplos | 🔲 | MEDIA | ABI estable y documentación API |

**Criterio de salida:** instalar y verificar un paquete firmado, conectar un
companion y compartir un recurso bajo control de capabilities y auditoría.

---

## Métricas actuales del proyecto

| Métrica | Valor |
|---------|-------|
| **Módulos del kernel** | 35 archivos de módulo (excluye `lib.rs`, `main.rs`, `tests.rs`) |
| **Líneas de código (Rust)** | ~11,000 |
| **Unit tests** | 125 (incluye MADT/APIC/SMP, integration, stress y mutation-fuzz) |
| **Syscalls definidas** | 28 (incluye Kill, SigAction, SigReturn, SigProcMask) |
| **Harnesses de test Python** | 8 (boot + ACPI S3 + 2 security + 2 integration + 2 e2e) |
| **CI checks** | 11 (build, fmt, clippy, unit, stress/fuzz, release ISO, boot, ACPI S3, security, integration, E2E) |
| **Make targets de test** | 10 (test, stress-test, iso-test, release-test, boot-test, smp-test, acpi-test, security-test, integration-test, e2e-test) |
| **Fases completadas** | 10 fases base completadas; Fases 11 y 12 avanzan en paralelo |

---

## Principios de escalabilidad

1. **Modularidad**: Cada subsistema es un módulo independiente con interfaz definida.
2. **No-alloc en kernel core**: Los módulos críticos usan arrays estáticos, no heap.
3. **Capability-based security**: Todo acceso es mediado por capabilities verificables.
4. **Audit-first**: Toda acción de seguridad se registra antes de ejecutarse.
5. **Brane architecture**: El OS es una membrana que escala conectándose a otras membranas.
6. **AI-assisted**: La IA observa y optimiza, pero nunca tiene control total.
7. **Test-driven**: Cada módulo tiene tests unitarios; CI valida en cada push.
