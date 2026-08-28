//! APIC register definitions, MMIO windows and deterministic redirection
//! helpers.
//!
//! The register wrappers deliberately keep volatile access behind `unsafe`
//! methods.  Discovery and address arithmetic remain safe and are exercised
//! by host-side tests; only the bare-metal bring-up maps the MMIO pages.

#[cfg(target_os = "none")]
use x86_64::structures::paging::{Mapper, Page, PageTableFlags, PhysFrame, Size2MiB, Translate};
#[cfg(target_os = "none")]
use x86_64::{PhysAddr, VirtAddr};

pub const LOCAL_APIC_DEFAULT_BASE: u64 = 0xFEE0_0000;
pub const LOCAL_APIC_ID: u32 = 0x020;
pub const LOCAL_APIC_VERSION: u32 = 0x030;
pub const LOCAL_APIC_SIVR: u32 = 0x0F0;
pub const LOCAL_APIC_EOI: u32 = 0x0B0;
pub const LOCAL_APIC_SIVR_ENABLE: u32 = 1 << 8;
pub const IO_APIC_REGSEL: u32 = 0x00;
pub const IO_APIC_WINDOW: u32 = 0x10;
pub const IO_APIC_ID: u32 = 0x00;
pub const IO_APIC_VERSION: u32 = 0x01;
pub const IO_APIC_ARBITRATION: u32 = 0x02;
pub const IO_APIC_REDIRECTION_BASE: u32 = 0x10;

/// Memory-mapped Local APIC window. The caller must provide the physical
/// base reported by MADT and the kernel's direct-map offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalApic {
    physical_base: u64,
    virtual_base: u64,
}

impl LocalApic {
    pub fn new(physical_base: u64, physical_memory_offset: u64) -> Option<Self> {
        if !is_valid_mmio_base(physical_base) {
            return None;
        }
        Some(Self {
            physical_base,
            virtual_base: physical_base.checked_add(physical_memory_offset)?,
        })
    }

    pub const fn physical_base(self) -> u64 {
        self.physical_base
    }

    pub const fn virtual_base(self) -> u64 {
        self.virtual_base
    }

    /// Returns the direct-map address for a register offset.
    pub const fn register_address(self, register: u32) -> Option<u64> {
        self.virtual_base.checked_add(register as u64)
    }

    /// Read one 32-bit APIC register through the direct map.
    ///
    /// # Safety
    /// The direct-map address must be mapped and point to a Local APIC page.
    pub unsafe fn read(self, register: u32) -> u32 {
        let address = self
            .register_address(register)
            .expect("Local APIC register address overflow");
        core::ptr::read_volatile(address as *const u32)
    }

    /// Write one 32-bit APIC register through the direct map.
    ///
    /// # Safety
    /// The direct-map address must be mapped and point to a writable Local
    /// APIC page; the register must be valid for the target APIC.
    pub unsafe fn write(self, register: u32, value: u32) {
        let address = self
            .register_address(register)
            .expect("Local APIC register address overflow");
        core::ptr::write_volatile(address as *mut u32, value);
    }

    /// Enable the Local APIC with the supplied spurious-interrupt vector.
    ///
    /// # Safety
    /// The Local APIC MMIO page must be mapped and owned by this CPU.
    pub unsafe fn enable(self, spurious_vector: u8) {
        let value = self.read(LOCAL_APIC_SIVR);
        self.write(
            LOCAL_APIC_SIVR,
            (value & !0xff) | spurious_vector as u32 | LOCAL_APIC_SIVR_ENABLE,
        );
    }

    /// Signal end-of-interrupt to the Local APIC.
    ///
    /// # Safety
    /// The Local APIC MMIO page must be mapped.
    pub unsafe fn end_of_interrupt(self) {
        self.write(LOCAL_APIC_EOI, 0);
    }
}

/// Memory-mapped I/O APIC window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoApic {
    physical_base: u64,
    virtual_base: u64,
}

impl IoApic {
    pub fn new(physical_base: u64, physical_memory_offset: u64) -> Option<Self> {
        if !is_valid_mmio_base(physical_base) {
            return None;
        }
        Some(Self {
            physical_base,
            virtual_base: physical_base.checked_add(physical_memory_offset)?,
        })
    }

    pub const fn physical_base(self) -> u64 {
        self.physical_base
    }

    pub const fn virtual_base(self) -> u64 {
        self.virtual_base
    }

