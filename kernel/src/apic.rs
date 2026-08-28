//! APIC register definitions and deterministic redirection-table helpers.
//!
//! Hardware MMIO access is intentionally kept out of this module until the
//! interrupt routing path is enabled during SMP bring-up.

pub const LOCAL_APIC_DEFAULT_BASE: u64 = 0xFEE0_0000;
pub const LOCAL_APIC_ID: u32 = 0x020;
pub const LOCAL_APIC_VERSION: u32 = 0x030;
pub const LOCAL_APIC_SIVR: u32 = 0x0F0;
pub const LOCAL_APIC_EOI: u32 = 0x0B0;
pub const IO_APIC_REGSEL: u32 = 0x00;
pub const IO_APIC_WINDOW: u32 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedirectionEntry {
    pub vector: u8,
    pub delivery_mode: u8,
    pub masked: bool,
    pub destination: u8,
}

impl RedirectionEntry {
    pub const fn new(vector: u8, destination: u8) -> Self {
        Self {
            vector,
            delivery_mode: 0,
            masked: true,
            destination,
        }
    }

    pub const fn unmasked(mut self) -> Self {
        self.masked = false;
        self
    }

    pub const fn to_raw(self) -> u64 {
        let mut value = self.vector as u64 | ((self.delivery_mode as u64 & 7) << 8);
        if self.masked {
            value |= 1 << 16;
        }
        value | ((self.destination as u64) << 56)
    }
}

pub const fn is_valid_mmio_base(address: u64) -> bool {
    address != 0 && address & 0xFFF == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_masked_redirection_entry() {
        let raw = RedirectionEntry::new(0x31, 2).to_raw();
        assert_eq!(raw & 0xFF, 0x31);
        assert_ne!(raw & (1 << 16), 0);
        assert_eq!(raw >> 56, 2);
    }

    #[test]
    fn unmask_clears_mask_bit() {
        assert_eq!(
            RedirectionEntry::new(32, 0).unmasked().to_raw() & (1 << 16),
            0
        );
    }

    #[test]
    fn validates_page_aligned_mmio_addresses() {
        assert!(is_valid_mmio_base(LOCAL_APIC_DEFAULT_BASE));
        assert!(!is_valid_mmio_base(0));
        assert!(!is_valid_mmio_base(0xFEE0_0001));
    }
}
