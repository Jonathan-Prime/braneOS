#![allow(dead_code)]
// ============================================================
// Brane OS Kernel — Audit Hooks
// ============================================================
//
// Provides a transversal audit log for all security-relevant
// events in the kernel: syscalls, capability checks, IPC,
// brane connections, and AI actions.
//
// Events are stored in a fixed-size ring buffer and can be
// flushed to the audit_service in user space.
//
// Spec reference: ARCHITECTURE.md §5.2.7
// ============================================================

use spin::Mutex;

use crate::sched::TaskId;
use crate::security::CapabilityId;

// -----------------------------------------------------------------------
// Audit Event Types
// -----------------------------------------------------------------------

/// What kind of action was audited.
#[derive(Debug, Clone, Copy)]
pub enum AuditAction {
    SyscallInvoked(u64),
    CapabilityChecked(CapabilityId),
    CapabilityGranted(CapabilityId),
    CapabilityRevoked(CapabilityId),
    IpcMessageSent { to: TaskId },
    IpcMessageReceived { from: TaskId },
    BraneConnected(u64),
    BraneDisconnected(u64),
    AiActionRequested(u64),
    AiActionAuthorized(u64),
    AiActionDenied(u64),
    PolicyEvaluated(u64),
    TaskCreated(TaskId),
    TaskTerminated(TaskId),
}

/// Outcome of the audited action.
#[derive(Debug, Clone, Copy)]
pub enum AuditResult {
    Success,
    Denied,
    Error(i64),
    Escalated,
}

/// A single audit event.
#[derive(Debug, Clone, Copy)]
pub struct AuditEvent {
    /// Monotonic event sequence number.
    pub seq: u64,
    /// Timer ticks at the time of the event.
    pub tick: u64,
    /// Task that triggered the event.
    pub source: TaskId,
    /// What happened.
    pub action: AuditAction,
    /// Capability used (if any).
    pub capability_used: Option<CapabilityId>,
    /// Result of the action.
    pub result: AuditResult,
}

// -----------------------------------------------------------------------
// Audit Log (Ring Buffer)
// -----------------------------------------------------------------------

const AUDIT_LOG_SIZE: usize = 512;

pub struct AuditLog {
    events: [Option<AuditEvent>; AUDIT_LOG_SIZE],
    head: usize,
    count: usize,
    next_seq: u64,
    total_events: u64,
}

impl AuditLog {
    const fn new() -> Self {
        const NONE: Option<AuditEvent> = None;
        Self {
            events: [NONE; AUDIT_LOG_SIZE],
            head: 0,
            count: 0,
            next_seq: 1,
            total_events: 0,
        }
    }

    /// Record a new audit event.
    ///
    /// If the buffer is full, the oldest event is overwritten.
    pub fn record(
        &mut self,
        source: TaskId,
        action: AuditAction,
        capability_used: Option<CapabilityId>,
        result: AuditResult,
    ) {
        let tick = crate::sched::SCHEDULER.lock().total_ticks();

        let event = AuditEvent {
            seq: self.next_seq,
            tick,
            source,
            action,
            capability_used,
            result,
        };

        let idx = (self.head + self.count) % AUDIT_LOG_SIZE;

        if self.count < AUDIT_LOG_SIZE {
            self.count += 1;
        } else {
            // Ring buffer full — advance head (overwrite oldest)
            self.head = (self.head + 1) % AUDIT_LOG_SIZE;
        }

        self.events[idx] = Some(event);
        self.next_seq += 1;
        self.total_events += 1;
    }

    /// Get the last N events (newest first).
    pub fn last_n(&self, n: usize) -> impl Iterator<Item = &AuditEvent> {
        let take = n.min(self.count);
        let start = if self.count < AUDIT_LOG_SIZE {
            self.count.saturating_sub(take)
        } else {
            (self.head + self.count - take) % AUDIT_LOG_SIZE
        };

        (0..take).filter_map(move |i| {
            let idx = (start + i) % AUDIT_LOG_SIZE;
            self.events[idx].as_ref()
        })
    }

    /// Total events recorded since boot (including overwritten ones).
    pub fn total_events(&self) -> u64 {
        self.total_events
    }

    /// Number of events currently in the buffer.
    pub fn buffered_count(&self) -> usize {
        self.count
    }
}

/// Global audit log.
pub static AUDIT: Mutex<AuditLog> = Mutex::new(AuditLog::new());

// -----------------------------------------------------------------------
// Convenience functions
// -----------------------------------------------------------------------

/// Record a syscall event.
pub fn log_syscall(source: TaskId, syscall_num: u64, result: AuditResult) {
    AUDIT.lock().record(
        source,
        AuditAction::SyscallInvoked(syscall_num),
        None,
        result,
    );
}

/// Record a capability check event.
pub fn log_cap_check(source: TaskId, cap_id: CapabilityId, result: AuditResult) {
    AUDIT.lock().record(
        source,
        AuditAction::CapabilityChecked(cap_id),
        Some(cap_id),
        result,
    );
}

/// Record an IPC send event.
pub fn log_ipc_send(source: TaskId, dest: TaskId, result: AuditResult) {
    AUDIT.lock().record(
        source,
        AuditAction::IpcMessageSent { to: dest },
        None,
        result,
    );
}

/// Record a brane connection event.
pub fn log_brane_connect(source: TaskId, brane_id: u64, result: AuditResult) {
    AUDIT
        .lock()
        .record(source, AuditAction::BraneConnected(brane_id), None, result);
}
