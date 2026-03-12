#![allow(dead_code)]
// ============================================================
// Brane OS Kernel — Syscall Dispatcher
// ============================================================
//
// Handles system calls from user space via the `syscall`/`sysret`
// instruction pair (or int 0x80 as fallback).
//
// All syscalls pass through this dispatcher, which:
//   1. Reads the syscall number from RAX.
//   2. Validates the capability (future: via capability_manager).
//   3. Dispatches to the appropriate handler.
//   4. Returns the result in RAX.
//
// Spec reference: ARCHITECTURE.md §5.2.4 (Syscall Dispatcher)
// ============================================================

// -----------------------------------------------------------------------
// Syscall Numbers
// -----------------------------------------------------------------------

/// System call identifiers.
///
/// Convention: grouped by subsystem in ranges of 10.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum SyscallNumber {
    // --- Process (0–9) ---
    Exit          = 0,
    Yield         = 1,
    GetPid        = 2,
    Fork          = 3,
    Exec          = 4,
    WaitPid       = 5,

    // --- Memory (10–19) ---
    Mmap          = 10,
    Munmap        = 11,

    // --- I/O (20–29) ---
    Write         = 20,
    Read          = 21,
    Open          = 22,
    Close         = 23,

    // --- IPC (30–39) ---
    Send          = 30,
    Recv          = 31,
    SendRecv      = 32,

    // --- Capabilities (40–49) ---
    RequestCap    = 40,
    ReleaseCap    = 41,
    CheckCap      = 42,

    // --- System (50–59) ---
    GetTime       = 50,
    GetSystemInfo = 51,

    // --- Brane (60–69) ---
    BraneDiscover = 60,
    BraneConnect  = 61,
    BraneSend     = 62,
    BraneRecv     = 63,
}

impl SyscallNumber {
    /// Convert a raw u64 to a SyscallNumber, if valid.
    pub fn from_raw(n: u64) -> Option<Self> {
        match n {
            0  => Some(Self::Exit),
            1  => Some(Self::Yield),
            2  => Some(Self::GetPid),
            3  => Some(Self::Fork),
            4  => Some(Self::Exec),
            5  => Some(Self::WaitPid),
            10 => Some(Self::Mmap),
            11 => Some(Self::Munmap),
            20 => Some(Self::Write),
            21 => Some(Self::Read),
            22 => Some(Self::Open),
            23 => Some(Self::Close),
            30 => Some(Self::Send),
            31 => Some(Self::Recv),
            32 => Some(Self::SendRecv),
            40 => Some(Self::RequestCap),
            41 => Some(Self::ReleaseCap),
            42 => Some(Self::CheckCap),
            50 => Some(Self::GetTime),
            51 => Some(Self::GetSystemInfo),
            60 => Some(Self::BraneDiscover),
            61 => Some(Self::BraneConnect),
            62 => Some(Self::BraneSend),
            63 => Some(Self::BraneRecv),
            _  => None,
        }
    }
}

// -----------------------------------------------------------------------
// Syscall Result
// -----------------------------------------------------------------------

/// Result of a syscall execution.
#[derive(Debug, Clone, Copy)]
pub enum SyscallResult {
    /// Syscall completed successfully. Value is the return data.
    Ok(u64),
    /// Syscall failed with an error code.
    Err(SyscallError),
}

/// Error codes returned by failed syscalls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum SyscallError {
    /// Unknown or invalid syscall number.
    InvalidSyscall     = -1,
    /// Invalid argument provided.
    InvalidArgument    = -2,
    /// Permission denied (capability check failed).
    PermissionDenied   = -3,
    /// The requested resource was not found.
    NotFound           = -4,
    /// Out of memory.
    OutOfMemory        = -5,
    /// The operation would block and non-blocking was requested.
    WouldBlock         = -6,
    /// IPC: no message available.
    NoMessage          = -7,
    /// IPC: destination task not found.
    InvalidDestination = -8,
    /// Brane: not connected.
    BraneNotConnected  = -9,
    /// Generic / internal error.
    Internal           = -100,
}

impl SyscallResult {
    /// Convert to a raw i64 for returning via RAX.
    pub fn to_raw(self) -> i64 {
        match self {
            SyscallResult::Ok(val) => val as i64,
            SyscallResult::Err(e) => e as i64,
        }
    }
}

// -----------------------------------------------------------------------
// Dispatcher
// -----------------------------------------------------------------------

