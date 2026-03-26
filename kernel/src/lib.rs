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
pub mod brane_discovery;
pub mod brane_session;
pub mod crypto;
pub mod dns;
pub mod framebuffer;
pub mod ipc;
pub mod memory;
pub mod module_loader;
pub mod net;
pub mod process;
pub mod ramfs;
pub mod sched;
pub mod security;
pub mod shell;
pub mod socket;
pub mod syscall;
pub mod tty;
pub mod vfs;
pub mod virtio;

#[cfg(test)]
mod tests;
