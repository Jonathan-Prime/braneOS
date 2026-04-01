// ============================================================
// Brane OS Kernel — Unit Tests
// ============================================================
//
// Tests for core kernel subsystems. Run with:
//   cargo test --lib
//
// These tests run in the host environment (not bare-metal),
// so they test logic only — not hardware interactions.
// ============================================================

#[cfg(test)]
mod frame_allocator_tests {
    use crate::memory::frame_allocator::BitmapFrameAllocator;

    #[test]
    fn new_allocator_has_no_free_frames() {
        let alloc = BitmapFrameAllocator::new();
        assert_eq!(alloc.free_count(), 0);
    }

    #[test]
    fn mark_region_free_increases_count() {
        let mut alloc = BitmapFrameAllocator::new();
        // Mark 1 MiB as free (256 frames)
        alloc.mark_region_free(0, 1024 * 1024);
        assert_eq!(alloc.free_count(), 256);
    }

    #[test]
    fn allocate_returns_frame_and_decreases_count() {
        let mut alloc = BitmapFrameAllocator::new();
        alloc.mark_region_free(0, 4096 * 10); // 10 frames
        assert_eq!(alloc.free_count(), 10);

        let frame = alloc.allocate();
        assert!(frame.is_some());
        assert_eq!(alloc.free_count(), 9);
    }

    #[test]
    fn allocate_returns_none_when_empty() {
        let mut alloc = BitmapFrameAllocator::new();
        assert_eq!(alloc.allocate(), None);
    }

    #[test]
    fn deallocate_returns_frame() {
        let mut alloc = BitmapFrameAllocator::new();
        alloc.mark_region_free(0, 4096);
        let addr = alloc.allocate().unwrap();
        assert_eq!(alloc.free_count(), 0);

        alloc.deallocate(addr);
        assert_eq!(alloc.free_count(), 1);
    }

    #[test]
    fn mark_region_used_reduces_count() {
        let mut alloc = BitmapFrameAllocator::new();
        alloc.mark_region_free(0, 4096 * 10);
        assert_eq!(alloc.free_count(), 10);

        alloc.mark_region_used(0, 4096 * 3);
        assert_eq!(alloc.free_count(), 7);
    }
}

#[cfg(test)]
mod scheduler_tests {
    use crate::sched::{Priority, Scheduler};

    #[test]
    fn new_scheduler_has_no_tasks() {
        let sched = Scheduler::new();
        assert_eq!(sched.active_count(), 0);
    }

    #[test]
    fn add_task_returns_id() {
        let mut sched = Scheduler::new();
        let id = sched.add_task("test_task", Priority::Normal);
        assert!(id.is_some());
        assert_eq!(sched.active_count(), 1);
    }

    #[test]
    fn remove_task_succeeds() {
        let mut sched = Scheduler::new();
        let id = sched.add_task("temp_task", Priority::Low).unwrap();
        assert!(sched.remove_task(id));
        assert_eq!(sched.active_count(), 0);
    }

    #[test]
    fn remove_nonexistent_task_returns_false() {
        let mut sched = Scheduler::new();
        assert!(!sched.remove_task(9999));
    }

    #[test]
    fn tick_advances_round_robin() {
        let mut sched = Scheduler::new();
        sched.add_task("task_a", Priority::Normal);
        sched.add_task("task_b", Priority::Normal);

        sched.tick();
        assert_eq!(sched.total_ticks(), 1);

        sched.tick();
        assert_eq!(sched.total_ticks(), 2);
    }
}

#[cfg(test)]
mod syscall_tests {
    use crate::syscall::{SyscallError, SyscallNumber, SyscallResult};

    #[test]
    fn syscall_number_from_valid_raw() {
        assert_eq!(SyscallNumber::from_raw(0), Some(SyscallNumber::Exit));
        assert_eq!(SyscallNumber::from_raw(2), Some(SyscallNumber::GetPid));
        assert_eq!(
            SyscallNumber::from_raw(60),
            Some(SyscallNumber::BraneDiscover)
        );
    }

    #[test]
    fn syscall_number_from_invalid_raw() {
        assert_eq!(SyscallNumber::from_raw(999), None);
        assert_eq!(SyscallNumber::from_raw(100), None);
    }

