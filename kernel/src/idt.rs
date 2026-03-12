// ============================================================
// Brane OS Kernel — Interrupt Descriptor Table (IDT)
// ============================================================
//
// Configures the IDT with handlers for:
//   - CPU exceptions (breakpoint, double fault, page fault, etc.)
//   - Hardware interrupts via the 8259 PIC (timer, keyboard)
//
// Spec reference: ARCHITECTURE.md §5.2.2 (Interrupt Manager)
// ============================================================

use spin::Lazy;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::gdt;
use crate::pic;
use crate::{serial_println, halt_loop};

// -----------------------------------------------------------------------
// IDT Setup
// -----------------------------------------------------------------------

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();

    // --- CPU Exceptions ---
    idt.breakpoint.set_handler_fn(breakpoint_handler);

    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }

    idt.page_fault.set_handler_fn(page_fault_handler);
    idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
    idt.segment_not_present.set_handler_fn(segment_not_present_handler);
    idt.stack_segment_fault.set_handler_fn(stack_segment_fault_handler);

    // --- Hardware Interrupts (PIC) ---
    idt[pic::InterruptIndex::Timer.as_u8()]
        .set_handler_fn(timer_interrupt_handler);
    idt[pic::InterruptIndex::Keyboard.as_u8()]
        .set_handler_fn(keyboard_interrupt_handler);

    idt
});

/// Load the IDT into the CPU.
///
/// Must be called after `gdt::init()`.
pub fn init() {
    IDT.load();
    serial_println!("[idt]  Interrupt Descriptor Table loaded.");
}

// -----------------------------------------------------------------------
// CPU Exception Handlers
// -----------------------------------------------------------------------

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    serial_println!("[EXCEPTION] Breakpoint");
    serial_println!("  {:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    serial_println!("[EXCEPTION] DOUBLE FAULT");
    serial_println!("  {:#?}", stack_frame);
    halt_loop();
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    serial_println!("[EXCEPTION] PAGE FAULT");
    serial_println!("  Accessed Address: {:?}", Cr2::read());
    serial_println!("  Error Code: {:?}", error_code);
    serial_println!("  {:#?}", stack_frame);
    halt_loop();
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    serial_println!("[EXCEPTION] GENERAL PROTECTION FAULT");
    serial_println!("  Error Code: {}", error_code);
    serial_println!("  {:#?}", stack_frame);
    halt_loop();
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    serial_println!("[EXCEPTION] INVALID OPCODE");
    serial_println!("  {:#?}", stack_frame);
    halt_loop();
}

extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    serial_println!("[EXCEPTION] SEGMENT NOT PRESENT");
    serial_println!("  Error Code: {}", error_code);
    serial_println!("  {:#?}", stack_frame);
    halt_loop();
}

extern "x86-interrupt" fn stack_segment_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    serial_println!("[EXCEPTION] STACK SEGMENT FAULT");
    serial_println!("  Error Code: {}", error_code);
    serial_println!("  {:#?}", stack_frame);
    halt_loop();
}

// -----------------------------------------------------------------------
// Hardware Interrupt Handlers
// -----------------------------------------------------------------------

/// Timer interrupt — fires on every PIT tick (~18.2 Hz by default).
/// This will drive the scheduler in future phases.
extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // Future: tick the scheduler here
    // sched::tick();

    unsafe {
        pic::PICS.lock().notify_end_of_interrupt(pic::InterruptIndex::Timer.as_u8());
    }
}

/// Keyboard interrupt — decodes PS/2 scancodes and prints to serial.
extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60); // PS/2 data port
    let scancode: u8 = unsafe { port.read() };

    crate::keyboard::handle_scancode(scancode);

    unsafe {
        pic::PICS.lock().notify_end_of_interrupt(pic::InterruptIndex::Keyboard.as_u8());
    }
}