/// Syscall context — the register state at the time of the syscall.
///
/// On x86_64:
///   rax = syscall number
///   rdi = arg1, rsi = arg2, rdx = arg3, r10 = arg4, r8 = arg5
#[derive(Debug, Clone, Copy)]
pub struct SyscallContext {
    pub number: u64,
    pub arg1: u64,
    pub arg2: u64,
    pub arg3: u64,
    pub arg4: u64,
    pub arg5: u64,
}

/// Main dispatcher: routes a syscall to the appropriate handler.
///
/// This is called from the low-level syscall entry point (assembly
/// stub or int 0x80 handler). Returns a result to be placed in RAX.
pub fn dispatch(ctx: &SyscallContext) -> SyscallResult {
    let syscall = match SyscallNumber::from_raw(ctx.number) {
        Some(s) => s,
        None => {
            crate::serial_println!(
                "[syscall] UNKNOWN syscall number: {}",
                ctx.number
            );
            return SyscallResult::Err(SyscallError::InvalidSyscall);
        }
    };

    crate::serial_println!("[syscall] {:?}(0x{:x}, 0x{:x}, 0x{:x})",
        syscall, ctx.arg1, ctx.arg2, ctx.arg3
    );

    match syscall {
        // --- Process ---
        SyscallNumber::Exit => handle_exit(ctx),
        SyscallNumber::Yield => handle_yield(ctx),
        SyscallNumber::GetPid => handle_getpid(ctx),

        // --- I/O ---
        SyscallNumber::Write => handle_write(ctx),

        // --- IPC ---
        SyscallNumber::Send => handle_ipc_send(ctx),
        SyscallNumber::Recv => handle_ipc_recv(ctx),

        // --- System ---
        SyscallNumber::GetTime => handle_get_time(ctx),
        SyscallNumber::GetSystemInfo => handle_get_system_info(ctx),

        // --- Unimplemented ---
        _ => {
            crate::serial_println!("[syscall] {:?} not yet implemented", syscall);
            SyscallResult::Err(SyscallError::InvalidSyscall)
        }
    }
}

// -----------------------------------------------------------------------
// Syscall Handlers (stubs — to be fully implemented)
// -----------------------------------------------------------------------

fn handle_exit(_ctx: &SyscallContext) -> SyscallResult {
    crate::serial_println!("[syscall] exit() — task termination requested");
    // Future: remove task from scheduler
    SyscallResult::Ok(0)
}

fn handle_yield(_ctx: &SyscallContext) -> SyscallResult {
    // Yield the current time slice to the next task
    crate::sched::SCHEDULER.lock().tick();
    SyscallResult::Ok(0)
}

fn handle_getpid(_ctx: &SyscallContext) -> SyscallResult {
    let scheduler = crate::sched::SCHEDULER.lock();
    match scheduler.current_task() {
        Some(task) => SyscallResult::Ok(task.id),
        None => SyscallResult::Err(SyscallError::Internal),
    }
}

fn handle_write(ctx: &SyscallContext) -> SyscallResult {
    // arg1 = fd (1 = stdout/serial), arg2 = buffer ptr, arg3 = length
    let fd = ctx.arg1;
    let _buf_ptr = ctx.arg2;
    let len = ctx.arg3;

    if fd != 1 {
        return SyscallResult::Err(SyscallError::InvalidArgument);
    }

    // In kernel mode, we can't safely dereference user pointers yet.
    // For now, just acknowledge the write.
    crate::serial_println!("[syscall] write(fd={}, len={}) — stub", fd, len);
    SyscallResult::Ok(len)
}

fn handle_ipc_send(ctx: &SyscallContext) -> SyscallResult {
    // Delegate to the IPC subsystem
    let dest = ctx.arg1;
    crate::serial_println!("[syscall] ipc_send(dest={}) — routing to IPC core", dest);
    // Future: crate::ipc::send(...)
    SyscallResult::Ok(0)
}

fn handle_ipc_recv(_ctx: &SyscallContext) -> SyscallResult {
    crate::serial_println!("[syscall] ipc_recv() — routing to IPC core");
    // Future: crate::ipc::recv(...)
    SyscallResult::Err(SyscallError::NoMessage)
}

fn handle_get_time(_ctx: &SyscallContext) -> SyscallResult {
    let ticks = crate::sched::SCHEDULER.lock().total_ticks();
    SyscallResult::Ok(ticks)
}

fn handle_get_system_info(_ctx: &SyscallContext) -> SyscallResult {
    let scheduler = crate::sched::SCHEDULER.lock();
    let active = scheduler.active_count() as u64;
    SyscallResult::Ok(active)
}
