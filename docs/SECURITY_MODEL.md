# SECURITY_MODEL.md — Brane OS

> Documento derivado de `PROJECT_MASTER_SPEC.md` §13–§14.  
> Estado: **Borrador inicial** — pendiente de elaboración detallada.

---

## 1. Enfoque de seguridad

Brane OS implementa un modelo de **seguridad basada en capacidades** (capability-based security), complementado con un motor de políticas y un sistema de auditoría integral.

### Principios fundamentales

1. **Menor privilegio** — Todo proceso, servicio o agente opera con el mínimo de permisos necesarios.
2. **Mediación obligatoria** — Toda acción sensible pasa por el capability broker.
3. **Auditoría completa** — Toda acción relevante queda registrada.
4. **Separación kernel/user** — El kernel no ejecuta lógica de política ni de IA.
5. **Defensa en profundidad** — Múltiples capas de validación.

---

## 2. Componentes de seguridad

### 2.1 Capability Manager (Kernel)
- Reside en el kernel (Capa 1).
- Mantiene tabla de capacidades por proceso/tarea.
- Valida permisos en tiempo de ejecución.
- No toma decisiones de política — solo verifica si existe la capacidad.

### 2.2 Policy Engine (Capa 2)
- Servicio en user space.
- Aplica reglas duras (deny/allow) y blandas (conditional).
- Evalúa contexto, identidad, y nivel de riesgo.
- Puede requerir aprobación humana para acciones sensibles.

### 2.3 Capability Broker (Capa 2)
- Mediador central de acceso para acciones privilegiadas.
- Recibe solicitudes, consulta al Policy Engine, y responde.
- Registra toda transacción en el Audit Service.

### 2.4 Audit Service (Capa 2)
- Registra eventos de seguridad de todo el sistema.
- Campos por evento:
  - `timestamp`
  - `source` (quién solicitó)
  - `action` (qué se solicitó)
  - `context` (estado del sistema)
  - `policy_applied` (qué regla se evaluó)
  - `result` (aprobado/denegado/escalado)
  - `metadata` (información adicional)

---

## 3. Capabilities

### 3.1 Modelo

Cada capacidad es un token que otorga permiso para realizar una acción específica.

```rust
pub struct Capability {
    id: CapabilityId,
    name: &'static str,
    scope: Scope,          // System, Service, Process
    risk_level: RiskLevel, // Low, Medium, High, Critical
    revocable: bool,
}
```

### 3.2 Capabilities definidas

| Capability | Scope | Riesgo | Descripción |
|-----------|-------|--------|-------------|
| `read_system_metrics` | System | Low | Leer métricas de CPU, memoria |
| `read_service_status` | System | Low | Leer estado de servicios |
| `read_audit_logs` | System | Medium | Leer logs de auditoría |
| `request_process_inspection` | Process | Medium | Inspeccionar un proceso |
| `restart_noncritical_service` | Service | Medium | Reiniciar servicios no críticos |
| `request_network_diagnostics` | System | Medium | Ejecutar diagnósticos de red |
| `require_human_approval` | System | High | Escalar para aprobación humana |
| `deny_sensitive_file_read` | System | Critical | Bloquear lectura de archivos sensibles |

---

## 4. Flujo de autorización

```text
┌──────────────┐    solicitud    ┌─────────────────┐
│  Solicitante  │───────────────▶│ Capability Broker│
│ (AI / Service)│                │                  │
└──────────────┘                └────────┬──────────┘
                                         │
                                         ▼
                                ┌─────────────────┐
                                │  Policy Engine   │
                                │  (evalúa reglas) │
                                └────────┬──────────┘
                                         │
                              ┌──────────┼──────────┐
                              ▼          ▼          ▼
                          APROBADO    DENEGADO   ESCALADO
                              │          │     (humano)
                              ▼          ▼          │
                        ┌──────────┐ ┌──────────┐   │
                        │ Ejecutar │ │ Rechazar │   │
                        └────┬─────┘ └────┬─────┘   │
                             │            │          │
                             ▼            ▼          ▼
                        ┌──────────────────────────────┐
                        │       Audit Service           │
                        │   (registra todo el evento)   │
                        └──────────────────────────────┘
```

---

## 5. Reglas de seguridad para IA

1. La IA **nunca** accede directamente a recursos del kernel.
2. La IA **siempre** opera bajo un nivel de acceso definido (§12 del master spec).
3. Toda solicitud IA pasa por: `safety_filter` → `capability_broker` → `policy_engine`.
4. Las acciones de IA son **revocables** en tiempo real.
5. El `audit_service` registra **toda** interacción IA con el sistema.

---

## 6. Separación de privilegios

| Contexto | Ring / Nivel | Acceso |
|----------|-------------|--------|
| Kernel Core | Ring 0 | Hardware directo |
| System Services | Ring 3 (privilegiado) | Syscalls |
| User Applications | Ring 3 | Syscalls limitadas |
| AI Agents | Ring 3 (restringido) | Solo vía broker |

---

## 7. Decisiones abiertas

- [ ] Formato exacto de tokens de capacidad.
- [ ] Mecanismo de revocación en caliente.
- [ ] Persistencia del policy store.
- [ ] Formato del audit log (binario vs. texto).
- [ ] Integración con identity_service para autenticación.

---

## 8. Próximos pasos

1. Definir API del capability_manager en kernel.
2. Diseñar formato de reglas del policy_engine.
3. Implementar audit log básico.
4. Crear tests de seguridad para denegación de acceso.
