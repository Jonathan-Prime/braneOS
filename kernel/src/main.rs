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

// --- Hardware-specific modules (binary-only, not in lib) ---
mod gdt;
mod idt;
mod keyboard;
mod pic;

// --- Re-import shared modules from the lib crate ---
use brane_os_kernel::{
    ai, audit, brane, ipc, memory, module_loader, process, sched, security, serial, syscall,
};
use brane_os_kernel::{serial_print, serial_println};

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

    // === Phase 3: Syscalls & IPC ===
    serial_println!();
    serial_println!("[boot] Phase 3: Syscall dispatcher & IPC...");

    // Register a test syscall to verify dispatch
    let test_ctx = syscall::SyscallContext {
        number: syscall::SyscallNumber::GetPid as u64,
        arg1: 0,
        arg2: 0,
        arg3: 0,
        arg4: 0,
        arg5: 0,
    };
    let result = syscall::dispatch(&test_ctx);
    serial_println!(
        "[sys]  Syscall dispatcher ready. Test GetPid => {}",
        result.to_raw()
    );

    // Test IPC: send a message between tasks
    {
        let msg = ipc::IpcMessage::new(
            1, // sender: init
            0, // receiver: kernel_idle
            ipc::MessageType::Notification,
            b"boot_complete",
        )
        .unwrap();
        let _ = ipc::IPC.lock().send(msg);
        let pending = ipc::IPC.lock().pending_count(0);
        serial_println!(
            "[ipc]  IPC core ready. Task 0 has {} pending message(s).",
            pending
        );
    }

    // === Phase 4: Security & Adaptability ===
    serial_println!();
    serial_println!("[boot] Phase 4: Security & adaptability...");

    // Grant initial capabilities
    {
        use security::{CapPermissions, CapScope, RiskLevel};
        let mut cap_mgr = security::CAP_MANAGER.lock();
        // kernel_idle gets basic read
        cap_mgr
            .grant(
                0,
                CapScope::System,
                CapPermissions::READ,
                RiskLevel::Low,
                false,
            )
            .ok();
        // init gets full system access
        cap_mgr
            .grant(
                1,
                CapScope::System,
                CapPermissions::READ
                    .union(CapPermissions::WRITE)
                    .union(CapPermissions::EXECUTE)
                    .union(CapPermissions::IPC_SEND)
                    .union(CapPermissions::IPC_RECV),
                RiskLevel::High,
                true,
            )
            .ok();
        serial_println!(
            "[cap]  Capability manager ready: {} active caps.",
            cap_mgr.active_count()
        );
    }

    // Record boot event in audit log
    audit::AUDIT.lock().record(
        0,
        audit::AuditAction::TaskCreated(0),
        None,
        audit::AuditResult::Success,
    );
    audit::AUDIT.lock().record(
        0,
        audit::AuditAction::TaskCreated(1),
        None,
        audit::AuditResult::Success,
    );
    serial_println!(
        "[aud]  Audit log ready: {} events recorded.",
        audit::AUDIT.lock().total_events()
    );

    // Register built-in kernel sub-branes
    {
        let mut loader = module_loader::MODULE_LOADER.lock();
        loader.load("serial_driver", (0, 1, 0), &[]).ok();
        loader.load("keyboard_driver", (0, 1, 0), &[]).ok();
        loader.load("timer_driver", (0, 1, 0), &[]).ok();
        serial_println!(
            "[mod]  Module loader ready: {} modules registered.",
            loader.loaded_count()
        );
    }

    // === Phase 5: Brane Protocol ===
    serial_println!();
    serial_println!("[boot] Phase 5: Brane Protocol...");

    {
        let mut brane_mgr = brane::BRANE.lock();
        // Set our local brane ID (derived from hardware ID in a real system)
        brane_mgr.set_local_id(0xBEA1);

        // Simulate discovering nearby branes
        let phone_id = brane_mgr
            .register_discovered(
                "pixel-9",
                brane::BraneType::Companion,
                brane::Transport::Bluetooth,
                0x07, // advertises read + write + execute
                85,
            )
            .unwrap();

        let _server_id = brane_mgr
            .register_discovered(
                "home-server",
                brane::BraneType::Peer,
                brane::Transport::TcpIp,
                0xFF, // advertises all caps
                100,
            )
            .unwrap();

        brane_mgr
            .register_discovered(
                "temp-sensor-01",
                brane::BraneType::IoT,
                brane::Transport::Ble,
                0x01, // read only
                70,
            )
            .ok();

        serial_println!(
            "[brane] {} branes discovered.",
            brane_mgr.discovered_count()
        );

        // Connect to the companion phone
        let session = brane_mgr.connect(phone_id, 1).unwrap();

        // Send a test telemetry message
        let msg = brane::BraneMessage::new(
            brane::BraneMessageType::Telemetry,
            0xBEA1,
            phone_id,
            session,
            b"{\"status\":\"boot_complete\",\"phase\":5}",
        )
        .unwrap();
        brane_mgr.send(session, &msg).ok();

        serial_println!(
            "[brane] Brane Protocol ready: {} active session(s).",
            brane_mgr.active_session_count()
        );
    }

    // === Phase 6: AI Subsystem ===
    serial_println!();
    serial_println!("[boot] Phase 6: AI subsystem...");
    {
        let mut engine = ai::AI_ENGINE.lock();
        engine.set_mode(ai::AiMode::ObserveOnly);
        engine.observe(
            ai::AiCategory::Resource,
            ai::AiSeverity::Info,
            "Boot complete. All subsystems nominal.",
            None,
        );
        engine.observe(
            ai::AiCategory::Security,
            ai::AiSeverity::Low,
            "2 capabilities granted during boot.",
            None,
        );
        let stats = engine.stats();
        serial_println!(
            "[ai]   AI engine ready (mode={:?}, observations={}).",
            stats.mode,
            stats.total_observations
        );
    }

    // === Phase 7: User Space Init ===
    serial_println!();
    serial_println!("[boot] Phase 7: User space...");
    {
        let mut table = process::PROCESS_TABLE.lock();
        // Create PID 1 — the init process
        let init_pid = table.create("init", None, 1).unwrap();
        table.start(init_pid);

        // Create initial system services
        let _log_pid = table.create("log_service", Some(init_pid), 2);
        let _net_pid = table.create("network_service", Some(init_pid), 3);
        let _brane_pid = table.create("brane_service", Some(init_pid), 4);

        serial_println!(
            "[proc] Process table ready: {} active processes.",
            table.active_count()
        );
    }

    // === Summary ===
    serial_println!();
    serial_println!("===========================================");
    serial_println!("  Brane OS v0.1 — Boot Complete");
    serial_println!("===========================================");
    serial_println!();
    serial_println!("  Phase 1: GDT, IDT, PIC          ✓");
    serial_println!("  Phase 2: Memory, Scheduler       ✓");
    serial_println!("  Phase 3: Syscalls, IPC           ✓");
    serial_println!("  Phase 4: Caps, Audit, Modules    ✓");
    serial_println!("  Phase 5: Brane Protocol          ✓");
    serial_println!("  Phase 6: AI Subsystem            ✓");
    serial_println!("  Phase 7: User Space              ✓");
    serial_println!();
    serial_println!("  All subsystems online.");
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
