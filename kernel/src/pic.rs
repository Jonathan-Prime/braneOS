// ============================================================
// Brane OS Kernel — PIC 8259 (Programmable Interrupt Controller)
// ============================================================
//
// Initializes the dual 8259 PIC in cascade mode, remapping
// hardware IRQs to vectors 32–47 to avoid collision with
// CPU exception vectors (0–31).
//
// Spec reference: ARCHITECTURE.md §5.2.2 (Interrupt Manager)
// ============================================================

use pic8259::ChainedPics;
use spin::Mutex;

/// Offset for the primary PIC (IRQ 0–7 → vectors 32–39).
pub const PIC_1_OFFSET: u8 = 32;

/// Offset for the secondary PIC (IRQ 8–15 → vectors 40–47).
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

/// Global PIC instance, protected by a spinlock.
pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// Hardware interrupt indices (after remapping).
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer    = PIC_1_OFFSET,      // IRQ 0 → vector 32
    Keyboard = PIC_1_OFFSET + 1,  // IRQ 1 → vector 33
}

impl InterruptIndex {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Initialize the PIC and unmask interrupts.
///
/// Must be called after `idt::init()`.
pub fn init() {
    unsafe {
        PICS.lock().initialize();
    }
    x86_64::instructions::interrupts::enable();
    crate::serial_println!("[pic]  8259 PIC initialized. Hardware interrupts enabled.");
}
