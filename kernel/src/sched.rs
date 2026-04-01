#![allow(dead_code)]
// ============================================================
// Brane OS Kernel — Scheduler (Round-Robin + Context Switch)
// ============================================================
//
// A cooperative round-robin scheduler that manages kernel tasks.
// Each task has a unique ID, a priority, a state, and a saved
// CPU context (`TaskContext`) so that it can be suspended and
// resumed at will.
//
// Context switching (Phase 10):
//   - Each task owns a `KernelStack` and a `TaskContext`.
//   - `yield_current()` triggers a cooperative switch to the
//     next ready task by calling `context::switch_context`.
//   - The timer tick marks tasks as Ready and can also trigger
//     a switch from an interrupt handler.
//
// Spec reference: ARCHITECTURE.md §5.2.3 (Scheduler)
//                 ROADMAP.md Fase 10 (Context switching real)
// ============================================================

use crate::context::{self, KernelStack, TaskContext};
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

// -----------------------------------------------------------------------
// Task descriptor
// -----------------------------------------------------------------------

/// Represents a scheduled kernel task.
///
/// Each task owns its execution context and kernel stack.
/// The stack is `Box`-allocated at task creation and lives
/// for the full lifetime of the task.
pub struct Task {
    pub id: TaskId,
    pub name: [u8; 32],
    pub name_len: usize,
    pub priority: Priority,
    pub state: TaskState,
    pub ticks: u64,

    /// Saved CPU state. Updated every time the task is preempted.
    pub ctx: TaskContext,

    /// Owning pointer to this task's kernel stack.
    ///
    /// `None` for tasks that were created without a real entry
    /// point (e.g. the initial boot task that reuses the bootloader
    /// stack).
    pub stack: Option<StackBox>,
}

/// Heap-allocated `KernelStack` wrapped in a raw pointer so that
/// `Task` can be stored in a fixed-size array without `Box<T>` in
/// `no_std`. We manage lifetimes manually.
///
/// Safety invariant: the pointer is always valid while the `Task`
/// exists.  When the task is dropped (slot → None), `StackBox::drop`
/// deallocates through `Box`.
pub struct StackBox(*mut KernelStack);

// SAFETY: tasks are only ever accessed behind the `Mutex<Scheduler>`
// so there is no concurrent access without synchronization.
unsafe impl Send for StackBox {}
unsafe impl Sync for StackBox {}

impl Drop for StackBox {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // Re-create the Box so Rust drops (and deallocates) it.
            let _ = unsafe { alloc::boxed::Box::from_raw(self.0) };
        }
    }
}

extern crate alloc;
use alloc::boxed::Box;

impl Task {
    /// Create a metadata-only task (no stack, no entry point).
    ///
    /// Used for the initial boot task which already has a stack
    /// provided by the bootloader.
    pub fn new_boot(id: TaskId, name: &str, priority: Priority) -> Self {
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
            ctx: TaskContext::empty(),
            stack: None,
        }
    }

    /// Create a task with its own kernel stack, ready to start at `entry`.
    ///
    /// The entry function must have the signature:
    /// ```rust
    /// extern "C" fn my_task() -> ! { ... }
    /// ```
    /// It must never return; call `exit_task()` or `loop {}` at the end.
    pub fn new_with_stack(id: TaskId, name: &str, priority: Priority, entry: fn() -> !) -> Self {
        let mut name_buf = [0u8; 32];
        let len = name.len().min(32);
        name_buf[..len].copy_from_slice(&name.as_bytes()[..len]);

        // Allocate the stack on the kernel heap.
        let stack_box = Box::new(KernelStack::new());
        let stack_top = stack_box.top();
        let stack_ptr = Box::into_raw(stack_box);

        // Build the initial context pointing to entry.
        let ctx = TaskContext::new_task(stack_top, entry as u64);

        Self {
            id,
            name: name_buf,
            name_len: len,
            priority,
            state: TaskState::Ready,
            ticks: 0,
            ctx,
            stack: Some(StackBox(stack_ptr)),
        }
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("???")
    }
}

