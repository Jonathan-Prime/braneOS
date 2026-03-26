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

use bootloader_api::{entry_point, BootInfo, BootloaderConfig};

pub const CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(bootloader_api::config::Mapping::Dynamic);
    config
};

entry_point!(kernel_main, config = &CONFIG);

use core::panic::PanicInfo;

// --- Hardware-specific modules (binary-only, not in lib) ---
mod gdt;
mod idt;
mod keyboard;
mod pic;

// --- Re-import shared modules from the lib crate ---
use brane_os_kernel::{
    ai, audit, brane, dns, framebuffer, ipc, memory, module_loader, net, process, ramfs, sched,
    security, serial, shell, socket, syscall, tty, vfs,
};
use brane_os_kernel::serial_println;

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
fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // --- Banner ---
    serial::init();
    serial_println!("===========================================");
    serial_println!("  Brane OS v0.1 — Kernel Booting");
    serial_println!("===========================================");
    serial_println!();

    // === Phase 1.5: Framebuffer (if available) ===
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let info = fb.info();
        let pixel_format = match info.pixel_format {
            bootloader_api::info::PixelFormat::Rgb => framebuffer::PixelFormat::Rgb,
            bootloader_api::info::PixelFormat::Bgr => framebuffer::PixelFormat::Bgr,
            bootloader_api::info::PixelFormat::U8 => framebuffer::PixelFormat::U8,
            _ => framebuffer::PixelFormat::Unknown,
        };

        let buffer = fb.buffer_mut();
        let config = framebuffer::FramebufferConfig {
            buffer_start: buffer.as_mut_ptr() as u64,
            buffer_len: buffer.len(),
            width: info.width,
            height: info.height,
            stride: info.stride,
            bytes_per_pixel: info.bytes_per_pixel,
            pixel_format,
        };
        framebuffer::FB_WRITER.lock().init(config);

        // Write to framebuffer
        use core::fmt::Write;
        let mut fb_writer = framebuffer::FB_WRITER.lock();
        let _ = writeln!(fb_writer, "Brane OS v0.1");
        let _ = writeln!(fb_writer, "Framebuffer: {}x{}", info.width, info.height);
        let _ = writeln!(fb_writer);
    } else {
        serial_println!("[fb]   No framebuffer available (serial only).");
    }

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

    // Initialize the frame allocator with the real bootloader memory map
    let mut frame_alloc = memory::frame_allocator::BitmapFrameAllocator::new();

    let mut usable_bytes: u64 = 0;
    for region in boot_info.memory_regions.iter() {
        use bootloader_api::info::MemoryRegionKind;
        if region.kind == MemoryRegionKind::Usable {
            let start = region.start;
            let size = region.end - region.start;
            frame_alloc.mark_region_free(start, size);
            usable_bytes += size;
        }
    }

    serial_println!(
        "[mem]  Frame allocator ready: {} free frames ({} MiB usable)",
        frame_alloc.free_count(),
        usable_bytes / (1024 * 1024)
    );

    // Initialize paging — get the OffsetPageTable from the bootloader's CR3
    let phys_offset = boot_info
        .physical_memory_offset
        .into_option()
        .expect("bootloader must provide physical_memory_offset");

    let mut mapper = unsafe { memory::paging::init(x86_64::VirtAddr::new(phys_offset)) };
    serial_println!(
        "[page] OffsetPageTable initialized (phys_offset=0x{:X})",
        phys_offset
    );

    // Initialize kernel heap — map pages and set up the linked-list allocator
    memory::heap::init(&mut mapper, &mut frame_alloc).expect("heap initialization failed");
    serial_println!(
        "[heap] Kernel heap initialized: {} KiB at 0x{:X}",
        memory::heap::HEAP_SIZE / 1024,
        memory::heap::HEAP_START
    );

    // Snapshot frame count for the `mem` command
    memory::frame_allocator::snapshot_free_count(&frame_alloc);

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
    serial_println!("  All core subsystems online.");
    serial_println!();

    // === Phase 8: VFS, TTY & Shell ===
    serial_println!("[boot] Phase 8: VFS, TTY & Shell...");

    // Initialize RamFS and mount at /
    ramfs::init();
    {
        let mut vfs_mgr = vfs::VFS.lock();
        let ramfs_ref: &mut dyn vfs::FileSystem = &mut *ramfs::RAMFS.lock();
        let ramfs_ptr: *mut dyn vfs::FileSystem = ramfs_ref;
        unsafe {
            vfs_mgr
                .mount("/", ramfs_ptr)
                .expect("failed to mount ramfs");
        }
    }
    serial_println!("[vfs]  VFS ready. / mounted (RamFS).");
    serial_println!("[tty]  TTY0 ready (serial + framebuffer).");
    serial_println!();

    // === Phase 9: Networking ===
    serial_println!("[boot] Phase 9: Networking...");
    let _net_available = net::init();
    dns::init();
    {
        let dns_resolver = dns::DNS.lock();
        serial_println!(
            "[dns]  DNS resolver ready: {} hosts.",
            dns_resolver.host_count()
        );
    }
    {
        let sock_table = socket::SOCKET_TABLE.lock();
        serial_println!(
            "[sock] Socket subsystem ready ({} slots).",
            sock_table.capacity()
        );
    }
    serial_println!();

    // === Interactive Shell ===
    serial_println!("[boot] Starting brsh (Brane Shell)...");
    serial_println!();
    tty::tty_println("Welcome to Brane OS v0.1");
    tty::tty_println("Type 'help' for available commands.");
    tty::tty_println("");
    shell::prompt();

    // Shell loop: wait for keyboard input, process commands
    loop {
        x86_64::instructions::hlt(); // Wait for interrupt

        // Check if a line is ready
        let mut tty_guard = tty::TTY.lock();
        if tty_guard.has_line() {
            // Copy the line to a local buffer before releasing the lock
            let mut cmd_buf = [0u8; tty::MAX_LINE];
            let line = tty_guard.read_line();
            let len = line.len().min(tty::MAX_LINE);
            cmd_buf[..len].copy_from_slice(&line.as_bytes()[..len]);
            tty_guard.clear_line();
            drop(tty_guard); // Release lock before executing command

            let cmd_str = core::str::from_utf8(&cmd_buf[..len]).unwrap_or("");
            shell::execute(cmd_str);
            shell::prompt();
        }
    }
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
