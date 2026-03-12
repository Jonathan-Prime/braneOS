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
