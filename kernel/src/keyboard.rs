// ============================================================
// Brane OS Kernel — PS/2 Keyboard Driver
// ============================================================
//
// Decodes scancodes from the PS/2 keyboard controller and
// prints characters to serial output.
//
// This is a minimal driver for early-stage input. It will be
// replaced by a proper driver in the drivers/ directory later.
//
// Spec reference: ARCHITECTURE.md §7 (Capa 3 — Drivers)
// ============================================================

use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use spin::Mutex;

/// Global keyboard state, protected by a spinlock.
static KEYBOARD: spin::Lazy<Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>>> =
    spin::Lazy::new(|| {
        Mutex::new(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            HandleControl::Ignore,
        ))
    });

/// Process a raw scancode from the PS/2 data port.
///
/// Called from the keyboard interrupt handler in `idt.rs`.
pub fn handle_scancode(scancode: u8) {
    let mut keyboard = KEYBOARD.lock();

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => {
                    brane_os_kernel::tty::TTY.lock().on_char(character);
                }
                DecodedKey::RawKey(_key) => {
                    // Ignore raw keys for now (arrows, function keys, etc.)
                }
            }
        }
    }
}
