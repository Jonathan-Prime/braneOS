# ROADMAP.md — Brane OS

> Documento derivado de `PROJECT_MASTER_SPEC.md` §19.  
> Estado: **Activo** — se actualiza conforme el proyecto avanza.  
> Última actualización: **2026-03-12**

---

## Visión general

```text
 ✅ COMPLETADO                                          🔲 PRÓXIMOS PASOS
 ══════════════════════════════════════════════════════  ══════════════════════════════════════════════════
 Fase 1    Fase 2      Fase 3     Fase 4     Fase 5   │ Fase 6     Fase 7     Fase 8     Fase 9    Fase 10
 Boot +    Memoria +   Syscalls + Seguridad  Brane    │ Bootloader Filesystem Networking Brane     Producción
 Kernel    Scheduler   IPC        Audit, IA  Protocol │ real       + Shell    + Cluster   Protocol  + Release
 mínimo                           Procesos            │            VFS, TTY   TCP/IP     v2        v1.0
 ─────────────────────────────────────────────────────┼─────────────────────────────────────────────────▶
```

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
| GitHub Actions CI | ✅ | Build (debug+release), `rustfmt`, `clippy -D warnings` |
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
| Syscall dispatcher | ✅ | 24 syscalls, 7 subsistemas (incl. Brane), 10 error codes |
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

## 🔲 Fase 7 — Filesystem, Shell y TTY (EN PROGRESO)

**Objetivo:** Sistema de archivos virtual, terminal y shell interactiva.

| Componente | Estado | Prioridad | Notas |
|-----------|--------|-----------|-------|
| VFS (Virtual Filesystem) | ✅ | ALTA | Trait `FileSystem`, mount table, path resolution |
| RamFS (in-memory FS) | ✅ | ALTA | 256 inodes, /dev, /proc, /tmp |
| TTY driver | ✅ | ALTA | Input ring buffer + dual output (serial+fb) |
| Shell mínima (`brsh`) | ✅ | ALTA | 13 comandos: help, ps, mem, ls, cat, etc. |
| `initramfs` | 🔲 | MEDIA | Imagen de boot con binarios iniciales |
| FAT32 / ext2 (lectura) | 🔲 | BAJA | Acceso a discos reales |

## 🔲 Fase 7 — Filesystem, Shell y TTY

**Objetivo:** Sistema de archivos virtual, terminal y shell interactiva.

| Componente | Estado | Prioridad | Notas |
|-----------|--------|-----------|-------|
| VFS (Virtual Filesystem) | 🔲 | ALTA | Trait `FileSystem`, mount points |
| RamFS (in-memory FS) | 🔲 | ALTA | Para boot y testing |
| TTY driver | 🔲 | ALTA | Terminal virtual via serial + framebuffer |
| Shell mínima (`brsh`) | 🔲 | ALTA | Comandos: `ls`, `ps`, `cat`, `help`, `brane status` |
| `initramfs` | 🔲 | MEDIA | Imagen de boot con binarios iniciales |
| FAT32 / ext2 (lectura) | 🔲 | BAJA | Acceso a discos reales |

**Dependencias:** Fase 6 (page tables para buffers de I/O).

---

## 🔲 Fase 8 — Networking y Clustering

**Objetivo:** Stack de red para comunicación brane real.

| Componente | Estado | Prioridad | Notas |
|-----------|--------|-----------|-------|
| Network driver (virtio-net) | 🔲 | ALTA | Para QEMU/KVM |
| Ethernet frame parsing | 🔲 | ALTA | L2 |
| ARP + IPv4 | 🔲 | ALTA | L3 |
| TCP/UDP | 🔲 | ALTA | L4, `smoltcp` crate |
| Socket API (syscalls) | 🔲 | ALTA | `socket`, `bind`, `listen`, `accept`, `connect` |
| DNS resolver | 🔲 | MEDIA | Resolución básica |
| TLS / Crypto | 🔲 | MEDIA | Para Brane Protocol encriptado |
| Brane Protocol over TCP | 🔲 | MEDIA | Reemplazar simulación con red real |
| Cluster discovery (mDNS) | 🔲 | BAJA | Auto-descubrimiento en LAN |

**Dependencias:** Fase 6 (DMA, page tables), Fase 7 (VFS para sockets).

---

## 🔲 Fase 9 — Brane Protocol v2 y Mobile Bridge

**Objetivo:** Protocolo brane real para interconexión con dispositivos.

| Componente | Estado | Prioridad | Notas |
|-----------|--------|-----------|-------|
| Brane handshake criptográfico | 🔲 | ALTA | Ed25519 + X25519 key exchange |
| Brane session encryption (E2E) | 🔲 | ALTA | ChaCha20-Poly1305 |
| Capability negotiation protocol | 🔲 | ALTA | Intercambio de caps entre branes |
| Mobile companion app (API) | 🔲 | MEDIA | REST/WebSocket bridge para iOS/Android |
| Brane resource sharing | 🔲 | MEDIA | Compartir CPU, storage, sensors |
| Brane migration | 🔲 | BAJA | Migrar procesos entre branes |
| IoT sensor protocol | 🔲 | BAJA | Protocolo ligero para dispositivos embebidos |

**Dependencias:** Fase 8 (TCP/IP stack).

---

## 🔲 Fase 10 — Producción, Estabilidad y Release

**Objetivo:** Estabilizar para release v1.0.

| Componente | Estado | Prioridad | Notas |
|-----------|--------|-----------|-------|
| Context switching real | 🔲 | ALTA | Save/restore registros, ring 0 ↔ ring 3 |
| User mode transitions | 🔲 | ALTA | `sysenter`/`sysexit` o `syscall`/`sysret` |
| Señales (SIGTERM, SIGKILL, etc.) | 🔲 | ALTA | Signal handling POSIX-like |
| Multi-core (SMP) | 🔲 | MEDIA | APIC, per-CPU scheduler |
| ACPI power management | 🔲 | MEDIA | Shutdown, sleep, wake |
| USB stack (xHCI) | 🔲 | MEDIA | Para periféricos reales |
| GPU driver (básico) | 🔲 | BAJA | Framebuffer → GPU acceleration |
| Package manager (`bpkg`) | 🔲 | BAJA | Instalación de software |
| Test suite completa | 🔲 | ALTA | Integration tests, stress tests, fuzzing |
| Documentación de API | 🔲 | MEDIA | `cargo doc`, guías de contribución |
| Release v1.0 | 🔲 | — | ISO booteable + documentación |

**Dependencias:** Todas las fases anteriores.

---

## Métricas actuales del proyecto

| Métrica | Valor |
|---------|-------|
| **Módulos del kernel** | 19 |
| **Líneas de código (Rust)** | ~6,500 |
| **Unit tests** | 35 |
| **Syscalls definidas** | 24 |
| **CI checks** | 3 (build, fmt, clippy) |
| **Fases completadas** | 6 de 10 (Fase 7 en progreso) |

---

## Principios de escalabilidad

1. **Modularidad**: Cada subsistema es un módulo independiente con interfaz definida.
2. **No-alloc en kernel core**: Los módulos críticos usan arrays estáticos, no heap.
3. **Capability-based security**: Todo acceso es mediado por capabilities verificables.
4. **Audit-first**: Toda acción de seguridad se registra antes de ejecutarse.
5. **Brane architecture**: El OS es una membrana que escala conectándose a otras membranas.
6. **AI-assisted**: La IA observa y optimiza, pero nunca tiene control total.
7. **Test-driven**: Cada módulo tiene tests unitarios; CI valida en cada push.
