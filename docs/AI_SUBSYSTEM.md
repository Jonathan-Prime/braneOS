# AI_SUBSYSTEM.md — Brane OS

> Documento derivado de `PROJECT_MASTER_SPEC.md` §11–§12, §14.  
> Estado: **Borrador inicial** — pendiente de elaboración detallada.

---

## 1. Visión

El subsistema de IA de Brane OS actúa como una **capa de inteligencia operativa** que observa, analiza, sugiere y — bajo autorización estricta — ejecuta acciones controladas sobre el sistema.

> **Regla fundamental:** La IA no tiene acceso directo libre a recursos del sistema.

---

## 2. Componentes

### 2.1 context_collector
- Recopila telemetría autorizada: CPU, memoria, procesos, fallos, eventos, logs, estado de servicios.
- Solo accede a métricas aprobadas por el policy engine.
- Ubicación: `ai/context_collector/`

### 2.2 model_runtime
- Ejecuta el modelo de inferencia local o conecta a motor controlado.
- Aislado en sandbox.
- Ubicación: `ai/model_runtime/`

### 2.3 decision_planner
- Interpreta resultados del modelo.
- Transforma observaciones en sugerencias o solicitudes de acción.
- Ubicación: `ai/decision_planner/`

### 2.4 safety_filter
- Clasifica riesgo de cada propuesta antes de enviarla al broker.
- Puede vetar acciones de alto riesgo.
- Ubicación: `ai/safety_filter/`

### 2.5 ai_orchestrator (System Service)
- Coordina el ciclo completo: recopilar → inferir → planificar → solicitar → registrar.
- Corre como servicio del sistema (Capa 2).
- Ubicación: `services/ai_orchestrator/`

---

## 3. Niveles de acceso

| Nivel | Nombre | Capacidades | Restricciones |
|-------|--------|-------------|---------------|
| 0 | Observador | Leer telemetría, detectar anomalías, reportes | Sin ejecución |
| 1 | Asistente | Alertas, priorización, propuestas | Requiere aprobación |
| 2 | Operador restringido | Acciones de bajo riesgo, reversibles | Solo si política permite |
| 3 | Operador privilegiado | Acciones sensibles | Capacidades explícitas + auditoría total |
| 4 | Advisory de kernel | Hints de scheduling, balanceo, caching | Sin modificación directa |

---

## 4. Ciclo de operación

```text
1. context_collector recopila métricas autorizadas
                │
                ▼
2. model_runtime ejecuta inferencia
                │
                ▼
3. decision_planner genera propuesta
                │
                ▼
4. safety_filter evalúa riesgo
                │
        ┌───────┴───────┐
        ▼               ▼
   Riesgo OK       Riesgo Alto
        │               │
        ▼               ▼
5. capability_broker   VETO
        │         (se registra)
        ▼
6. policy_engine evalúa
        │
   ┌────┼────┐
   ▼    ▼    ▼
  OK  DENY  ESCALAR
   │    │    (humano)
   ▼    ▼
7. audit_service registra todo
```

---

## 5. Interfaces

### AI → System
```
ai_orchestrator → IPC → capability_broker
capability_broker → IPC → policy_engine
```

### System → AI
```
context_collector ← IPC ← system services (métricas)
```

---

## 6. Decisiones abiertas

- [ ] Modelo de inferencia (reglas vs. ML local vs. LLM externo).
- [ ] Formato del contexto recopilado.
- [ ] Protocolo entre safety_filter y capability_broker.
- [ ] Sandbox/aislamiento del model_runtime.
- [ ] Mecanismo de feedback loop.
- [ ] Persistencia de decisiones y aprendizaje.

---

## 7. Próximos pasos

1. Diseñar API del context_collector.
2. Definir formato de propuestas del decision_planner.
3. Implementar safety_filter básico (reglas estáticas).
4. Integrar con broker y auditoría.
5. Crear tests de aislamiento IA.
