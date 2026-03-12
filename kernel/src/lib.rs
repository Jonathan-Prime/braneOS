// ============================================================
// Brane OS Kernel — Library Root
// ============================================================
//
// Re-exports and module declarations for the kernel crate.
// All kernel subsystems are declared here so they can be
// used by both the binary target (main.rs) and unit tests.
//
// Spec reference: PROJECT_MASTER_SPEC.md §9.2 (Kernel Core)
// ============================================================

#![no_std]

pub mod serial;

pub mod ai;
pub mod audit;
pub mod brane;
pub mod ipc;
pub mod memory;
pub mod module_loader;
pub mod process;
pub mod sched;
pub mod security;
pub mod syscall;

#[cfg(test)]
mod tests;
