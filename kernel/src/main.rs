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

extern crate alloc;

use core::panic::PanicInfo;

mod serial;
mod gdt;
mod idt;
mod pic;
mod keyboard;
mod memory;
mod sched;

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
/// 5. Frame allocator (physical memory)
/// 6. Heap allocator (kernel heap)
/// 7. Scheduler (task management)
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

    // === Phase 1: Core Hardware ===
    serial_println!("[boot] Phase 1: Core hardware...");

    gdt::init();
    serial_println!("[gdt]  Global Descriptor Table loaded.");

    idt::init();
    // idt::init() prints its own message

    pic::init();
    // pic::init() prints its own message

    // === Phase 2: Memory ===
    serial_println!();
    serial_println!("[boot] Phase 2: Memory subsystem...");

    // Initialize the frame allocator with a simulated memory map.
    // In the future, the bootloader will provide the real memory map.
    let mut frame_alloc = memory::frame_allocator::BitmapFrameAllocator::new();

    // Simulate: mark 16 MiB – 128 MiB as usable memory
    // (below 16 MiB is reserved for kernel + BIOS/UEFI)
    frame_alloc.mark_region_free(16 * 1024 * 1024, 128 * 1024 * 1024);

    serial_println!(
        "[mem]  Frame allocator ready: {} free frames ({} MiB usable)",
        frame_alloc.free_count(),
        (frame_alloc.free_count() * 4096) / (1024 * 1024)
    );

    // NOTE: Heap initialization requires a page mapper, which needs
    // the bootloader's page table access. For now, we skip the heap
    // init and rely on stack-allocated structures. The heap will be
    // enabled when we integrate with the `bootloader` crate.
    serial_println!("[heap] Heap allocator: deferred (needs bootloader page tables).");

    // === Phase 2: Scheduler ===
    serial_println!();
    serial_println!("[boot] Phase 2: Scheduler...");
    {
        let mut scheduler = sched::SCHEDULER.lock();
        scheduler.add_task("kernel_idle", sched::Priority::Idle);
        scheduler.add_task("init", sched::Priority::System);
        serial_println!(
            "[sched] Scheduler ready: {} tasks registered.",
            scheduler.active_count()
        );
    }

    // === Summary ===
    serial_println!();
    serial_println!("===========================================");
    serial_println!("  Brane OS — Boot Complete");
    serial_println!("===========================================");
    serial_println!();
    serial_println!("  Phase 1: GDT, IDT, PIC          ✓");
    serial_println!("  Phase 2: Memory, Scheduler       ✓");
    serial_println!();
    serial_println!("  Pending:");
    serial_println!("    - Syscall Dispatcher  (Phase 3)");
    serial_println!("    - IPC Core            (Phase 3)");
    serial_println!("    - Capability Manager  (Phase 4)");
    serial_println!("    - Audit Hooks         (Phase 4)");
    serial_println!("    - Module Loader       (Phase 4)");
    serial_println!("    - Brane Protocol      (Phase 5)");
    serial_println!();
    serial_println!("[boot] Keyboard active. Entering halt loop...");

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
