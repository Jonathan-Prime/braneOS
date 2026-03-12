// ============================================================
// Brane OS Kernel — Serial Output (UART 16550)
// ============================================================
//
// Provides early serial logging via COM1 (port 0x3F8).
// Used before any framebuffer or console is available.
// ============================================================

use spin::Mutex;
use uart_16550::SerialPort;

/// Global serial port, protected by a spinlock.
pub static SERIAL1: spin::Lazy<Mutex<SerialPort>> = spin::Lazy::new(|| {
    let mut serial_port = unsafe { SerialPort::new(0x3F8) };
    serial_port.init();
    Mutex::new(serial_port)
});

/// Initialize the serial port.
///
/// Forces lazy initialization of the global serial port.
pub fn init() {
    // Access the lazy static to force initialization
    let _ = &*SERIAL1;
}

// ---------------------------------------------------------------------------
// Macros for serial printing
// ---------------------------------------------------------------------------

/// Print to the serial port (COM1).
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_serial_print(format_args!($($arg)*));
    };
}

/// Print to the serial port (COM1) with a newline.
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => {
        $crate::serial_print!("{}\n", format_args!($($arg)*));
    };
}

/// Internal function used by the serial_print! macro.
#[doc(hidden)]
pub fn _serial_print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    SERIAL1.lock().write_fmt(args).expect("Printing to serial failed");
}
