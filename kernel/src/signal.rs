// ============================================================
// Brane OS Kernel — POSIX Signals
// ============================================================
//
// Implements POSIX-like signal delivery, handlers, and actions.
//
// Spec reference: ROADMAP.md Fase 10 (POSIX signals)
// ============================================================

use crate::process::{Pid, PROCESS_TABLE};
use spin::Mutex;

const MAX_PROCESSES: usize = 128;

/// POSIX signal numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Signal {
    Hup = 1,
    Int = 2,
    Quit = 3,
    Kill = 9,
    Usr1 = 10,
    Usr2 = 12,
    Term = 15,
    Chld = 17,
    Cont = 18,
    Stop = 19,
}

impl TryFrom<u8> for Signal {
    type Error = &'static str;
    fn try_from(val: u8) -> Result<Self, Self::Error> {
        match val {
            1 => Ok(Signal::Hup),
            2 => Ok(Signal::Int),
            3 => Ok(Signal::Quit),
            9 => Ok(Signal::Kill),
            10 => Ok(Signal::Usr1),
            12 => Ok(Signal::Usr2),
            15 => Ok(Signal::Term),
            17 => Ok(Signal::Chld),
            18 => Ok(Signal::Cont),
            19 => Ok(Signal::Stop),
            _ => Err("Invalid signal number"),
        }
    }
}

/// Actions to perform upon receiving a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalAction {
    Default,
    Ignore,
    Handler(u64), // virtual address of user-space handler
}

/// Actions table for a single process.
#[derive(Debug, Clone, Copy)]
pub struct SignalTable {
    pub actions: [SignalAction; 32],
}

impl SignalTable {
    pub const fn new() -> Self {
        Self {
            actions: [SignalAction::Default; 32],
        }
    }
}

impl Default for SignalTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Manages registered signal handlers across all process slots.
pub struct SignalManager {
    pub tables: [SignalTable; MAX_PROCESSES],
}

impl SignalManager {
    pub const fn new() -> Self {
        Self {
            tables: [SignalTable::new(); MAX_PROCESSES],
        }
    }

    /// Maps a process ID to its index in the static table.
    fn slot_of(&self, pid: Pid) -> Option<usize> {
        let p_table = PROCESS_TABLE.lock();
        p_table.slot_of(pid)
    }

    /// Send a signal to a target process.
    pub fn send(&mut self, pid: Pid, sig: Signal) -> Result<(), &'static str> {
        let mut p_table = PROCESS_TABLE.lock();
        if let Some(proc) = p_table.get_mut(pid) {
            if proc.state == crate::process::ProcessState::Terminated {
                return Err("Process already terminated");
            }
            let sig_bit = 1 << (sig as u8);
            proc.signal_pending |= sig_bit;
            Ok(())
        } else {
            Err("Process not found")
        }
    }

    /// Register a signal action for a target process.
    pub fn set_action(
        &mut self,
        pid: Pid,
        sig: Signal,
        action: SignalAction,
    ) -> Result<(), &'static str> {
        if sig == Signal::Kill || sig == Signal::Stop {
            return Err("SIGKILL and SIGSTOP cannot be caught or ignored");
        }
        if let Some(slot) = self.slot_of(pid) {
            let sig_num = sig as usize;
            if sig_num < 32 {
                self.tables[slot].actions[sig_num] = action;
                Ok(())
            } else {
                Err("Signal number out of range")
            }
        } else {
            Err("Process not found")
        }
    }

    /// Get the registered action for a signal.
    pub fn get_action(&self, pid: Pid, sig: Signal) -> SignalAction {
        if let Some(slot) = self.slot_of(pid) {
            let sig_num = sig as usize;
            if sig_num < 32 {
                return self.tables[slot].actions[sig_num];
            }
        }
        SignalAction::Default
    }

    /// Returns the mask of currently pending and unblocked signals.
    pub fn pending_for(&self, pid: Pid) -> u64 {
        let p_table = PROCESS_TABLE.lock();
        if let Some(proc) = p_table.get(pid) {
            proc.signal_pending & !proc.signal_blocked
        } else {
            0
        }
    }

    /// Clears a signal from the pending mask.
    pub fn clear_pending(&mut self, pid: Pid, sig: Signal) {
        let mut p_table = PROCESS_TABLE.lock();
        if let Some(proc) = p_table.get_mut(pid) {
            let sig_bit = 1 << (sig as u8);
            proc.signal_pending &= !sig_bit;
        }
    }

    /// Scans pending signals and processes the first active one.
    /// Returns the signal and handler address if a custom handler is registered.
    pub fn deliver_pending(&mut self, pid: Pid) -> Option<(Signal, u64)> {
        let pending_mask = self.pending_for(pid);
        if pending_mask == 0 {
            return None;
        }

        let signals_to_check = [
            Signal::Kill,
            Signal::Term,
            Signal::Int,
            Signal::Quit,
            Signal::Hup,
            Signal::Usr1,
            Signal::Usr2,
            Signal::Chld,
            Signal::Cont,
            Signal::Stop,
        ];

        for sig in signals_to_check {
            let sig_bit = 1 << (sig as u8);
            if (pending_mask & sig_bit) != 0 {
                self.clear_pending(pid, sig);

                let action = self.get_action(pid, sig);
                match action {
                    SignalAction::Ignore => {
                        continue;
                    }
                    SignalAction::Default => match sig {
                        Signal::Chld | Signal::Cont => {
                            continue;
                        }
                        Signal::Stop => {
                            continue;
                        }
                        _ => {
                            let mut p_table = PROCESS_TABLE.lock();
                            p_table.terminate(pid, 128 + sig as i32);
                            return None;
                        }
                    },
                    SignalAction::Handler(handler_addr) => {
                        return Some((sig, handler_addr));
                    }
                }
            }
        }
        None
    }
}

impl Default for SignalManager {
    fn default() -> Self {
        Self::new()
    }
}

pub static SIGNAL_MANAGER: Mutex<SignalManager> = Mutex::new(SignalManager::new());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_try_from() {
        assert_eq!(Signal::try_from(9), Ok(Signal::Kill));
        assert_eq!(Signal::try_from(15), Ok(Signal::Term));
        assert_eq!(Signal::try_from(99), Err("Invalid signal number"));
    }

    #[test]
    fn test_signal_manager_defaults() {
        let manager = SignalManager::new();
        assert_eq!(manager.get_action(1, Signal::Term), SignalAction::Default);
    }
}
