#![allow(dead_code)]
// ============================================================
// Brane OS Kernel — Scheduler (Round-Robin)
// ============================================================
//
// A basic cooperative round-robin scheduler that manages
// kernel tasks. Each task has a unique ID, a priority, and
// a state (Ready, Running, Blocked, Finished).
//
// In this phase, the scheduler only tracks task metadata.
// Context switching will be added in Phase 3 when user space
// transitions are implemented.
//
// Spec reference: ARCHITECTURE.md §5.2.3 (Scheduler)
// ============================================================

use spin::Mutex;

/// Maximum number of tasks the scheduler can manage.
const MAX_TASKS: usize = 64;

/// Global scheduler instance.
pub static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());

/// Unique task identifier.
pub type TaskId = u64;

/// Task priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Priority {
    Idle = 0,
    Low = 1,
    Normal = 2,
    High = 3,
    Realtime = 4,
    System = 5,
}

/// Task execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Finished,
}

/// Represents a scheduled task.
#[derive(Debug, Clone)]
pub struct Task {
    pub id: TaskId,
    pub name: [u8; 32], // fixed-size name (no alloc needed)
    pub name_len: usize,
    pub priority: Priority,
    pub state: TaskState,
    pub ticks: u64, // total ticks this task has received
}

impl Task {
    pub fn new(id: TaskId, name: &str, priority: Priority) -> Self {
        let mut name_buf = [0u8; 32];
        let len = name.len().min(32);
        name_buf[..len].copy_from_slice(&name.as_bytes()[..len]);
        Self {
            id,
            name: name_buf,
            name_len: len,
            priority,
            state: TaskState::Ready,
            ticks: 0,
        }
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("???")
    }
}

/// Round-robin scheduler.
///
/// Manages a fixed-size array of tasks and cycles through
/// them on each tick.
pub struct Scheduler {
    tasks: [Option<Task>; MAX_TASKS],
    current: usize,
    next_id: TaskId,
    tick_count: u64,
}

impl Scheduler {
    pub const fn new() -> Self {
        const NONE: Option<Task> = None;
        Self {
            tasks: [NONE; MAX_TASKS],
            current: 0,
            next_id: 1,
            tick_count: 0,
        }
    }

    /// Add a new task to the scheduler. Returns its TaskId.
    pub fn add_task(&mut self, name: &str, priority: Priority) -> Option<TaskId> {
        for slot in self.tasks.iter_mut() {
            if slot.is_none() {
                let id = self.next_id;
                self.next_id += 1;
                *slot = Some(Task::new(id, name, priority));
                return Some(id);
            }
        }
        None // No free slots
    }

    /// Remove a task by its ID.
    pub fn remove_task(&mut self, id: TaskId) -> bool {
        for slot in self.tasks.iter_mut() {
            if let Some(task) = slot {
                if task.id == id {
                    *slot = None;
                    return true;
                }
            }
        }
        false
    }

    /// Called on every timer tick. Advances the round-robin.
    pub fn tick(&mut self) {
        self.tick_count += 1;

        // Mark current as Ready if it was Running
        if let Some(ref mut task) = self.tasks[self.current] {
            if task.state == TaskState::Running {
                task.state = TaskState::Ready;
            }
        }

        // Find next Ready task (round-robin)
        let start = self.current;
        loop {
            self.current = (self.current + 1) % MAX_TASKS;
            if let Some(ref mut task) = self.tasks[self.current] {
                if task.state == TaskState::Ready {
                    task.state = TaskState::Running;
                    task.ticks += 1;
                    return;
                }
            }
            if self.current == start {
                break; // Wrapped around, no ready tasks
            }
        }
    }

    /// Get the currently running task.
    pub fn current_task(&self) -> Option<&Task> {
        self.tasks[self.current].as_ref()
    }

    /// Total timer ticks since boot.
    pub fn total_ticks(&self) -> u64 {
        self.tick_count
    }

    /// Number of active (non-None) tasks.
    pub fn active_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.is_some()).count()
    }
}