    #[test]
    fn syscall_result_to_raw() {
        let ok = SyscallResult::Ok(42);
        assert_eq!(ok.to_raw(), 42);

        let err = SyscallResult::Err(SyscallError::PermissionDenied);
        assert_eq!(err.to_raw(), -3);
    }
}

#[cfg(test)]
mod capability_tests {
    use crate::security::{CapError, CapPermissions, CapScope, CapabilityManager, RiskLevel};

    #[test]
    fn grant_and_check_capability() {
        let mut mgr = CapabilityManager::new();
        mgr.grant(
            1,
            CapScope::System,
            CapPermissions::READ,
            RiskLevel::Low,
            true,
        )
        .unwrap();

        let result = mgr.check(1, CapPermissions::READ, CapScope::System);
        assert!(result.is_ok());
    }

    #[test]
    fn check_missing_capability_fails() {
        let mgr = CapabilityManager::new();
        let result = mgr.check(1, CapPermissions::WRITE, CapScope::System);
        assert_eq!(result, Err(CapError::PermissionDenied));
    }

    #[test]
    fn revoke_capability() {
        let mut mgr = CapabilityManager::new();
        let id = mgr
            .grant(
                1,
                CapScope::System,
                CapPermissions::READ,
                RiskLevel::Low,
                true,
            )
            .unwrap();
        assert!(mgr.revoke(id).is_ok());
        assert_eq!(mgr.active_count(), 0);
    }

    #[test]
    fn revoke_non_revocable_fails() {
        let mut mgr = CapabilityManager::new();
        let id = mgr
            .grant(
                1,
                CapScope::System,
                CapPermissions::READ,
                RiskLevel::Low,
                false,
            )
            .unwrap();
        assert_eq!(mgr.revoke(id), Err(CapError::PermissionDenied));
    }

    #[test]
    fn permission_union_and_has() {
        let perm = CapPermissions::READ.union(CapPermissions::WRITE);
        assert!(perm.has(CapPermissions::READ));
        assert!(perm.has(CapPermissions::WRITE));
        assert!(!perm.has(CapPermissions::EXECUTE));
    }
}

#[cfg(test)]
mod ipc_tests {
    use crate::ipc::{IpcMessage, MessageType};

    #[test]
    fn create_message() {
        let msg = IpcMessage::new(1, 2, MessageType::Notification, b"hello");
        assert!(msg.is_ok());
        let msg = msg.unwrap();
        assert_eq!(msg.sender, 1);
        assert_eq!(msg.receiver, 2);
        assert_eq!(msg.data(), b"hello");
    }

    #[test]
    fn message_too_large_fails() {
        let big_data = [0u8; 5000]; // > MAX_PAYLOAD (4096)
        let msg = IpcMessage::new(1, 2, MessageType::Request, &big_data);
        assert!(msg.is_err());
    }

    #[test]
    fn message_type_from_raw() {
        assert_eq!(MessageType::from_raw(0), Some(MessageType::Request));
        assert_eq!(MessageType::from_raw(3), Some(MessageType::BraneRelay));
        assert_eq!(MessageType::from_raw(99), None);
    }
}

#[cfg(test)]
mod module_loader_tests {
    use crate::module_loader::{ModuleError, ModuleLoader, ModuleStatus};

    #[test]
    fn load_module() {
        let mut loader = ModuleLoader::new();
        let id = loader.load("test_mod", (1, 0, 0), &[]);
        assert!(id.is_ok());
        assert_eq!(loader.loaded_count(), 1);
    }

    #[test]
    fn duplicate_module_fails() {
        let mut loader = ModuleLoader::new();
        loader.load("test_mod", (1, 0, 0), &[]).unwrap();
        let result = loader.load("test_mod", (2, 0, 0), &[]);
        assert_eq!(result, Err(ModuleError::AlreadyLoaded));
    }

    #[test]
    fn unload_module() {
        let mut loader = ModuleLoader::new();
        let id = loader.load("temp_mod", (1, 0, 0), &[]).unwrap();
        assert!(loader.unload(id).is_ok());
        assert_eq!(loader.loaded_count(), 0);
    }

    #[test]
    fn unload_with_dependents_fails() {
        let mut loader = ModuleLoader::new();
        let base = loader.load("base", (1, 0, 0), &[]).unwrap();
        loader.load("child", (1, 0, 0), &[base]).unwrap();
        assert_eq!(loader.unload(base), Err(ModuleError::HasDependents));
    }

