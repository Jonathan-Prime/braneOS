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
use x86_64::instructions::port::Port;

const PS2_DATA: u16 = 0x60;
const PS2_STATUS_COMMAND: u16 = 0x64;
const STATUS_OUTPUT_FULL: u8 = 1;
const STATUS_INPUT_FULL: u8 = 1 << 1;
const IO_TIMEOUT: usize = 100_000;

/// Global keyboard state, protected by a spinlock.
static KEYBOARD: spin::Lazy<Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>>> =
    spin::Lazy::new(|| {
        Mutex::new(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            HandleControl::Ignore,
        ))
    });

/// Initialize the first PS/2 controller port and enable keyboard scanning.
///
/// Firmware normally leaves the keyboard operational on cold boot, but ACPI
/// S3 may reset the i8042 controller. This routine is intentionally bounded so
/// unsupported hardware cannot stall kernel initialization forever.
pub fn init() -> bool {
    let mut command = Port::<u8>::new(PS2_STATUS_COMMAND);
    let mut data = Port::<u8>::new(PS2_DATA);

    unsafe {
        for _ in 0..32 {
            if command.read() & STATUS_OUTPUT_FULL == 0 {
                break;
            }
            data.read();
        }

        if !wait_input_clear(&mut command) {
            return false;
        }
        command.write(0x20); // read controller configuration byte
        if !wait_output_full(&mut command) {
            return false;
        }
        let config = data.read();

        if !wait_input_clear(&mut command) {
            return false;
        }
        command.write(0x60); // write controller configuration byte
        if !wait_input_clear(&mut command) {
            return false;
        }
        // Enable IRQ1, first-port clock and set-2 → set-1 translation. The
        // decoder consumes set-1 scancodes, while a keyboard reset by S3
        // starts emitting set 2 again.
        data.write((config | 0x41) & !(1 << 4));

        if !wait_input_clear(&mut command) {
            return false;
        }
        command.write(0xae); // enable first PS/2 port
        if !wait_input_clear(&mut command) {
            return false;
        }
        data.write(0xf4); // enable keyboard scanning
        if !wait_output_full(&mut command) {
            return false;
        }
        data.read() == 0xfa // keyboard ACK
    }
}

fn wait_input_clear(status: &mut Port<u8>) -> bool {
    for _ in 0..IO_TIMEOUT {
        if unsafe { status.read() } & STATUS_INPUT_FULL == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn wait_output_full(status: &mut Port<u8>) -> bool {
    for _ in 0..IO_TIMEOUT {
        if unsafe { status.read() } & STATUS_OUTPUT_FULL != 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

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
