# ROADMAP.md — Brane OS

> Documento derivado de `PROJECT_MASTER_SPEC.md` §19.  
> Estado: **Activo** — se actualiza conforme el proyecto avanza.

---

## Visión general

```text
Fase 1          Fase 2          Fase 3         Fase 4          Fase 5         Fase 6
Boot +          Memoria +       Syscalls +     Servicios       IA             IA actuadora
Kernel mínimo   Scheduler       IPC            del sistema     observadora    restringida
───────────────────────────────────────────────────────────────────────────────────────▶
```

---

## Fase 1 — Boot y kernel mínimo ◀ ACTUAL

**Objetivo:** Arrancar en QEMU con salida serial funcional.

| Componente | Estado | Notas |
|-----------|--------|-------|
| Estructura del repositorio | ✅ Completo | Per spec §20 |
| Cargo workspace (no_std) | ✅ Completo | x86_64-unknown-none |
| Serial output (UART 16550) | ✅ Completo | COM1, macros serial_print!/serial_println! |
| Boot banner | ✅ Completo | Entry point _start, panic handler |
| Bootloader / UEFI | 🔲 Pendiente | Decidir boot path |
| Interrupciones iniciales (IDT) | 🔲 Pendiente | |
| Makefile + QEMU runner | ✅ Completo | build, run, test, clean |
| Documentación base | ✅ Completo | Master spec + docs derivados |

---

## Fase 2 — Memoria y Scheduler

**Objetivo:** Heap funcional, paging, y scheduler básico.

| Componente | Estado | Notas |
|-----------|--------|-------|
| Frame allocator | 🔲 Pendiente | |
| Page tables | 🔲 Pendiente | |
| Heap allocator | 🔲 Pendiente | linked_list_allocator |
| Task struct | 🔲 Pendiente | |
| Scheduler (round-robin) | 🔲 Pendiente | |
| Context switching | 🔲 Pendiente | |

---

## Fase 3 — Syscalls e IPC

**Objetivo:** Interfaz kernel/user space y comunicación entre procesos.

| Componente | Estado | Notas |
|-----------|--------|-------|
| Syscall dispatcher | 🔲 Pendiente | |
| Syscalls mínimas | 🔲 Pendiente | write, exit, yield, send, recv |
| IPC básico | 🔲 Pendiente | Message passing |
| User mode transition | 🔲 Pendiente | |

---

## Fase 4 — Servicios del sistema

**Objetivo:** Servicios core funcionando en user space.

| Componente | Estado | Notas |
|-----------|--------|-------|
| init | 🔲 Pendiente | |
| process_manager | 🔲 Pendiente | |
| filesystem_service | 🔲 Pendiente | |
| policy_engine | 🔲 Pendiente | |
| audit_service | 🔲 Pendiente | |
| Shell mínima | 🔲 Pendiente | |

---

## Fase 5 — IA observadora

**Objetivo:** Subsistema IA capaz de observar y sugerir.

| Componente | Estado | Notas |
|-----------|--------|-------|
| context_collector | 🔲 Pendiente | |
| model_runtime | 🔲 Pendiente | |
| decision_planner | 🔲 Pendiente | |
| Reportes y sugerencias | 🔲 Pendiente | |

---

## Fase 6 — IA actuadora restringida

**Objetivo:** IA capaz de ejecutar acciones limitadas bajo control.

| Componente | Estado | Notas |
|-----------|--------|-------|
| capability_broker | 🔲 Pendiente | |
| safety_filter | 🔲 Pendiente | |
| Ejecución limitada | 🔲 Pendiente | |
| Trazabilidad total | 🔲 Pendiente | |