    #[test]
    fn start_and_suspend_module() {
        let mut loader = ModuleLoader::new();
        let id = loader.load("svc", (1, 0, 0), &[]).unwrap();
        assert!(loader.start(id).is_ok());
        assert_eq!(loader.info(id).unwrap().status, ModuleStatus::Running);
        assert!(loader.suspend(id).is_ok());
        assert_eq!(loader.info(id).unwrap().status, ModuleStatus::Suspended);
    }
}

#[cfg(test)]
mod brane_tests {
    use crate::brane::{
        BraneError, BraneManager, BraneMessage, BraneMessageType, BraneType, Transport,
    };

    #[test]
    fn discover_brane() {
        let mut mgr = BraneManager::new();
        let id = mgr.register_discovered(
            "test-phone",
            BraneType::Companion,
            Transport::Bluetooth,
            0x07,
            90,
        );
        assert!(id.is_ok());
        assert_eq!(mgr.discovered_count(), 1);
    }

    #[test]
    fn connect_to_brane() {
        let mut mgr = BraneManager::new();
        mgr.set_local_id(1);
        let brane_id = mgr
            .register_discovered("srv", BraneType::Peer, Transport::TcpIp, 0xFF, 100)
            .unwrap();
        let session = mgr.connect(brane_id, 0);
        assert!(session.is_ok());
        assert_eq!(mgr.active_session_count(), 1);
    }

    #[test]
    fn double_connect_fails() {
        let mut mgr = BraneManager::new();
        mgr.set_local_id(1);
        let id = mgr
            .register_discovered("dev", BraneType::IoT, Transport::Ble, 0x01, 50)
            .unwrap();
        mgr.connect(id, 0).unwrap();
        assert_eq!(mgr.connect(id, 0), Err(BraneError::AlreadyConnected));
    }

    #[test]
    fn create_brane_message() {
        let msg = BraneMessage::new(BraneMessageType::Data, 1, 2, 1, b"payload");
        assert!(msg.is_ok());
        assert_eq!(msg.unwrap().data(), b"payload");
    }
}

#[cfg(test)]
mod process_tests {
    use crate::process::ProcessTable;

    #[test]
    fn create_process() {
        let mut table = ProcessTable::new();
        let pid = table.create("init", None, 0);
        assert!(pid.is_some());
        assert_eq!(table.active_count(), 1);
    }

    #[test]
    fn start_process() {
        let mut table = ProcessTable::new();
        let pid = table.create("svc", None, 0).unwrap();
        assert!(table.start(pid));
    }

    #[test]
    fn terminate_process() {
        let mut table = ProcessTable::new();
        let pid = table.create("temp", None, 0).unwrap();
        table.start(pid);
        assert!(table.terminate(pid, 0));
        assert_eq!(table.active_count(), 0);
    }
}

#[cfg(test)]
mod ai_tests {
    use crate::ai::{AiCategory, AiEngine, AiMode, AiSeverity};

    #[test]
    fn default_mode_is_observe_only() {
        let engine = AiEngine::new();
        assert_eq!(engine.mode(), AiMode::ObserveOnly);
    }

    #[test]
    fn disabled_mode_ignores_observations() {
        let mut engine = AiEngine::new();
        engine.set_mode(AiMode::Disabled);
        let id = engine.observe(AiCategory::Resource, AiSeverity::Info, "test", None);
        assert_eq!(id, 0);
    }

    #[test]
    fn observe_returns_incrementing_ids() {
        let mut engine = AiEngine::new();
        let id1 = engine.observe(AiCategory::Security, AiSeverity::Low, "evt1", None);
        let id2 = engine.observe(AiCategory::Security, AiSeverity::Low, "evt2", None);
        assert_eq!(id2, id1 + 1);
    }
}

