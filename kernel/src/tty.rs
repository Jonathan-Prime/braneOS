// ============================================================
// Brane OS Kernel — TTY Driver
// ============================================================
//
// Provides a virtual terminal that combines keyboard input
// with serial + framebuffer output.
//
// Input comes from the PS/2 keyboard interrupt handler.
// Output goes to both serial and framebuffer.
//
// Spec reference: ARCHITECTURE.md §5.3 (planned)
// ============================================================

use spin::Mutex;

/// Size of the keyboard input buffer.
const INPUT_BUF_SIZE: usize = 1024;

/// Maximum command line length.
pub const MAX_LINE: usize = 256;

// -----------------------------------------------------------------------
// Input Ring Buffer
// -----------------------------------------------------------------------

struct InputBuffer {
    buf: [u8; INPUT_BUF_SIZE],
    head: usize, // write position
    tail: usize, // read position
    count: usize,
}

impl InputBuffer {
    const fn new() -> Self {
        Self {
            buf: [0; INPUT_BUF_SIZE],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    fn push(&mut self, byte: u8) {
        if self.count < INPUT_BUF_SIZE {
            self.buf[self.head] = byte;
            self.head = (self.head + 1) % INPUT_BUF_SIZE;
            self.count += 1;
        }
    }

    fn pop(&mut self) -> Option<u8> {
        if self.count == 0 {
            return None;
        }
        let byte = self.buf[self.tail];
        self.tail = (self.tail + 1) % INPUT_BUF_SIZE;
        self.count -= 1;
        Some(byte)
    }

    #[allow(dead_code)]
    fn is_empty(&self) -> bool {
        self.count == 0
    }
}

// -----------------------------------------------------------------------
// TTY State
// -----------------------------------------------------------------------

pub struct Tty {
    input: InputBuffer,
    line_buf: [u8; MAX_LINE],
    line_len: usize,
    line_ready: bool,
}

impl Tty {
    const fn new() -> Self {
        Self {
            input: InputBuffer::new(),
            line_buf: [0; MAX_LINE],
            line_len: 0,
            line_ready: false,
        }
    }

    /// Called from the keyboard interrupt handler when a character is decoded.
    pub fn on_char(&mut self, c: char) {
        if c == '\n' || c == '\r' {
            // Line complete
            self.line_ready = true;
            self.input.push(b'\n');
            // Echo newline
            crate::serial_println!();
            crate::framebuffer::fb_print("\n");
        } else if c == '\x08' || c == '\x7f' {
            // Backspace
            if self.line_len > 0 {
                self.line_len -= 1;
                // Echo backspace
                crate::serial_print!("\x08 \x08");
                crate::framebuffer::fb_print("\x08 \x08");
            }
        } else if c.is_ascii() && self.line_len < MAX_LINE - 1 {
            self.line_buf[self.line_len] = c as u8;
            self.line_len += 1;
            self.input.push(c as u8);

            // Echo character
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            crate::serial_print!("{}", s);
            crate::framebuffer::fb_print(s);
        }
    }

    /// Check if a complete line is ready.
    pub fn has_line(&self) -> bool {
        self.line_ready
    }

    /// Read the current line buffer. Returns the line contents (without newline).
    /// Resets the line for next input.
    pub fn read_line(&mut self) -> &str {
        let line = core::str::from_utf8(&self.line_buf[..self.line_len]).unwrap_or("");
        // We don't reset here — caller should call clear_line() when done
        line
    }

    /// Clear the line buffer for the next command.
    pub fn clear_line(&mut self) {
        self.line_len = 0;
        self.line_ready = false;
        // Also drain remaining input bytes
        while self.input.pop().is_some() {}
    }
}

/// Global TTY instance.
pub static TTY: Mutex<Tty> = Mutex::new(Tty::new());

/// Write output to both serial and framebuffer.
pub fn tty_print(s: &str) {
    crate::serial_print!("{}", s);
    crate::framebuffer::fb_print(s);
}

/// Write output with newline to both serial and framebuffer.
pub fn tty_println(s: &str) {
    crate::serial_println!("{}", s);
    crate::framebuffer::fb_print(s);
    crate::framebuffer::fb_print("\n");
}
