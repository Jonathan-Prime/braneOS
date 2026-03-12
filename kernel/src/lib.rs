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

// --- Core subsystem modules ---

// pub mod arch;       // Architecture-specific code (x86_64) — future
// pub mod syscall;    // Syscall dispatcher — Phase 3
// pub mod ipc;        // Inter-process communication — Phase 3
// pub mod security;   // Capability manager — Phase 4
// pub mod audit;      // Audit hooks — Phase 4
