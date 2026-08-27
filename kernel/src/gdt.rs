// ============================================================
// Brane OS Kernel — Global Descriptor Table (GDT)
// ============================================================
//
// Sets up the GDT with kernel code/data segments and a TSS
// (Task State Segment) containing an IST (Interrupt Stack Table)
// entry for the double fault handler.
//
// Why: x86_64 requires a GDT for segment descriptors. The TSS
// provides a clean stack for double faults, preventing triple
// faults when the kernel stack overflows.
// ============================================================

use spin::{Lazy, Mutex};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

/// IST index used for the double fault handler stack.
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

/// Size of the IST stack for double faults (20 KiB).
const IST_STACK_SIZE: usize = 4096 * 5;

/// Static stack for the double fault IST entry.
#[repr(align(16))]
#[allow(dead_code)]
struct IstStack([u8; IST_STACK_SIZE]);

static mut IST_STACK: IstStack = IstStack([0; IST_STACK_SIZE]);

/// Task State Segment — provides the IST stack for double faults.
static TSS: Lazy<TaskStateSegment> = Lazy::new(|| {
    let mut tss = TaskStateSegment::new();
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
        let stack_start = VirtAddr::from_ptr(&raw const IST_STACK);
        stack_start + IST_STACK_SIZE as u64 // stack grows downward
    };
    tss
});

/// Holds the GDT and the selectors needed to load it.
struct Gdt {
    table: GlobalDescriptorTable,
    kernel_code_selector: SegmentSelector,
    kernel_data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
    user_data_selector: SegmentSelector,
    user_code_selector: SegmentSelector,
}

impl Gdt {
    fn new() -> Self {
        let mut table = GlobalDescriptorTable::new();
        let kernel_code_selector = table.append(Descriptor::kernel_code_segment());
        let kernel_data_selector = table.append(Descriptor::kernel_data_segment());
        let tss_selector = table.append(Descriptor::tss_segment(&TSS));
        // Ring-3 segments: data MUST come before code so STAR[63:48] points
        // to the data selector and STAR[63:48]+8 lands on the code selector.
        let user_data_selector = table.append(Descriptor::user_data_segment());
        let user_code_selector = table.append(Descriptor::user_code_segment());
        Self {
            table,
            kernel_code_selector,
            kernel_data_selector,
            tss_selector,
            user_data_selector,
            user_code_selector,
        }
    }
}

// Loading TR marks the TSS descriptor busy in memory. Reconstructing the GDT
// before each load restores an available TSS descriptor, which makes `init`
// safe to call again after ACPI S3 firmware has replaced the descriptor tables.
static GDT: Lazy<Mutex<Gdt>> = Lazy::new(|| Mutex::new(Gdt::new()));

/// Initialize the GDT and load it into the CPU.
///
/// Must be called once during early kernel init, before the IDT.
pub fn init() {
    use x86_64::instructions::segmentation::{Segment, CS, DS, ES, SS};
    use x86_64::instructions::tables::load_tss;

    let mut gdt = GDT.lock();
    *gdt = Gdt::new();
    unsafe {
        gdt.table.load_unsafe();
        CS::set_reg(gdt.kernel_code_selector);
        DS::set_reg(gdt.kernel_data_selector);
        ES::set_reg(gdt.kernel_data_selector);
        SS::set_reg(gdt.kernel_data_selector);
        load_tss(gdt.tss_selector);
    }
}

/// Raw selector value for ring-0 code segment (used in STAR MSR).
pub fn kernel_code_selector() -> u16 {
    GDT.lock().kernel_code_selector.0
}

/// Raw selector value for ring-3 data segment (used in STAR MSR).
pub fn user_data_selector() -> u16 {
    GDT.lock().user_data_selector.0
}

/// Raw selector value for ring-3 code segment (used in iretq trampoline).
pub fn user_code_selector() -> u16 {
    GDT.lock().user_code_selector.0
}
