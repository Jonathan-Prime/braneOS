# ARCHITECTURE.md — Brane OS

> Documento derivado de `PROJECT_MASTER_SPEC.md` §8–§10.  
> Estado: **Borrador inicial** — pendiente de elaboración detallada.

---

## 1. Modelo de arquitectura

**Tipo:** Kernel híbrido modular.

Brane OS utiliza una arquitectura híbrida que combina las ventajas de un kernel monolítico (rendimiento, simplicidad en fases tempranas) con la modularidad y extensibilidad de un microkernel (servicios desacoplados, seguridad por aislamiento).

### 1.1 Justificación

| Criterio | Monolítico | Microkernel | **Híbrido (Brane OS)** |
|----------|-----------|-------------|----------------------|
| Complejidad inicial | Baja | Alta | Media |
| Rendimiento IPC | Alto (en-kernel) | Bajo (context switch) | Medio (selectivo) |
| Seguridad | Menor (todo en ring 0) | Alta (aislamiento) | Alta (servicios fuera del kernel) |
| Modularidad | Baja | Alta | Alta |
| Integración IA segura | Difícil | Natural | Natural |

---

## 2. Capas del sistema

```text
┌─────────────────────────────────────────────────┐
│  Capa 4 — User Space                            │
│  Shell, Admin Tools, AI Agents, Utilities        │
├─────────────────────────────────────────────────┤
│  Capa 3 — Drivers                                │
│  Serial, Timer, Disk, Input, Net                 │
├─────────────────────────────────────────────────┤
│  Capa 2 — System Services                        │
│  init, process_manager, filesystem_service,      │
│  device_manager, network_manager,                │
│  identity_service, policy_engine,                │
│  capability_broker, audit_service,               │
│  ai_orchestrator                                 │
├─────────────────────────────────────────────────┤
│  Capa 1 — Kernel Core                            │
│  Scheduler, Memory Manager, Interrupt Manager,   │
│  Syscall Dispatcher, Task Manager, IPC Core,     │
│  Capability Manager, Audit Hooks                 │
├─────────────────────────────────────────────────┤
│  Capa 0 — Boot & Platform                        │
│  Bootloader, Early Init, Platform Bootstrap      │
└─────────────────────────────────────────────────┘
```

---

## 3. Capa 0 — Boot y plataforma

### Responsabilidades
- Inicialización temprana del hardware.
- Lectura del mapa de memoria desde UEFI/BIOS.
- Carga del kernel en memoria.
- Paso de control al entry point del kernel.

### Decisiones abiertas
- [ ] Bootloader custom vs. crate `bootloader`.
- [ ] UEFI vs. Legacy BIOS.
- [ ] Protocolo de handoff (boot info struct).

---

## 4. Capa 1 — Kernel Core

### Módulos

| Módulo | Responsabilidad | Estado |
|--------|----------------|--------|
| `scheduler` | Planificación de hilos/tareas | Pendiente |
| `memory_manager` | Heap, paging, frame allocator | Pendiente |
| `interrupt_manager` | IDT, handlers, excepciones | Pendiente |
| `syscall_dispatcher` | Interfaz kernel/user | Pendiente |
| `task_manager` | Creación, destrucción, estados de tareas | Pendiente |
| `ipc_core` | Comunicación entre procesos | Pendiente |
| `capability_manager` | Validación de capacidades en kernel | Pendiente |
| `audit_hooks` | Hooks para auditoría transversal | Pendiente |

### Principios de diseño del kernel
1. Mínimo código en ring 0.
2. Sin lógica de inferencia o IA en el kernel.
3. Todas las decisiones de política delegadas a Capa 2.
4. Interfaces claras y estables entre módulos.

---

## 5. Capa 2 — Servicios del sistema

Los servicios corren en user space (o en un modo privilegiado controlado) y se comunican con el kernel a través de syscalls e IPC.

### Servicios planificados

| Servicio | Dependencias | Prioridad |
|----------|-------------|-----------|
| `init` | Kernel Core | Fase 4 (alta) |
| `process_manager` | Kernel Core, IPC | Fase 4 |
| `filesystem_service` | Kernel Core, Drivers | Fase 4 |
| `policy_engine` | IPC, Capability Manager | Fase 4 |
| `audit_service` | IPC, Audit Hooks | Fase 4 |
| `device_manager` | Kernel Core, Drivers | Fase 4 |
| `capability_broker` | Policy Engine, Capability Manager | Fase 6 |
| `identity_service` | IPC | Fase 6 |
| `network_manager` | Drivers, IPC | Futuro |
| `ai_orchestrator` | Todos los servicios de seguridad | Fase 5-6 |

---

## 6. Capa 3 — Drivers

### Familias iniciales
- **Serial (UART 16550)** — Salida de logs temprana. ✅ Implementado en kernel.
- **Timer (PIT/APIC)** — Base para scheduling.
- **Disk** — Acceso a almacenamiento básico.
- **Input** — Teclado básico.
- **Net** — Futuro.

---

## 7. Capa 4 — User Space

- **Shell mínima** — Interfaz de comandos básica.
- **Admin tools** — Inspección de estado del sistema.
- **AI Agents** — Runtime IA en user space.
- **Utilities** — Herramientas auxiliares.

---

## 8. Interfaces entre capas

### Kernel ↔ Services (Syscalls)
```
Servicio → syscall(id, args...) → Kernel
Kernel  → resultado / error    → Servicio
```

### Services ↔ Services (IPC)
```
Servicio A → IPC message → Servicio B
Servicio B → IPC reply   → Servicio A
```

### AI ↔ System (Mediado)
```
AI Agent → ai_orchestrator → capability_broker → policy_engine → acción
                                                                → audit_service
```

---

## 9. Decisiones de arquitectura registradas

Ver carpeta `ADR/` para decisiones formales.

- **ADR-001**: Arquitectura híbrida modular inicial.

---

## 10. Próximos pasos

1. Definir formato exacto de boot handoff.
2. Diseñar interfaz de syscalls mínimas.
3. Definir protocolo IPC base.
4. Documentar invariantes del kernel.
5. Elaborar diagramas de secuencia para flujos críticos.
