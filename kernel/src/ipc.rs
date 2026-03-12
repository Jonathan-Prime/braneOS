#![allow(dead_code)]
// ============================================================
// Brane OS Kernel — IPC Core (Message Passing)
// ============================================================
//
// Inter-process communication via message passing.
// Each task has a message queue. Sending a message places it
// in the receiver's queue, and receiving dequeues from own queue.
//
// Message types:
//   - Request / Response: RPC-style communication
//   - Notification: fire-and-forget
//   - BraneRelay: forwarded from an external brane
//
// This is a kernel-space IPC. All messages pass through the
// kernel, which can enforce capability checks and audit logging.
//
// Spec reference: ARCHITECTURE.md §5.2.5 (IPC Core)
// ============================================================

use spin::Mutex;

use crate::sched::TaskId;
use crate::syscall::{SyscallError, SyscallResult};

// -----------------------------------------------------------------------
// Configuration
// -----------------------------------------------------------------------

/// Maximum payload size per message (bytes).
pub const MAX_PAYLOAD: usize = 4096;

/// Maximum messages per task queue.
const QUEUE_CAPACITY: usize = 16;

/// Maximum number of task queues (matches scheduler MAX_TASKS).
const MAX_QUEUES: usize = 64;

// -----------------------------------------------------------------------
// Message Types
// -----------------------------------------------------------------------

/// Type tag for IPC messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    /// RPC-style request expecting a response.
    Request      = 0,
    /// Response to a previous request.
    Response     = 1,
    /// Fire-and-forget notification.
    Notification = 2,
    /// Message relayed from an external brane.
    BraneRelay   = 3,
}

impl MessageType {
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Request),
            1 => Some(Self::Response),
            2 => Some(Self::Notification),
            3 => Some(Self::BraneRelay),
            _ => None,
        }
    }
}

// -----------------------------------------------------------------------
// IPC Message
// -----------------------------------------------------------------------

/// An IPC message with a fixed-size header and payload.
#[derive(Clone)]
pub struct IpcMessage {
    /// Sender task ID.
    pub sender: TaskId,
    /// Receiver task ID.
    pub receiver: TaskId,
    /// Message type.
    pub msg_type: MessageType,
    /// Actual bytes used in payload.
    pub payload_len: usize,
    /// Message payload (fixed buffer, not heap-allocated).
    pub payload: [u8; MAX_PAYLOAD],
}

impl IpcMessage {
    /// Create a new message with the given payload bytes.
    pub fn new(
        sender: TaskId,
        receiver: TaskId,
        msg_type: MessageType,
        data: &[u8],
    ) -> Result<Self, SyscallError> {
        if data.len() > MAX_PAYLOAD {
            return Err(SyscallError::InvalidArgument);
        }
        let mut payload = [0u8; MAX_PAYLOAD];
        payload[..data.len()].copy_from_slice(data);
        Ok(Self {
            sender,
            receiver,
            msg_type,
            payload_len: data.len(),
            payload,
        })
    }

    /// Get the payload as a byte slice.
    pub fn data(&self) -> &[u8] {
        &self.payload[..self.payload_len]
    }
}

// -----------------------------------------------------------------------
// Message Queue (per-task ring buffer)
// -----------------------------------------------------------------------

/// A fixed-size ring buffer for storing incoming messages.
struct MessageQueue {
    messages: [Option<IpcMessage>; QUEUE_CAPACITY],
    head: usize,
    tail: usize,
    count: usize,
}

impl MessageQueue {
    const fn new() -> Self {
        const NONE: Option<IpcMessage> = None;
        Self {
            messages: [NONE; QUEUE_CAPACITY],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    /// Enqueue a message. Returns Err if the queue is full.
    fn push(&mut self, msg: IpcMessage) -> Result<(), SyscallError> {
        if self.count >= QUEUE_CAPACITY {
            return Err(SyscallError::WouldBlock);
        }
        self.messages[self.tail] = Some(msg);
        self.tail = (self.tail + 1) % QUEUE_CAPACITY;
        self.count += 1;
        Ok(())
    }

    /// Dequeue a message. Returns None if empty.
    fn pop(&mut self) -> Option<IpcMessage> {
        if self.count == 0 {
            return None;
        }
        let msg = self.messages[self.head].take();
        self.head = (self.head + 1) % QUEUE_CAPACITY;
        self.count -= 1;
        msg
    }

    /// Number of messages in the queue.
    fn len(&self) -> usize {
        self.count
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }
}

// -----------------------------------------------------------------------
// Global IPC Manager
// -----------------------------------------------------------------------

/// The global IPC manager that holds all task message queues.
pub struct IpcManager {
    queues: [MessageQueue; MAX_QUEUES],
    total_sent: u64,
    total_delivered: u64,
    total_dropped: u64,
}

impl IpcManager {
    const fn new() -> Self {
        const QUEUE: MessageQueue = MessageQueue::new();
        Self {
            queues: [QUEUE; MAX_QUEUES],
            total_sent: 0,
            total_delivered: 0,
            total_dropped: 0,
        }
    }

    /// Send a message to a destination task.
    ///
    /// The message is placed in the receiver's queue.
    /// Returns Ok if delivered, Err if the queue is full or task invalid.
    pub fn send(&mut self, msg: IpcMessage) -> SyscallResult {
        let dest_idx = msg.receiver as usize;

        if dest_idx >= MAX_QUEUES {
            crate::serial_println!(
                "[ipc] send FAILED: invalid destination {}",
                msg.receiver
            );
            return SyscallResult::Err(SyscallError::InvalidDestination);
        }

        self.total_sent += 1;

        match self.queues[dest_idx].push(msg) {
            Ok(()) => {
                self.total_delivered += 1;
                crate::serial_println!(
                    "[ipc] message delivered to task {} (queue len: {})",
                    dest_idx,
                    self.queues[dest_idx].len()
                );
                SyscallResult::Ok(0)
            }
            Err(e) => {
                self.total_dropped += 1;
                crate::serial_println!(
                    "[ipc] send FAILED: task {} queue full",
                    dest_idx
                );
                SyscallResult::Err(e)
            }
        }
    }

    /// Receive a message for a given task.
    ///
    /// Dequeues the oldest message from the task's queue.
    /// Returns NoMessage if the queue is empty.
    pub fn recv(&mut self, task_id: TaskId) -> Result<IpcMessage, SyscallError> {
        let idx = task_id as usize;

        if idx >= MAX_QUEUES {
            return Err(SyscallError::InvalidArgument);
        }

        match self.queues[idx].pop() {
            Some(msg) => {
                crate::serial_println!(
                    "[ipc] task {} received message from task {} ({:?})",
                    task_id, msg.sender, msg.msg_type
                );
                Ok(msg)
            }
            None => Err(SyscallError::NoMessage),
        }
    }

    /// Get the number of pending messages for a task.
    pub fn pending_count(&self, task_id: TaskId) -> usize {
        let idx = task_id as usize;
        if idx >= MAX_QUEUES {
            return 0;
        }
        self.queues[idx].len()
    }

    /// Total messages sent since boot.
    pub fn stats(&self) -> (u64, u64, u64) {
        (self.total_sent, self.total_delivered, self.total_dropped)
    }
}

/// Global IPC manager, protected by a spinlock.
pub static IPC: Mutex<IpcManager> = Mutex::new(IpcManager::new());
