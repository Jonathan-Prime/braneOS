// ============================================================
// Brane OS Kernel — Entry Point
// ============================================================
//
// This is the bare-metal entry point for the Brane OS kernel.
// It runs on x86_64 with no standard library.
//
// Architecture: hybrid modular kernel
// See: docs/PROJECT_MASTER_SPEC.md §8-§10
//      docs/ARCHITECTURE.md §4-§5
// ============================================================

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

mod serial;
mod gdt;
mod idt;
mod pic;
mod keyboard;

// -----------------------------------------------------------------------
// Kernel Init
// -----------------------------------------------------------------------

/// Kernel entry point.
///
/// Called after the bootloader hands control to us.
/// Initializes subsystems in order:
/// 1. Serial output (logging)
/// 2. GDT + TSS (required for IST)
/// 3. IDT (exception & interrupt handlers)
/// 4. PIC (hardware interrupts)
///
/// After init, the kernel enters a halt loop waiting for interrupts.
#[no_mangle]
pub extern "C" fn _start() -> ! {
    // --- Banner ---
    serial::init();
    serial_println!("===========================================");
    serial_println!("  Brane OS v0.1 — Kernel Booting");
    serial_println!("===========================================");
    serial_println!();

    // --- Subsystem Init ---
    serial_println!("[boot] Initializing subsystems...");
    serial_println!();

    gdt::init();
    serial_println!("[gdt]  Global Descriptor Table loaded.");

    idt::init();
    // idt::init() prints its own status

    pic::init();
    // pic::init() prints its own status

    serial_println!();
    serial_println!("[boot] All Phase 1 subsystems initialized.");
    serial_println!("[boot] Keyboard input active (PS/2).");
    serial_println!("[boot] Timer interrupt active (PIT ~18.2 Hz).");
    serial_println!();
    serial_println!("[boot] Subsystems pending:");
    serial_println!("       - Memory Manager    (Phase 2)");
    serial_println!("       - Scheduler         (Phase 2)");
    serial_println!("       - Syscall Dispatcher (Phase 3)");
    serial_println!("       - IPC Core          (Phase 3)");
    serial_println!("       - Capability Manager (Phase 4)");
    serial_println!("       - Audit Hooks       (Phase 4)");
    serial_println!("       - Module Loader     (Phase 4)");
    serial_println!();
    serial_println!("[boot] Entering halt loop. Waiting for interrupts...");

    halt_loop();
}

// -----------------------------------------------------------------------
// Panic & Halt
// -----------------------------------------------------------------------

/// Panic handler — prints to serial and halts.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!();
    serial_println!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    serial_println!("[KERNEL PANIC] {}", info);
    serial_println!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    halt_loop();
}

/// Halts the CPU in an infinite loop, saving power.
/// Interrupts will still fire and be handled.
pub fn halt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