// -----------------------------------------------------------------------
// Context switching tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod context_tests {
    use crate::context::{KernelStack, TaskContext};

    #[test]
    fn empty_context_is_all_zeros() {
        let ctx = TaskContext::empty();
        assert_eq!(ctx.rbx, 0);
        assert_eq!(ctx.r12, 0);
        assert_eq!(ctx.r13, 0);
        assert_eq!(ctx.r14, 0);
        assert_eq!(ctx.r15, 0);
        assert_eq!(ctx.rbp, 0);
        assert_eq!(ctx.rsp, 0);
        assert_eq!(ctx.rip, 0);
    }

    #[test]
    fn new_task_context_sets_rip() {
        let fake_entry: u64 = 0xDEAD_BEEF_0000_1234;
        let stack_top: u64 = 0xFFFF_8000_0001_0000;
        let ctx = TaskContext::new_task(stack_top, fake_entry);
        assert_eq!(ctx.rip, fake_entry);
    }

    #[test]
    fn new_task_context_rsp_below_stack_top() {
        let stack_top: u64 = 0xFFFF_8000_0001_0000;
        let ctx = TaskContext::new_task(stack_top, 0x1000);
        assert!(ctx.rsp < stack_top, "RSP must be below stack top");
    }

    #[test]
    fn new_task_context_rbp_equals_stack_top() {
        let stack_top: u64 = 0xFFFF_8000_0001_0000;
        let ctx = TaskContext::new_task(stack_top, 0x1000);
        assert_eq!(ctx.rbp, stack_top);
    }

    #[test]
    fn new_task_callee_regs_zero() {
        let ctx = TaskContext::new_task(0xFFFF_0000, 0x1000);
        assert_eq!(ctx.rbx, 0);
        assert_eq!(ctx.r12, 0);
        assert_eq!(ctx.r13, 0);
        assert_eq!(ctx.r14, 0);
        assert_eq!(ctx.r15, 0);
    }

    #[test]
    fn context_is_copy() {
        let ctx = TaskContext::new_task(0x1_0000, 0x2000);
        let ctx2 = ctx;
        assert_eq!(ctx.rip, ctx2.rip);
        assert_eq!(ctx.rsp, ctx2.rsp);
    }

    #[test]
    fn kernel_stack_size_is_16_kib() {
        assert_eq!(KernelStack::SIZE, 16 * 1024);
    }

    #[test]
    fn kernel_stack_top_above_base() {
        let stack = KernelStack::new();
        let base = stack.data.as_ptr() as u64;
        assert!(stack.top() > base);
    }

    #[test]
    fn kernel_stack_top_within_bounds() {
        let stack = KernelStack::new();
        let base = stack.data.as_ptr() as u64;
        let end = base + KernelStack::SIZE as u64;
        assert!(stack.top() <= end);
    }

    #[test]
    fn kernel_stack_top_is_16_byte_aligned() {
        let stack = KernelStack::new();
        assert_eq!(stack.top() % 16, 0, "stack top must be 16-byte aligned");
    }
}

