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

use spin::Lazy;
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
    tss_selector: SegmentSelector,
}

static GDT: Lazy<Gdt> = Lazy::new(|| {
    let mut gdt = GlobalDescriptorTable::new();
    let kernel_code_selector = gdt.append(Descriptor::kernel_code_segment());
    let tss_selector = gdt.append(Descriptor::tss_segment(&TSS));
    Gdt {
        table: gdt,
        kernel_code_selector,
        tss_selector,
    }
});

/// Initialize the GDT and load it into the CPU.
///
/// Must be called once during early kernel init, before the IDT.
pub fn init() {
    use x86_64::instructions::segmentation::{Segment, CS};
    use x86_64::instructions::tables::load_tss;

    GDT.table.load();
    unsafe {
        CS::set_reg(GDT.kernel_code_selector);
        load_tss(GDT.tss_selector);
    }
}
