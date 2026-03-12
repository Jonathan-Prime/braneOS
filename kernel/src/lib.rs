// ============================================================
// Brane OS Kernel — Library Root
// ============================================================
//
// Re-exports and module declarations for the kernel crate.
// As subsystems are implemented, they are declared here.
//
// Spec reference: PROJECT_MASTER_SPEC.md §9.2 (Kernel Core)
// ============================================================

#![no_std]
#![feature(abi_x86_interrupt)]

// --- Core subsystem modules (to be implemented) ---

// pub mod arch;       // Architecture-specific code (x86_64)
// pub mod memory;     // Memory manager (heap, paging)
// pub mod sched;      // Scheduler
// pub mod syscall;    // Syscall dispatcher
// pub mod ipc;        // Inter-process communication
// pub mod security;   // Capability manager
// pub mod audit;      // Audit hooks
