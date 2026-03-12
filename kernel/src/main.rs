// ============================================================
// Brane OS Kernel — Entry Point
// ============================================================
//
// This is the bare-metal entry point for the Brane OS kernel.
// It runs on x86_64 with no standard library.
//
// Architecture: hybrid modular kernel
// See: docs/PROJECT_MASTER_SPEC.md §8-§10
// ============================================================

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

mod serial;

/// Kernel entry point.
///
/// Called after the bootloader hands control to us.
/// At this stage we:
/// 1. Initialize serial output for early logging
/// 2. Print the boot banner
/// 3. Halt the CPU (nothing else to do yet)
#[no_mangle]
pub extern "C" fn _start() -> ! {
    serial::init();

    serial_println!("===========================================");
    serial_println!("  Brane OS v0.1 — Kernel Booting");
    serial_println!("===========================================");
    serial_println!();
    serial_println!("[boot] Serial output initialized.");
    serial_println!("[boot] Kernel entry point reached.");
    serial_println!("[boot] Architecture: x86_64");
    serial_println!();
    serial_println!("[boot] Subsystems pending initialization:");
    serial_println!("       - Memory Manager");
    serial_println!("       - Interrupt Manager");
    serial_println!("       - Scheduler");
    serial_println!("       - Syscall Dispatcher");
    serial_println!("       - IPC Core");
    serial_println!("       - Capability Manager");
    serial_println!("       - Audit Hooks");
    serial_println!();
    serial_println!("[boot] Halting CPU. Nothing more to do.");

    halt_loop();
}

/// Panic handler — prints to serial and halts.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("[PANIC] {}", info);
    halt_loop();
}

/// Halts the CPU in an infinite loop, saving power.
fn halt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