    pub const fn register_address(self, register: u32) -> Option<u64> {
        self.virtual_base.checked_add(register as u64)
    }

    /// Read one I/O APIC register through the selector/window pair.
    ///
    /// # Safety
    /// The direct-map address must be mapped to an I/O APIC page. Calls must
    /// be serialized when multiple CPUs can access the same controller.
    pub unsafe fn read_register(self, register: u8) -> u32 {
        let selector = self
            .register_address(IO_APIC_REGSEL)
            .expect("I/O APIC selector address overflow") as *mut u32;
        let window = self
            .register_address(IO_APIC_WINDOW)
            .expect("I/O APIC window address overflow") as *const u32;
        core::ptr::write_volatile(selector, register as u32);
        core::ptr::read_volatile(window)
    }

    /// Write one I/O APIC register through the selector/window pair.
    ///
    /// # Safety
    /// The direct-map address must be mapped to a writable I/O APIC page.
    /// Calls must be serialized when multiple CPUs can access the controller.
    pub unsafe fn write_register(self, register: u8, value: u32) {
        let selector = self
            .register_address(IO_APIC_REGSEL)
            .expect("I/O APIC selector address overflow") as *mut u32;
        let window = self
            .register_address(IO_APIC_WINDOW)
            .expect("I/O APIC window address overflow") as *mut u32;
        core::ptr::write_volatile(selector, register as u32);
        core::ptr::write_volatile(window, value);
    }

    /// Return the number of redirection entries advertised by a VERSION
    /// register value. The encoded field is the highest valid index.
    pub const fn redirection_count(version: u32) -> u16 {
        ((version >> 16) & 0xff) as u16 + 1
    }

    const fn redirection_registers(index: u8) -> Option<(u8, u8)> {
        let low = IO_APIC_REDIRECTION_BASE as u16 + index as u16 * 2;
        let high = low + 1;
        if high > u8::MAX as u16 {
            None
        } else {
            Some((low as u8, high as u8))
        }
    }

    /// Read one redirection-table entry.
    ///
    /// Returns `None` when the register pair cannot be represented by the
    /// controller's 8-bit selector (indices above the architectural range).
    ///
    /// # Safety
    /// The I/O APIC MMIO page must be mapped and this controller must not be
    /// accessed concurrently.
    pub unsafe fn read_redirection(self, index: u8) -> Option<RedirectionEntry> {
        let (low, high) = Self::redirection_registers(index)?;
        let value = self.read_register(low) as u64 | (self.read_register(high) as u64) << 32;
        Some(RedirectionEntry::from_raw(value))
    }

    /// Write one redirection-table entry.
    ///
    /// # Safety
    /// The I/O APIC MMIO page must be mapped and this controller must not be
    /// accessed concurrently.
    pub unsafe fn write_redirection(self, index: u8, entry: RedirectionEntry) -> bool {
        let Some((low, high)) = Self::redirection_registers(index) else {
            return false;
        };
        let value = entry.to_raw();
        self.write_register(low, value as u32);
        self.write_register(high, (value >> 32) as u32);
        true
    }
}

/// Summary of the APIC topology discovered from MADT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApicTopology {
    pub local_apic_address: u64,
    pub enabled_cpu_count: usize,
    pub io_apic_count: usize,
    pub first_io_apic_address: Option<u64>,
}

impl ApicTopology {
    pub fn from_madt(info: &crate::madt::MadtInfo) -> Self {
        Self {
            local_apic_address: info.local_apic_address,
            enabled_cpu_count: info.enabled_cpu_count(),
            io_apic_count: info.io_apic_count,
            first_io_apic_address: (info.io_apic_count > 0)
                .then(|| info.io_apics[0])
                .flatten()
                .map(|entry| entry.address as u64),
        }
    }
}