// -----------------------------------------------------------------------
// Scheduler
// -----------------------------------------------------------------------

/// Round-robin scheduler with cooperative context switching.
pub struct Scheduler {
    tasks: [Option<Task>; MAX_TASKS],
    current: usize,
    next_id: TaskId,
    tick_count: u64,
}

// We need `const fn new()` so the static can be initialized at compile time.
// `Option<Task>` is not Copy because `Task` contains a `StackBox`, so we
// use a helper to build the array.
impl Scheduler {
    pub const fn new() -> Self {
        // SAFETY: `Option<Task>` is a valid all-zero value (None).
        // We cannot use `[None; MAX_TASKS]` directly because `Task`
        // is not Copy, so we use MaybeUninit to build the array.
        const NONE: Option<Task> = None;
        Self {
            tasks: [NONE; MAX_TASKS],
            current: 0,
            next_id: 1,
            tick_count: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Task management
    // -----------------------------------------------------------------------

    /// Add a boot task (no stack allocation).
    ///
    /// Use this for the initial kernel context that is already
    /// running on the bootloader stack.
    pub fn add_task(&mut self, name: &str, priority: Priority) -> Option<TaskId> {
        for slot in self.tasks.iter_mut() {
            if slot.is_none() {
                let id = self.next_id;
                self.next_id += 1;
                *slot = Some(Task::new_boot(id, name, priority));
                return Some(id);
            }
        }
        None
    }

    /// Add a task that starts execution at `entry` on its own stack.
    ///
    /// Returns the TaskId, or `None` if the task table is full.
    pub fn add_task_with_entry(
        &mut self,
        name: &str,
        priority: Priority,
        entry: fn() -> !,
    ) -> Option<TaskId> {
        for slot in self.tasks.iter_mut() {
            if slot.is_none() {
                let id = self.next_id;
                self.next_id += 1;
                *slot = Some(Task::new_with_stack(id, name, priority, entry));
                return Some(id);
            }
        }
        None
    }

    /// Remove a task by ID. Returns `true` if found and removed.
    pub fn remove_task(&mut self, id: TaskId) -> bool {
        for slot in self.tasks.iter_mut() {
            if let Some(task) = slot {
                if task.id == id {
                    *slot = None; // Drop releases the StackBox
                    return true;
                }
            }
        }
        false
    }

    /// Block a task (move it to Blocked state).
    pub fn block_task(&mut self, id: TaskId) -> bool {
        for slot in self.tasks.iter_mut() {
            if let Some(task) = slot {
                if task.id == id {
                    task.state = TaskState::Blocked;
                    return true;
                }
            }
        }
        false
    }

    /// Unblock a task (move it back to Ready).
    pub fn unblock_task(&mut self, id: TaskId) -> bool {
        for slot in self.tasks.iter_mut() {
            if let Some(task) = slot {
                if task.id == id && task.state == TaskState::Blocked {
                    task.state = TaskState::Ready;
                    return true;
                }
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Scheduling
    // -----------------------------------------------------------------------

    /// Called on every timer tick. Advances round-robin state.
    ///
    /// This updates task states but does NOT perform the context
    /// switch itself; the actual `switch_context` call happens in
    /// `yield_current()` to avoid holding the scheduler lock across
    /// the assembly switch.
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
                break; // Wrapped around — no ready tasks
            }
        }
    }

    /// Pick the next ready task index without advancing `self.current`.
    ///
    /// Returns `None` if there is no ready task other than the current one.
    fn next_ready(&self) -> Option<usize> {
        let start = self.current;
        let mut idx = (start + 1) % MAX_TASKS;
        loop {
            if let Some(ref task) = self.tasks[idx] {
                if task.state == TaskState::Ready {
                    return Some(idx);
                }
            }
            idx = (idx + 1) % MAX_TASKS;
            if idx == start {
                break;
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Context-switch helpers (called from yield_current)
    // -----------------------------------------------------------------------

    /// Returns raw pointers to the old and new task contexts so that
    /// `yield_current` can perform the switch after releasing the lock.
    ///
    /// Returns `None` if there is no other ready task to switch to.
    ///
    /// SAFETY: The caller must ensure the scheduler lock is released
    /// before actually calling `switch_context` with these pointers.
    pub fn prepare_switch(&mut self) -> Option<(*mut TaskContext, *const TaskContext)> {
        let next_idx = self.next_ready()?;

        // Mark current → Ready, next → Running
        if let Some(ref mut task) = self.tasks[self.current] {
            if task.state == TaskState::Running {
                task.state = TaskState::Ready;
            }
        }
        if let Some(ref mut task) = self.tasks[next_idx] {
            task.state = TaskState::Running;
            task.ticks += 1;
        }

        let old_ptr = self.tasks[self.current]
            .as_mut()
            .map(|t| &mut t.ctx as *mut TaskContext)?;

        let new_ptr = self.tasks[next_idx]
            .as_ref()
            .map(|t| &t.ctx as *const TaskContext)?;

        self.current = next_idx;

        Some((old_ptr, new_ptr))
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    pub fn current_task(&self) -> Option<&Task> {
        self.tasks[self.current].as_ref()
    }

    pub fn current_task_id(&self) -> Option<TaskId> {
        self.tasks[self.current].as_ref().map(|t| t.id)
    }

    pub fn total_ticks(&self) -> u64 {
        self.tick_count
    }

    pub fn active_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.is_some()).count()
    }

    /// Returns a short summary of all tasks (for the `sched` shell command).
    pub fn snapshot(&self) -> [Option<TaskSnapshot>; MAX_TASKS] {
        const NONE: Option<TaskSnapshot> = None;
        let mut out = [NONE; MAX_TASKS];
        for (i, slot) in self.tasks.iter().enumerate() {
            if let Some(task) = slot {
                out[i] = Some(TaskSnapshot {
                    id: task.id,
                    name: task.name,
                    name_len: task.name_len,
                    priority: task.priority,
                    state: task.state,
                    ticks: task.ticks,
                    rsp: task.ctx.rsp,
                    rip: task.ctx.rip,
                });
            }
        }
        out
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// A lightweight snapshot of a task's state (no heap, Copy-able).
#[derive(Debug, Clone, Copy)]
pub struct TaskSnapshot {
    pub id: TaskId,
    pub name: [u8; 32],
    pub name_len: usize,
    pub priority: Priority,
    pub state: TaskState,
    pub ticks: u64,
    pub rsp: u64,
    pub rip: u64,
}

impl TaskSnapshot {
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("???")
    }
}

// -----------------------------------------------------------------------
// Public API: cooperative yield
// -----------------------------------------------------------------------

/// Voluntarily yield the current task and switch to the next ready task.
///
/// This is the primary cooperative scheduling primitive. A task should
/// call this when it is waiting for I/O, sleeping, or has finished its
/// current work quantum.
///
/// # Safety
///
/// Must not be called while holding any spinlock that might be needed
/// by another task (deadlock). The scheduler lock is released before
/// the actual context switch occurs.
pub fn yield_current() {
    // Get the raw context pointers while holding the lock, then
    // immediately release it before doing the switch.
    let switch_pair = SCHEDULER.lock().prepare_switch();

    if let Some((old_ptr, new_ptr)) = switch_pair {
        // SAFETY:
        // - Both pointers come from live task descriptors inside the
        //   scheduler's fixed-size array, so they are valid for the
        //   lifetime of the scheduler (static).
        // - The scheduler lock is NOT held during the switch.
        // - Interrupts may fire during the switch; the IDT handlers
        //   do NOT acquire the scheduler lock, so there's no deadlock.
        unsafe {
            context::switch_context(old_ptr, new_ptr);
        }
    }
    // If there is no other ready task, we simply return and the
    // current task continues.
}
