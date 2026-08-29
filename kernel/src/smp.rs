//! SMP topology and Application Processor (AP) boot state.
//!
//! Discovery builds a deterministic boot plan without touching APIC registers.
//! The target-only startup path then allocates a low-memory trampoline, hands
//! each AP a stack and the current page-table state, and records an explicit
//! `Starting`/`Online`/`Failed` lifecycle so malformed topology cannot leave
//! the BSP without a bounded recovery path.

use crate::madt::MadtInfo;

#[cfg(target_os = "none")]
use crate::memory::frame_allocator::BitmapFrameAllocator;
#[cfg(target_os = "none")]
use x86_64::structures::paging::{OffsetPageTable, Page, PageTableFlags, PhysFrame, Translate};
#[cfg(target_os = "none")]
use x86_64::structures::DescriptorTablePointer;
#[cfg(target_os = "none")]
use x86_64::{PhysAddr, VirtAddr};

pub const MAX_CPUS: usize = 32;
/// Fixed-delivery vector used by the BSP to verify AP interrupt readiness.
pub const AP_INTERRUPT_PROBE_VECTOR: u8 = 0xF0;
#[cfg(target_os = "none")]
const UNREGISTERED_CPU: u16 = u16::MAX;
#[cfg(target_os = "none")]
const LEGACY_LIMIT: u64 = 0x10_0000;
const PAGE_SIZE: u64 = 4096;
#[cfg(target_os = "none")]
const AP_START_TIMEOUT_SPINS: usize = 5_000_000;
#[cfg(target_os = "none")]
const AP_ACK_ONLINE: u32 = 1;
#[cfg(target_os = "none")]
const AP_ACK_FAILED: u32 = 2;
#[cfg(target_os = "none")]
const AP_INTERRUPT_TIMEOUT_SPINS: usize = 2_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CpuLifecycle {
    Disabled = 0,
    Discovered = 1,
    Starting = 2,
    Online = 3,
    Failed = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuDescriptor {
    pub processor_uid: u32,
    pub apic_id: u32,
    pub enabled: bool,
    pub lifecycle: CpuLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuBootPlan {
    pub cpus: [Option<CpuDescriptor>; MAX_CPUS],
    pub cpu_count: usize,
    pub enabled_cpu_count: usize,
    pub bsp_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuPlanError {
    NoEnabledCpu,
    DuplicateApicId(u32),
    BspAlreadyAssigned,
    BspNotFound(u32),
    InvalidState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApStartupError {
    TrampolineAddressInvalid,
    TrampolineTooLarge,
    TrampolineMappingConflict,
    TrampolineContextOverflow,
    PageTableAbove4G,
    BspNotAssigned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApTrampoline {
    pub physical_address: u64,
    context_offset: u64,
}

impl ApTrampoline {
    pub const fn vector(self) -> u8 {
        (self.physical_address / PAGE_SIZE) as u8
    }

    #[cfg(target_os = "none")]
    fn context_virtual(self, physical_memory_offset: u64) -> Option<u64> {
        physical_memory_offset
            .checked_add(self.physical_address)?
            .checked_add(self.context_offset)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApStartupReport {
    pub attempted: usize,
    pub online: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApInterruptReport {
    pub attempted: usize,
    pub responsive: usize,
    pub failed: usize,
}

/// Aggregate result of repeated APIC scheduler kicks after SMP bring-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApDispatchStressReport {
    pub rounds: usize,
    pub attempted: usize,
    pub responsive: usize,
    pub failed: usize,
}

impl CpuBootPlan {
    /// Build a deterministic plan from MADT entries without touching APIC
    /// registers or allocating per-CPU memory.
    pub fn from_madt(info: &MadtInfo) -> Result<Self, CpuPlanError> {
        let mut plan = Self {
            cpus: [None; MAX_CPUS],
            cpu_count: 0,
            enabled_cpu_count: 0,
            bsp_index: None,
        };
        for entry in info.cpus.iter().take(info.cpu_count).flatten() {
            if plan.cpu_count == MAX_CPUS {
                break;
            }
            if plan
                .cpus
                .iter()
                .take(plan.cpu_count)
                .flatten()
                .any(|cpu| cpu.apic_id == entry.apic_id)
            {
                return Err(CpuPlanError::DuplicateApicId(entry.apic_id));
            }
            let lifecycle = if entry.enabled {
                plan.enabled_cpu_count += 1;
                CpuLifecycle::Discovered
            } else {
                CpuLifecycle::Disabled
            };
            plan.cpus[plan.cpu_count] = Some(CpuDescriptor {
                processor_uid: entry.processor_uid,
                apic_id: entry.apic_id,
                enabled: entry.enabled,
                lifecycle,
            });
            plan.cpu_count += 1;
        }
        if plan.enabled_cpu_count == 0 {
            return Err(CpuPlanError::NoEnabledCpu);
        }
        Ok(plan)
    }

    /// Mark the processor whose Local APIC ID was read during bring-up as the
    /// bootstrap processor. Exactly one BSP is allowed in a boot plan.
    pub fn assign_bsp(&mut self, apic_id: u32) -> Result<usize, CpuPlanError> {
        if self.bsp_index.is_some() {
            return Err(CpuPlanError::BspAlreadyAssigned);
        }
        let Some(index) = self
            .cpus
            .iter()
            .take(self.cpu_count)
            .position(|cpu| cpu.is_some_and(|cpu| cpu.enabled && cpu.apic_id == apic_id))
        else {
            return Err(CpuPlanError::BspNotFound(apic_id));
        };
        self.cpus[index].as_mut().unwrap().lifecycle = CpuLifecycle::Online;
        self.bsp_index = Some(index);
        Ok(index)
    }

    pub fn mark_starting(&mut self, apic_id: u32) -> Result<usize, CpuPlanError> {
        let index = self.find_enabled(apic_id)?;
        let cpu = self.cpus[index].as_mut().unwrap();
        if cpu.lifecycle != CpuLifecycle::Discovered {
            return Err(CpuPlanError::InvalidState);
        }
        cpu.lifecycle = CpuLifecycle::Starting;
        Ok(index)
    }

    pub fn mark_online(&mut self, apic_id: u32) -> Result<usize, CpuPlanError> {
        let index = self.find_enabled(apic_id)?;
        let cpu = self.cpus[index].as_mut().unwrap();
        if cpu.lifecycle != CpuLifecycle::Starting {
            return Err(CpuPlanError::InvalidState);
        }
        cpu.lifecycle = CpuLifecycle::Online;
        Ok(index)
    }

    pub fn mark_failed(&mut self, apic_id: u32) -> Result<usize, CpuPlanError> {
        let index = self.find_enabled(apic_id)?;
        let cpu = self.cpus[index].as_mut().unwrap();
        if cpu.lifecycle != CpuLifecycle::Starting {
            return Err(CpuPlanError::InvalidState);
        }
        cpu.lifecycle = CpuLifecycle::Failed;
        Ok(index)
    }

    /// Mark an AP that was online but stopped responding to its health probe.
    pub fn mark_unresponsive(&mut self, apic_id: u32) -> Result<usize, CpuPlanError> {
        let index = self.find_enabled(apic_id)?;
        let cpu = self.cpus[index].as_mut().unwrap();
        if cpu.lifecycle != CpuLifecycle::Online {
            return Err(CpuPlanError::InvalidState);
        }
        cpu.lifecycle = CpuLifecycle::Failed;
        Ok(index)
    }

    pub fn online_cpu_count(&self) -> usize {
        self.cpus
            .iter()
            .take(self.cpu_count)
            .flatten()
            .filter(|cpu| cpu.lifecycle == CpuLifecycle::Online)
            .count()
    }

    fn find_enabled(&self, apic_id: u32) -> Result<usize, CpuPlanError> {
        self.cpus
            .iter()
            .take(self.cpu_count)
            .position(|cpu| cpu.is_some_and(|cpu| cpu.enabled && cpu.apic_id == apic_id))
            .ok_or(CpuPlanError::BspNotFound(apic_id))
    }
}

#[cfg(target_os = "none")]
#[repr(C)]
#[derive(Clone, Copy)]
struct ApContext {
    cr0: u64,
    cr3: u64,
    cr4: u64,
    efer: u64,
    stack: u64,
    entry: u64,
    apic_id: u32,
    cpu_slot: u32,
    ready: u64,
    idt: DescriptorTablePointer,
}

#[cfg(target_os = "none")]
#[repr(align(16))]
#[derive(Clone, Copy)]
#[allow(dead_code)]
struct ApStack([u8; 16 * 1024]);

#[cfg(target_os = "none")]
static mut AP_STACKS: [ApStack; MAX_CPUS] = [ApStack([0; 16 * 1024]); MAX_CPUS];

#[cfg(target_os = "none")]
static AP_START_ACK: [core::sync::atomic::AtomicU32; MAX_CPUS] =
    [const { core::sync::atomic::AtomicU32::new(0) }; MAX_CPUS];

#[cfg(target_os = "none")]
static AP_INTERRUPT_ACK: [core::sync::atomic::AtomicU32; 256] =
    [const { core::sync::atomic::AtomicU32::new(0) }; 256];

#[cfg(target_os = "none")]
static AP_SCHEDULER_KICK: [core::sync::atomic::AtomicU32; MAX_CPUS] =
    [const { core::sync::atomic::AtomicU32::new(0) }; MAX_CPUS];

#[cfg(target_os = "none")]
static CPU_INDEX_BY_APIC_ID: [core::sync::atomic::AtomicU16; 256] =
    [const { core::sync::atomic::AtomicU16::new(UNREGISTERED_CPU) }; 256];

/// Associate an xAPIC ID with the stable per-CPU runtime slot.
pub fn register_cpu_index(apic_id: u32, cpu_slot: usize) {
    #[cfg(target_os = "none")]
    if apic_id <= u8::MAX as u32 && cpu_slot < MAX_CPUS {
        CPU_INDEX_BY_APIC_ID[apic_id as usize]
            .store(cpu_slot as u16, core::sync::atomic::Ordering::Release);
    }
    #[cfg(not(target_os = "none"))]
    let _ = (apic_id, cpu_slot);
}

/// Return the runtime slot for the processor executing this code.
pub fn current_cpu_index() -> usize {
    #[cfg(target_os = "none")]
    {
        if let Some(apic_id) = crate::apic::current_local_apic_id() {
            let slot =
                CPU_INDEX_BY_APIC_ID[apic_id as usize].load(core::sync::atomic::Ordering::Acquire);
            if slot != UNREGISTERED_CPU {
                return slot as usize;
            }
        }
    }
    0
}

/// Consume one scheduler wake-up request on the current AP.
#[cfg(target_os = "none")]
fn take_scheduler_kick(cpu_slot: usize) -> bool {
    AP_SCHEDULER_KICK
        .get(cpu_slot)
        .is_some_and(|kick| kick.swap(0, core::sync::atomic::Ordering::AcqRel) != 0)
}

#[cfg(target_os = "none")]
unsafe extern "C" {
    static smp_ap_trampoline_start: u8;
    static smp_ap_trampoline_end: u8;
    static smp_ap_long_mode_target: u8;
    static smp_ap_far_target: u8;
    static smp_ap_gdt: u8;
    static smp_ap_gdt_base: u8;
    static smp_ap_context: u8;
}

#[cfg(target_os = "none")]
pub fn prepare_ap_trampoline(
    mapper: &mut OffsetPageTable<'static>,
    frame_allocator: &mut BitmapFrameAllocator,
    physical_memory_offset: u64,
) -> Result<ApTrampoline, ApStartupError> {
    let trampoline_phys = frame_allocator
        .allocate_below(LEGACY_LIMIT)
        .ok_or(ApStartupError::TrampolineAddressInvalid)?;
    if !(PAGE_SIZE..LEGACY_LIMIT).contains(&trampoline_phys)
        || trampoline_phys > LEGACY_LIMIT - PAGE_SIZE
        || trampoline_phys & (PAGE_SIZE - 1) != 0
    {
        return Err(ApStartupError::TrampolineAddressInvalid);
    }
    let page = Page::containing_address(VirtAddr::new(trampoline_phys));
    let frame = PhysFrame::containing_address(PhysAddr::new(trampoline_phys));
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    match mapper.translate_addr(page.start_address()) {
        Some(mapped) if mapped == frame.start_address() => {}
        Some(_) => return Err(ApStartupError::TrampolineMappingConflict),
        None => crate::memory::paging::map_page(mapper, page, frame, flags, frame_allocator)
            .map_err(|_| ApStartupError::TrampolineMappingConflict)?,
    }

    unsafe {
        let source = core::ptr::addr_of!(smp_ap_trampoline_start);
        let end = core::ptr::addr_of!(smp_ap_trampoline_end);
        let length = end as usize - source as usize;
        if length > PAGE_SIZE as usize {
            return Err(ApStartupError::TrampolineTooLarge);
        }
        let destination_address = physical_memory_offset
            .checked_add(trampoline_phys)
            .ok_or(ApStartupError::TrampolineContextOverflow)?;
        let destination = destination_address as *mut u8;
        core::ptr::write_bytes(destination, 0, PAGE_SIZE as usize);
        core::ptr::copy_nonoverlapping(source, destination, length);

        let context_offset = symbol_offset(core::ptr::addr_of!(smp_ap_context));
        let far_target_offset = symbol_offset(core::ptr::addr_of!(smp_ap_far_target));
        let gdt_base_offset = symbol_offset(core::ptr::addr_of!(smp_ap_gdt_base));
        let gdt_offset = symbol_offset(core::ptr::addr_of!(smp_ap_gdt));
        if physical_memory_offset
            .checked_add(trampoline_phys)
            .and_then(|base| base.checked_add(context_offset as u64))
            .is_none()
        {
            return Err(ApStartupError::TrampolineContextOverflow);
        }
        core::ptr::write_unaligned(
            destination.add(far_target_offset) as *mut u32,
            (trampoline_phys + symbol_offset(core::ptr::addr_of!(smp_ap_long_mode_target)) as u64)
                as u32,
        );
        core::ptr::write_unaligned(
            destination.add(gdt_base_offset) as *mut u32,
            (trampoline_phys + gdt_offset as u64) as u32,
        );

        let cr0: u64;
        let cr3: u64;
        let cr4: u64;
        core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags));
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        if cr0 > u32::MAX as u64 || cr3 > u32::MAX as u64 || cr4 > u32::MAX as u64 {
            return Err(ApStartupError::PageTableAbove4G);
        }
        let efer = x86_64::registers::model_specific::Msr::new(0xC000_0080).read();
        let context = ApContext {
            cr0,
            cr3,
            cr4,
            efer,
            stack: 0,
            entry: ap_entry as *const () as u64,
            apic_id: 0,
            cpu_slot: 0,
            ready: 0,
            idt: x86_64::instructions::tables::sidt(),
        };
        core::ptr::write_unaligned(destination.add(context_offset) as *mut ApContext, context);
        Ok(ApTrampoline {
            physical_address: trampoline_phys,
            context_offset: context_offset as u64,
        })
    }
}

#[cfg(target_os = "none")]
pub fn start_application_processors(
    trampoline: ApTrampoline,
    physical_memory_offset: u64,
    local_apic: crate::apic::LocalApic,
    plan: &mut CpuBootPlan,
) -> Result<ApStartupReport, ApStartupError> {
    if plan.bsp_index.is_none() {
        return Err(ApStartupError::BspNotAssigned);
    }
    let Some(context_virtual) = trampoline.context_virtual(physical_memory_offset) else {
        return Err(ApStartupError::TrampolineContextOverflow);
    };
    let vector = trampoline.vector();
    if vector == 0 {
        return Err(ApStartupError::TrampolineAddressInvalid);
    }

    let mut report = ApStartupReport::default();
    let mut cpu_slot = 1usize;
    for slot in 0..plan.cpu_count {
        let Some(cpu) = plan.cpus[slot] else { continue };
        if !cpu.enabled || Some(slot) == plan.bsp_index {
            continue;
        }
        report.attempted += 1;
        let ap_cpu_slot = cpu_slot;
        cpu_slot += 1;
        if cpu.apic_id > u8::MAX as u32 {
            plan.cpus[slot].as_mut().unwrap().lifecycle = CpuLifecycle::Failed;
            report.failed += 1;
            continue;
        }
        if plan.mark_starting(cpu.apic_id).is_err() {
            report.failed += 1;
            continue;
        }
        let ack = &AP_START_ACK[slot];
        ack.store(0, core::sync::atomic::Ordering::Release);
        let mut context = unsafe { core::ptr::read_unaligned(context_virtual as *const ApContext) };
        context.stack = unsafe {
            core::ptr::addr_of!(AP_STACKS[slot]) as u64 + core::mem::size_of::<ApStack>() as u64
        };
        context.entry = ap_entry as *const () as u64;
        context.apic_id = cpu.apic_id;
        context.cpu_slot = ap_cpu_slot as u32;
        context.ready = ack as *const _ as u64;
        unsafe {
            core::ptr::write_unaligned(context_virtual as *mut ApContext, context);
            if !local_apic.send_init_sipi(cpu.apic_id as u8, vector) {
                let _ = plan.mark_failed(cpu.apic_id);
                report.failed += 1;
                continue;
            }
        }
        let mut started = false;
        for _ in 0..AP_START_TIMEOUT_SPINS {
            if ack.load(core::sync::atomic::Ordering::Acquire) != 0 {
                started = true;
                break;
            }
            core::hint::spin_loop();
        }
        if started
            && ack.load(core::sync::atomic::Ordering::Acquire) == AP_ACK_ONLINE
            && plan.mark_online(cpu.apic_id).is_ok()
        {
            report.online += 1;
        } else {
            let _ = plan.mark_failed(cpu.apic_id);
            report.failed += 1;
        }
    }
    Ok(report)
}

/// Send one fixed IPI to every online AP and wait for its shared IDT handler
/// to acknowledge it. A missing response demotes that AP to `Failed` without
/// affecting the BSP or the other processors.
#[cfg(target_os = "none")]
pub fn verify_application_processors(
    local_apic: crate::apic::LocalApic,
    plan: &mut CpuBootPlan,
) -> ApInterruptReport {
    let mut report = ApInterruptReport::default();
    for slot in 0..plan.cpu_count {
        let Some(cpu) = plan.cpus[slot] else { continue };
        if !cpu.enabled || Some(slot) == plan.bsp_index || cpu.lifecycle != CpuLifecycle::Online {
            continue;
        }
        if cpu.apic_id > u8::MAX as u32 {
            continue;
        }
        report.attempted += 1;
        let ack = &AP_INTERRUPT_ACK[cpu.apic_id as usize];
        ack.store(0, core::sync::atomic::Ordering::Release);
        let delivered =
            unsafe { local_apic.send_fixed_ipi(cpu.apic_id as u8, AP_INTERRUPT_PROBE_VECTOR) };
        let mut responsive = false;
        if delivered {
            for _ in 0..AP_INTERRUPT_TIMEOUT_SPINS {
                if ack.load(core::sync::atomic::Ordering::Acquire) != 0 {
                    responsive = true;
                    break;
                }
                core::hint::spin_loop();
            }
        }
        if responsive {
            report.responsive += 1;
        } else {
            let _ = plan.mark_unresponsive(cpu.apic_id);
            report.failed += 1;
        }
    }
    report
}

/// Exercise the AP interrupt/dispatcher path for several rounds. A failed
/// round stops the test so an unresponsive AP is not hammered indefinitely;
/// `verify_application_processors` records the AP as `Failed` in that case.
#[cfg(target_os = "none")]
pub fn stress_application_processors(
    local_apic: crate::apic::LocalApic,
    plan: &mut CpuBootPlan,
    rounds: usize,
) -> ApDispatchStressReport {
    let mut report = ApDispatchStressReport::default();
    for _ in 0..rounds.max(1) {
        let round = verify_application_processors(local_apic, plan);
        report.rounds += 1;
        report.attempted += round.attempted;
        report.responsive += round.responsive;
        report.failed += round.failed;
        if round.failed != 0 {
            break;
        }
    }
    report
}

/// Record an interrupt probe from the AP currently executing the handler.
#[cfg(target_os = "none")]
pub fn acknowledge_interrupt_probe() {
    // The same fixed IPI used by the bring-up health check also serves as a
    // scheduler kick once the run queues have been configured. The interrupt
    // itself only records the wake-up; the AP's normal loop performs the
    // register-context handoff outside the x86-interrupt frame.
    let cpu_slot = current_cpu_index();
    if let Some(kick) = AP_SCHEDULER_KICK.get(cpu_slot) {
        kick.store(1, core::sync::atomic::Ordering::Release);
    }
    if let Some(apic_id) = crate::apic::current_local_apic_id() {
        AP_INTERRUPT_ACK[apic_id as usize].store(1, core::sync::atomic::Ordering::Release);
    }
}

#[cfg(target_os = "none")]
extern "C" fn ap_entry(
    apic_id: u32,
    cpu_slot: u32,
    ready: *const core::sync::atomic::AtomicU32,
    idt: *const DescriptorTablePointer,
) -> ! {
    x86_64::instructions::interrupts::disable();
    let initialized = unsafe {
        crate::gdt::init_ap(cpu_slot as usize).is_ok()
            && crate::usermode::init_syscall_msrs_for_cpu(cpu_slot as usize)
            && crate::sched::init_cpu_scheduler(cpu_slot as usize)
            && crate::apic::enable_current_local_apic()
    };
    if initialized {
        unsafe {
            x86_64::instructions::tables::lidt(&*idt);
            register_cpu_index(apic_id, cpu_slot as usize);
            x86_64::instructions::interrupts::enable();
            (*ready).store(AP_ACK_ONLINE, core::sync::atomic::Ordering::Release);
        }
    } else {
        unsafe {
            (*ready).store(AP_ACK_FAILED, core::sync::atomic::Ordering::Release);
        }
    }
    loop {
        x86_64::instructions::hlt();
        if take_scheduler_kick(cpu_slot as usize) {
            let _ = crate::sched::switch_current_cpu_context();
        }
    }
}

#[cfg(target_os = "none")]
unsafe fn symbol_offset(symbol: *const u8) -> usize {
    symbol as usize - core::ptr::addr_of!(smp_ap_trampoline_start) as usize
}

#[cfg(target_os = "none")]
core::arch::global_asm!(
    r#"
    .pushsection .text.smp_ap, "ax", @progbits
    .balign 16
    .global smp_ap_trampoline_start
    .global smp_ap_trampoline_end
    .global smp_ap_long_mode_target
    .global smp_ap_far_target
    .global smp_ap_gdt
    .global smp_ap_gdt_base
    .global smp_ap_context

    .code16
smp_ap_trampoline_start:
    cli
    cld
    mov ax, cs
    mov ds, ax
    lgdt cs:[SMP_AP_GDT_DESCRIPTOR_OFFSET]
    mov eax, dword ptr cs:[SMP_AP_CONTEXT_OFFSET + 16]
    mov cr4, eax
    mov eax, dword ptr cs:[SMP_AP_CONTEXT_OFFSET + 8]
    mov cr3, eax
    mov ecx, 0xc0000080
    mov eax, dword ptr cs:[SMP_AP_CONTEXT_OFFSET + 24]
    mov edx, dword ptr cs:[SMP_AP_CONTEXT_OFFSET + 28]
    wrmsr
    mov eax, dword ptr cs:[SMP_AP_CONTEXT_OFFSET]
    mov cr0, eax
    .byte 0x66, 0xea
smp_ap_far_target:
    .long 0
    .word 0x08

    .code64
smp_ap_long_mode_target:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov rsp, qword ptr [rip + smp_ap_context + 32]
    mov edi, dword ptr [rip + smp_ap_context + 48]
    mov esi, dword ptr [rip + smp_ap_context + 52]
    mov rdx, qword ptr [rip + smp_ap_context + 56]
    lea rcx, [rip + smp_ap_context + 64]
    mov rax, qword ptr [rip + smp_ap_context + 40]
    jmp rax

    .balign 8
smp_ap_gdt:
    .quad 0x0000000000000000
    .quad 0x00af9a000000ffff
    .quad 0x00cf92000000ffff
smp_ap_gdt_end:
smp_ap_gdt_descriptor:
    .word smp_ap_gdt_end - smp_ap_gdt - 1
smp_ap_gdt_base:
    .long 0

    .balign 8
smp_ap_context:
    .zero 64
smp_ap_trampoline_end:
    .set SMP_AP_GDT_DESCRIPTOR_OFFSET, smp_ap_gdt_descriptor - smp_ap_trampoline_start
    .set SMP_AP_CONTEXT_OFFSET, smp_ap_context - smp_ap_trampoline_start
    .code64
    .popsection
"#
);

#[cfg(test)]
mod tests {
    use super::*;

    fn madt(cpus: &[(u32, u32, bool)]) -> MadtInfo {
        let mut info = MadtInfo {
            local_apic_address: 0xFEE0_0000,
            cpus: [None; MAX_CPUS],
            cpu_count: 0,
            io_apics: [None; 8],
            io_apic_count: 0,
            interrupt_overrides: [None; 16],
            interrupt_override_count: 0,
        };
        for &(uid, apic_id, enabled) in cpus {
            info.cpus[info.cpu_count] = Some(crate::madt::CpuEntry {
                processor_uid: uid,
                apic_id,
                enabled,
            });
            info.cpu_count += 1;
        }
        info
    }

    #[test]
    fn builds_plan_and_assigns_bsp() {
        let mut plan =
            CpuBootPlan::from_madt(&madt(&[(0, 2, true), (1, 7, true), (2, 9, false)])).unwrap();
        assert_eq!(plan.enabled_cpu_count, 2);
        assert_eq!(plan.assign_bsp(7), Ok(1));
        assert_eq!(plan.online_cpu_count(), 1);
        assert_eq!(plan.cpus[2].unwrap().lifecycle, CpuLifecycle::Disabled);
    }

    #[test]
    fn rejects_duplicate_apic_ids() {
        assert_eq!(
            CpuBootPlan::from_madt(&madt(&[(0, 2, true), (1, 2, true)])),
            Err(CpuPlanError::DuplicateApicId(2))
        );
    }

    #[test]
    fn rejects_empty_enabled_set_and_unknown_bsp() {
        assert_eq!(
            CpuBootPlan::from_madt(&madt(&[(0, 2, false)])),
            Err(CpuPlanError::NoEnabledCpu)
        );
        let mut plan = CpuBootPlan::from_madt(&madt(&[(0, 2, true)])).unwrap();
        assert_eq!(plan.assign_bsp(3), Err(CpuPlanError::BspNotFound(3)));
    }

    #[test]
    fn enforces_ap_lifecycle_transitions() {
        let mut plan = CpuBootPlan::from_madt(&madt(&[(0, 2, true), (1, 7, true)])).unwrap();
        assert_eq!(plan.mark_online(7), Err(CpuPlanError::InvalidState));
        assert_eq!(plan.mark_starting(7), Ok(1));
        assert_eq!(plan.mark_online(7), Ok(1));
        assert_eq!(plan.mark_failed(7), Err(CpuPlanError::InvalidState));
        assert_eq!(plan.assign_bsp(2), Ok(0));
        assert_eq!(plan.assign_bsp(7), Err(CpuPlanError::BspAlreadyAssigned));
    }

    #[test]
    fn demotes_unresponsive_online_ap() {
        let mut plan = CpuBootPlan::from_madt(&madt(&[(0, 2, true), (1, 7, true)])).unwrap();
        assert_eq!(plan.assign_bsp(2), Ok(0));
        assert_eq!(plan.mark_starting(7), Ok(1));
        assert_eq!(plan.mark_online(7), Ok(1));
        assert_eq!(plan.mark_unresponsive(7), Ok(1));
        assert_eq!(plan.cpus[1].unwrap().lifecycle, CpuLifecycle::Failed);
        assert_eq!(plan.mark_unresponsive(7), Err(CpuPlanError::InvalidState));
    }

    #[test]
    fn encodes_sipi_vector_from_low_page() {
        let trampoline = ApTrampoline {
            physical_address: 0x9_000,
            context_offset: 0,
        };
        assert_eq!(trampoline.vector(), 9);
    }
}