/// Map one 4 KiB APIC MMIO page into the kernel's direct-map address space.
///
/// The bootloader normally maps this range already. If it does not, the page
/// is installed with uncached, writable permissions before a register wrapper
/// is constructed; an existing mapping to the same frame is left intact. This
/// function is only meaningful on bare metal.
#[cfg(target_os = "none")]
pub fn map_mmio_page(
    mapper: &mut x86_64::structures::paging::OffsetPageTable<'static>,
    frame_allocator: &mut crate::memory::frame_allocator::BitmapFrameAllocator,
    physical_memory_offset: u64,
    physical_base: u64,
) -> Result<(), &'static str> {
    if !is_valid_mmio_base(physical_base) {
        return Err("APIC MMIO base is not page aligned");
    }
    let virtual_base = physical_base
        .checked_add(physical_memory_offset)
        .ok_or("APIC MMIO virtual address overflow")?;
    let page = Page::containing_address(VirtAddr::new(virtual_base));
    let frame = PhysFrame::containing_address(PhysAddr::new(physical_base));
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_EXECUTE
        | PageTableFlags::NO_CACHE;
    match mapper.translate_addr(page.start_address()) {
        Some(mapped) if mapped == frame.start_address() => {
            // The bootloader's direct map is cacheable by default. APIC
            // registers are side-effectful, so retain the mapping but switch
            // this page to uncached attributes before exposing it.
            unsafe {
                if let Ok(flush) = mapper.update_flags(page, flags) {
                    flush.flush();
                } else {
                    // bootloader 0.11 commonly uses 2 MiB direct-map pages;
                    // update the containing large page when a 4 KiB update
                    // is rejected because its parent entry is huge.
                    let large_page =
                        Page::<Size2MiB>::containing_address(VirtAddr::new(virtual_base));
                    let large_frame =
                        PhysFrame::<Size2MiB>::containing_address(PhysAddr::new(physical_base));
                    if mapper.translate_addr(large_page.start_address())
                        != Some(large_frame.start_address())
                    {
                        return Err("APIC MMIO page is mapped to an unexpected frame");
                    }
                    mapper
                        .update_flags(large_page, flags)
                        .map_err(|_| "failed to update APIC MMIO page flags")?
                        .flush();
                }
            }
            Ok(())
        }
        Some(_) => Err("APIC MMIO page is mapped to a different frame"),
        None => crate::memory::paging::map_page(mapper, page, frame, flags, frame_allocator),
    }
}

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

    pub const fn with_delivery_mode(mut self, delivery_mode: u8) -> Self {
        self.delivery_mode = delivery_mode & 7;
        self
    }

    pub const fn from_raw(raw: u64) -> Self {
        Self {
            vector: raw as u8,
            delivery_mode: ((raw >> 8) & 7) as u8,
            masked: raw & (1 << 16) != 0,
            destination: (raw >> 56) as u8,
        }
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

    #[test]
    fn constructs_direct_map_window() {
        let apic = LocalApic::new(LOCAL_APIC_DEFAULT_BASE, 0x1000_0000).unwrap();
        assert_eq!(apic.physical_base(), LOCAL_APIC_DEFAULT_BASE);
        assert_eq!(apic.virtual_base(), 0x10EE_0000_0);
        assert!(LocalApic::new(0xFEE0_0001, 0).is_none());
    }

    #[test]
    fn rejects_virtual_address_overflow() {
        assert!(LocalApic::new(u64::MAX - 0xfff, 0x1000).is_none());
        assert!(IoApic::new(u64::MAX - 0xfff, 0x1000).is_none());
    }

    #[test]
    fn decodes_and_encodes_redirection_entries() {
        let entry = RedirectionEntry::new(0x45, 3)
            .with_delivery_mode(4)
            .unmasked();
        assert_eq!(RedirectionEntry::from_raw(entry.to_raw()), entry);
    }

    #[test]
    fn computes_io_apic_redirection_count() {
        assert_eq!(IoApic::redirection_count(0x0017_0011), 0x18);
        assert_eq!(IoApic::redirection_count(0), 1);
    }

    #[test]
    fn validates_io_apic_register_pair_range() {
        assert!(IoApic::new(0xFEC0_0000, 0).is_some());
        assert!(IoApic::new(0xFEC0_0001, 0).is_none());
        assert!(IoApic::new(0xFEC0_0000, 0)
            .unwrap()
            .register_address(IO_APIC_WINDOW)
            .is_some());
    }

    #[test]
    fn topology_carries_first_io_apic() {
        let info = crate::madt::MadtInfo {
            local_apic_address: LOCAL_APIC_DEFAULT_BASE,
            cpus: [None; 32],
            cpu_count: 0,
            io_apics: [Some(crate::madt::IoApicEntry {
                id: 1,
                address: 0xFEC0_0000,
                global_irq_base: 0,
            }); 8],
            io_apic_count: 1,
        };
        let topology = ApicTopology::from_madt(&info);
        assert_eq!(topology.local_apic_address, LOCAL_APIC_DEFAULT_BASE);
        assert_eq!(topology.io_apic_count, 1);
        assert_eq!(topology.first_io_apic_address, Some(0xFEC0_0000));
    }
}