// -----------------------------------------------------------------------
// Scheduler context-switch integration tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod scheduler_context_tests {
    use crate::sched::{Priority, Scheduler, TaskState};

    #[test]
    fn add_boot_task_has_zero_rsp() {
        let mut sched = Scheduler::new();
        let id = sched.add_task("boot", Priority::System).unwrap();
        let snap = sched.snapshot();
        let t = snap.iter().flatten().find(|t| t.id == id).unwrap();
        assert_eq!(t.rsp, 0, "boot task has no real RSP until first switch");
        assert_eq!(t.rip, 0, "boot task RIP is zero until first switch");
    }

    #[test]
    fn add_task_with_entry_has_nonzero_rip() {
        extern "C" fn fake_task() -> ! {
            loop {}
        }
        let mut sched = Scheduler::new();
        let id = sched
            .add_task_with_entry("worker", Priority::Normal, fake_task)
            .unwrap();
        let snap = sched.snapshot();
        let t = snap.iter().flatten().find(|t| t.id == id).unwrap();
        assert_ne!(t.rip, 0, "entry task should have a valid RIP");
        assert_ne!(t.rsp, 0, "entry task should have an allocated RSP");
    }

    #[test]
    fn prepare_switch_returns_none_with_one_task() {
        let mut sched = Scheduler::new();
        sched.add_task("solo", Priority::System);
        assert!(sched.prepare_switch().is_none());
    }

    #[test]
    fn prepare_switch_advances_current_task() {
        let mut sched = Scheduler::new();
        sched.add_task("task_a", Priority::Normal);
        sched.add_task("task_b", Priority::Normal);
        sched.tick();
        let before = sched.current_task_id();
        let pair = sched.prepare_switch();
        assert!(pair.is_some());
        let after = sched.current_task_id();
        assert_ne!(before, after, "current task should have changed");
    }

    #[test]
    fn blocked_task_not_selected_for_switch() {
        let mut sched = Scheduler::new();
        sched.add_task("task_a", Priority::Normal).unwrap();
        let id_b = sched.add_task("task_b", Priority::Normal).unwrap();
        sched.add_task("task_c", Priority::Normal).unwrap();
        sched.tick();
        sched.block_task(id_b);
        for _ in 0..6 {
            let _ = sched.prepare_switch();
            if let Some(cur) = sched.current_task_id() {
                assert_ne!(cur, id_b, "blocked task must not be scheduled");
            }
        }
    }

    #[test]
    fn unblock_task_makes_it_ready() {
        let mut sched = Scheduler::new();
        sched.add_task("task_a", Priority::Normal).unwrap();
        let id_b = sched.add_task("task_b", Priority::Normal).unwrap();
        sched.block_task(id_b);
        assert!(sched.unblock_task(id_b));
        let snap = sched.snapshot();
        let t = snap.iter().flatten().find(|t| t.id == id_b).unwrap();
        assert_eq!(t.state, TaskState::Ready);
    }

    #[test]
    fn snapshot_reflects_all_tasks() {
        let mut sched = Scheduler::new();
        sched.add_task("a", Priority::Low);
        sched.add_task("b", Priority::Normal);
        sched.add_task("c", Priority::High);
        let snap = sched.snapshot();
        assert_eq!(snap.iter().flatten().count(), 3);
    }

    #[test]
    fn remove_task_decreases_count() {
        let mut sched = Scheduler::new();
        let id = sched.add_task("tmp", Priority::Low).unwrap();
        assert_eq!(sched.active_count(), 1);
        sched.remove_task(id);
        assert_eq!(sched.active_count(), 0);
    }
}
// -----------------------------------------------------------------------
// FAT32 stub tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod fat32_tests {
    use crate::fat32::{Fat32BootSector, PartitionEntry};

    #[test]
    fn parse_mbr_valid_partition() {
        let mut data = [0u8; 16];
        data[0] = 0x80; // active
        data[4] = 0x0B; // FAT32 (CHS)
        data[8..12].copy_from_slice(&2048u32.to_le_bytes()); // start LBA
        data[12..16].copy_from_slice(&102400u32.to_le_bytes()); // sectors

        let entry = PartitionEntry::parse(&data).expect("should parse valid partition");
        assert_eq!(entry.status, 0x80);
        assert_eq!(entry.partition_type, 0x0B);
        assert_eq!(entry.start_lba, 2048);
        assert_eq!(entry.sector_count, 102400);
    }

    #[test]
    fn parse_mbr_empty_partition() {
        let data = [0u8; 16];
        assert!(PartitionEntry::parse(&data).is_none(), "zeroed partition should be None");
    }

    #[test]
    fn parse_boot_sector_invalid_signature() {
        let data = [0u8; 512];
        assert!(Fat32BootSector::parse(&data).is_none(), "missing 0x55AA signature");
    }

    #[test]
    fn parse_boot_sector_valid() {
        let mut data = [0u8; 512];
        data[510] = 0x55;
        data[511] = 0xAA;
        
        // Bytes per sector
        data[11..13].copy_from_slice(&512u16.to_le_bytes());
        // Sectors per cluster
        data[13] = 8;
        // Reserved sectors
        data[14..16].copy_from_slice(&32u16.to_le_bytes());
        // FAT count
        data[16] = 2;
        // Total sectors 32
        data[32..36].copy_from_slice(&200000u32.to_le_bytes());
        // Sectors per FAT 32
        data[36..40].copy_from_slice(&1000u32.to_le_bytes());
        
        // Volume label "BRANE_OS   "
        let label = b"BRANE_OS   ";
        data[71..82].copy_from_slice(label);
        
        // FS Type "FAT32   "
        let fstype = b"FAT32   ";
        data[82..90].copy_from_slice(fstype);

        let bs = Fat32BootSector::parse(&data).expect("should parse valid boot sector");
        assert_eq!(bs.bytes_per_sector, 512);
        assert_eq!(bs.sectors_per_cluster, 8);
        assert_eq!(bs.reserved_sectors, 32);
        assert_eq!(bs.fat_count, 2);
        assert_eq!(bs.total_sectors_32, 200000);
        assert_eq!(bs.sectors_per_fat_32, 1000);
        assert_eq!(&bs.volume_label, label);
        assert_eq!(&bs.fs_type_label, fstype);
    }
}
