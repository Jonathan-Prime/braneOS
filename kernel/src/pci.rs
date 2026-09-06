//! PCI configuration-space discovery and resource decoding.
//!
//! The bare-metal backend uses PCI Configuration Mechanism #1 (CF8/CFC),
//! serialized across CPUs. Enumeration starts at bus zero, follows PCI-to-PCI
//! bridges, visits all functions of multifunction devices, and records a
//! fixed-size inventory without allocating from the kernel heap.

#![allow(dead_code)]

use spin::Mutex;

#[cfg(target_os = "none")]
use x86_64::instructions::port::Port;

const CONFIG_ADDRESS_PORT: u16 = 0xCF8;
const CONFIG_DATA_PORT: u16 = 0xCFC;
const INVALID_VENDOR_ID: u16 = 0xFFFF;

pub const MAX_PCI_DEVICES: usize = 256;
pub const MAX_PCI_BARS: usize = 6;
pub const COMMAND_IO_SPACE: u16 = 1 << 0;
pub const COMMAND_MEMORY_SPACE: u16 = 1 << 1;
pub const COMMAND_BUS_MASTER: u16 = 1 << 2;

static CONFIG_ACCESS_LOCK: Mutex<()> = Mutex::new(());

/// A PCI bus/device/function tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PciAddress {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciAddress {
    pub const fn new(bus: u8, device: u8, function: u8) -> Option<Self> {
        if device < 32 && function < 8 {
            Some(Self {
                bus,
                device,
                function,
            })
        } else {
            None
        }
    }

    const fn config_address(self, offset: u8) -> u32 {
        0x8000_0000
            | ((self.bus as u32) << 16)
            | ((self.device as u32) << 11)
            | ((self.function as u32) << 8)
            | ((offset as u32) & 0xFC)
    }
}

/// One decoded Base Address Register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciBar {
    Unused,
    Io { base: u32 },
    Memory32 { base: u32, prefetchable: bool },
    Memory64 { base: u64, prefetchable: bool },
    UpperHalf64,
    Reserved { raw: u32 },
}

impl PciBar {
    pub const fn base(self) -> Option<u64> {
        match self {
            Self::Io { base } | Self::Memory32 { base, .. } => Some(base as u64),
            Self::Memory64 { base, .. } => Some(base),
            Self::Unused | Self::UpperHalf64 | Self::Reserved { .. } => None,
        }
    }

    pub const fn is_io(self) -> bool {
        matches!(self, Self::Io { .. })
    }
}

/// A discovered PCI function and the resources advertised by its header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciDevice {
    pub address: PciAddress,
    pub vendor_id: u16,
    pub device_id: u16,
    pub command: u16,
    pub status: u16,
    pub revision_id: u8,
    pub programming_interface: u8,
    pub subclass: u8,
    pub class_code: u8,
    pub header_type: u8,
    pub multifunction: bool,
    pub subsystem_vendor_id: u16,
    pub subsystem_id: u16,
    pub bars: [PciBar; MAX_PCI_BARS],
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
    pub secondary_bus: Option<u8>,
}

impl PciDevice {
    pub const EMPTY: Self = Self {
        address: PciAddress {
            bus: 0,
            device: 0,
            function: 0,
        },
        vendor_id: 0,
        device_id: 0,
        command: 0,
        status: 0,
        revision_id: 0,
        programming_interface: 0,
        subclass: 0,
        class_code: 0,
        header_type: 0,
        multifunction: false,
        subsystem_vendor_id: 0,
        subsystem_id: 0,
        bars: [PciBar::Unused; MAX_PCI_BARS],
        interrupt_line: 0xFF,
        interrupt_pin: 0,
        secondary_bus: None,
    };

    pub const fn bar(self, index: usize) -> Option<PciBar> {
        if index < MAX_PCI_BARS {
            Some(self.bars[index])
        } else {
            None
        }
    }

    pub const fn is_pci_bridge(self) -> bool {
        self.class_code == 0x06 && self.subclass == 0x04
    }
}

/// Fixed-capacity PCI inventory.
#[derive(Clone, Copy)]
pub struct PciInventory {
    devices: [Option<PciDevice>; MAX_PCI_DEVICES],
    count: usize,
    overflowed: bool,
    buses_scanned: usize,
}

impl PciInventory {
    pub const fn new() -> Self {
        Self {
            devices: [None; MAX_PCI_DEVICES],
            count: 0,
            overflowed: false,
            buses_scanned: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.count
    }

    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub const fn overflowed(&self) -> bool {
        self.overflowed
    }

    pub const fn buses_scanned(&self) -> usize {
        self.buses_scanned
    }

    pub fn devices(&self) -> impl Iterator<Item = &PciDevice> {
        self.devices[..self.count].iter().flatten()
    }

    pub fn find(&self, mut predicate: impl FnMut(&PciDevice) -> bool) -> Option<PciDevice> {
        self.devices().find(|device| predicate(device)).copied()
    }

    fn push(&mut self, device: PciDevice) {
        if self.count == MAX_PCI_DEVICES {
            self.overflowed = true;
            return;
        }
        self.devices[self.count] = Some(device);
        self.count += 1;
    }

    fn clear(&mut self) {
        self.devices.fill(None);
        self.count = 0;
        self.overflowed = false;
        self.buses_scanned = 0;
    }
}

impl Default for PciInventory {
    fn default() -> Self {
        Self::new()
    }
}

/// Backend used by the topology walker. Tests provide an in-memory backend.
pub trait PciConfigAccess {
    fn read_u32(&self, address: PciAddress, offset: u8) -> u32;
}

pub struct LegacyConfigAccess;

impl PciConfigAccess for LegacyConfigAccess {
    fn read_u32(&self, address: PciAddress, offset: u8) -> u32 {
        #[cfg(target_os = "none")]
        {
            let _guard = CONFIG_ACCESS_LOCK.lock();
            unsafe {
                let mut address_port = Port::<u32>::new(CONFIG_ADDRESS_PORT);
                let mut data_port = Port::<u32>::new(CONFIG_DATA_PORT);
                address_port.write(address.config_address(offset));
                data_port.read()
            }
        }
        #[cfg(not(target_os = "none"))]
        {
            let _ = (address, offset);
            u32::MAX
        }
    }
}

/// Enable legacy port decoding and DMA for a discovered PCI function.
///
/// Only the 16-bit command register is written, avoiding write-one-to-clear
/// status bits in the upper half of the configuration dword.
pub fn enable_legacy_io_bus_mastering(device: PciDevice) -> u16 {
    let command = device.command | COMMAND_IO_SPACE | COMMAND_BUS_MASTER;
    #[cfg(target_os = "none")]
    {
        let _guard = CONFIG_ACCESS_LOCK.lock();
        unsafe {
            let mut address_port = Port::<u32>::new(CONFIG_ADDRESS_PORT);
            let mut command_port = Port::<u16>::new(CONFIG_DATA_PORT);
            address_port.write(device.address.config_address(0x04));
            command_port.write(command);
        }
    }
    command
}

fn read_u16(access: &impl PciConfigAccess, address: PciAddress, offset: u8) -> u16 {
    let value = access.read_u32(address, offset & 0xFC);
    ((value >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

fn function_exists(access: &impl PciConfigAccess, address: PciAddress) -> bool {
    read_u16(access, address, 0x00) != INVALID_VENDOR_ID
}

fn decode_bars(
    access: &impl PciConfigAccess,
    address: PciAddress,
    header_type: u8,
) -> [PciBar; MAX_PCI_BARS] {
    let mut bars = [PciBar::Unused; MAX_PCI_BARS];
    let limit = match header_type & 0x7F {
        0x00 => 6,
        0x01 => 2,
        _ => 0,
    };
    let mut index = 0;
    while index < limit {
        let raw = access.read_u32(address, 0x10 + (index as u8 * 4));
        if raw == 0 || raw == u32::MAX {
            index += 1;
            continue;
        }
        if raw & 1 != 0 {
            bars[index] = PciBar::Io {
                base: raw & 0xFFFF_FFFC,
            };
            index += 1;
            continue;
        }

        let prefetchable = raw & 0x8 != 0;
        match (raw >> 1) & 0x3 {
            0x0 | 0x1 => {
                bars[index] = PciBar::Memory32 {
                    base: raw & 0xFFFF_FFF0,
                    prefetchable,
                };
                index += 1;
            }
            0x2 if index + 1 < limit => {
                let upper = access.read_u32(address, 0x10 + ((index + 1) as u8 * 4));
                bars[index] = PciBar::Memory64 {
                    base: ((upper as u64) << 32) | ((raw & 0xFFFF_FFF0) as u64),
                    prefetchable,
                };
                bars[index + 1] = PciBar::UpperHalf64;
                index += 2;
            }
            _ => {
                bars[index] = PciBar::Reserved { raw };
                index += 1;
            }
        }
    }
    bars
}

fn read_device(access: &impl PciConfigAccess, address: PciAddress) -> Option<PciDevice> {
    let identity = access.read_u32(address, 0x00);
    let vendor_id = identity as u16;
    if vendor_id == INVALID_VENDOR_ID {
        return None;
    }
    let command_status = access.read_u32(address, 0x04);
    let class_revision = access.read_u32(address, 0x08);
    let header = access.read_u32(address, 0x0C);
    let raw_header_type = (header >> 16) as u8;
    let header_type = raw_header_type & 0x7F;
    let subsystem = if header_type == 0 {
        access.read_u32(address, 0x2C)
    } else {
        0
    };
    let interrupt = access.read_u32(address, 0x3C);
    let secondary_bus = if header_type == 1 {
        let buses = access.read_u32(address, 0x18);
        let secondary = (buses >> 8) as u8;
        (secondary != 0).then_some(secondary)
    } else {
        None
    };

    Some(PciDevice {
        address,
        vendor_id,
        device_id: (identity >> 16) as u16,
        command: command_status as u16,
        status: (command_status >> 16) as u16,
        revision_id: class_revision as u8,
        programming_interface: (class_revision >> 8) as u8,
        subclass: (class_revision >> 16) as u8,
        class_code: (class_revision >> 24) as u8,
        header_type,
        multifunction: raw_header_type & 0x80 != 0,
        subsystem_vendor_id: subsystem as u16,
        subsystem_id: (subsystem >> 16) as u16,
        bars: decode_bars(access, address, header_type),
        interrupt_line: interrupt as u8,
        interrupt_pin: (interrupt >> 8) as u8,
        secondary_bus,
    })
}

fn enqueue_bus(
    bus: u8,
    pending: &mut [u8; 256],
    pending_len: &mut usize,
    queued: &mut [bool; 256],
) {
    if queued[bus as usize] || *pending_len == pending.len() {
        return;
    }
    queued[bus as usize] = true;
    pending[*pending_len] = bus;
    *pending_len += 1;
}

fn enumerate_into(access: &impl PciConfigAccess, inventory: &mut PciInventory) {
    inventory.clear();
    let mut pending = [0u8; 256];
    let mut queued = [false; 256];
    let mut pending_len = 0usize;
    let mut cursor = 0usize;
    enqueue_bus(0, &mut pending, &mut pending_len, &mut queued);

    while cursor < pending_len {
        let bus = pending[cursor];
        cursor += 1;
        inventory.buses_scanned += 1;

        for device_number in 0..32u8 {
            let function_zero = PciAddress {
                bus,
                device: device_number,
                function: 0,
            };
            let Some(first) = read_device(access, function_zero) else {
                continue;
            };
            let function_count = if first.multifunction { 8 } else { 1 };

            for function in 0..function_count {
                let address = PciAddress {
                    bus,
                    device: device_number,
                    function,
                };
                let pci_device = if function == 0 {
                    first
                } else {
                    if !function_exists(access, address) {
                        continue;
                    }
                    let Some(device) = read_device(access, address) else {
                        continue;
                    };
                    device
                };

                if pci_device.is_pci_bridge() {
                    if let Some(secondary) = pci_device.secondary_bus {
                        enqueue_bus(secondary, &mut pending, &mut pending_len, &mut queued);
                    }
                }
                // A multifunction host bridge may expose one root bus per
                // function even without a type-1 PCI bridge header.
                if bus == 0
                    && device_number == 0
                    && function != 0
                    && pci_device.class_code == 0x06
                    && pci_device.subclass == 0x00
                {
                    enqueue_bus(function, &mut pending, &mut pending_len, &mut queued);
                }
                inventory.push(pci_device);
            }
        }
    }
}

/// Enumerate all PCI functions reachable from the root bus.
///
/// This value-returning form is primarily useful for host tests. Bare metal
/// enumerates directly into the static inventory to avoid placing the large
/// fixed-capacity table on the small boot stack.
pub fn enumerate_with(access: &impl PciConfigAccess) -> PciInventory {
    let mut inventory = PciInventory::new();
    enumerate_into(access, &mut inventory);
    inventory
}

pub static PCI_INVENTORY: Mutex<PciInventory> = Mutex::new(PciInventory::new());

/// Discover PCI topology through the legacy configuration-space backend.
pub fn init() -> (usize, usize, bool) {
    let mut inventory = PCI_INVENTORY.lock();
    enumerate_into(&LegacyConfigAccess, &mut inventory);
    (
        inventory.len(),
        inventory.buses_scanned(),
        inventory.overflowed(),
    )
}

/// Find one function in the boot-time inventory.
pub fn find_device(predicate: impl FnMut(&PciDevice) -> bool) -> Option<PciDevice> {
    PCI_INVENTORY.lock().find(predicate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct MockConfig {
        registers: BTreeMap<(u8, u8, u8, u8), u32>,
    }

    impl MockConfig {
        fn write(&mut self, address: PciAddress, offset: u8, value: u32) {
            self.registers.insert(
                (address.bus, address.device, address.function, offset & 0xFC),
                value,
            );
        }

        fn add_device(
            &mut self,
            address: PciAddress,
            vendor: u16,
            device: u16,
            class: u8,
            subclass: u8,
            header_type: u8,
        ) {
            self.write(address, 0x00, ((device as u32) << 16) | vendor as u32);
            self.write(
                address,
                0x08,
                ((class as u32) << 24) | ((subclass as u32) << 16),
            );
            self.write(address, 0x0C, (header_type as u32) << 16);
        }
    }

    impl PciConfigAccess for MockConfig {
        fn read_u32(&self, address: PciAddress, offset: u8) -> u32 {
            self.registers
                .get(&(address.bus, address.device, address.function, offset & 0xFC))
                .copied()
                .unwrap_or(u32::MAX)
        }
    }

    fn address(bus: u8, device: u8, function: u8) -> PciAddress {
        PciAddress::new(bus, device, function).unwrap()
    }

    #[test]
    fn follows_bridges_and_multifunction_devices() {
        let mut config = MockConfig::default();
        config.add_device(address(0, 1, 0), 0x1234, 0x0001, 0x02, 0x00, 0x00);
        config.add_device(address(0, 2, 0), 0x1234, 0x0002, 0x0C, 0x03, 0x80);
        config.add_device(address(0, 2, 1), 0x1234, 0x0003, 0x0C, 0x03, 0x00);
        config.add_device(address(0, 3, 0), 0x1234, 0x0004, 0x06, 0x04, 0x01);
        config.write(address(0, 3, 0), 0x18, 2 << 8);
        config.add_device(address(2, 0, 0), 0xABCD, 0x0005, 0x01, 0x08, 0x00);

        let inventory = enumerate_with(&config);

        assert_eq!(inventory.len(), 5);
        assert_eq!(inventory.buses_scanned(), 2);
        assert!(inventory
            .devices()
            .any(|device| device.address == address(0, 2, 1)));
        assert!(inventory
            .devices()
            .any(|device| device.address == address(2, 0, 0)));
    }

    #[test]
    fn decodes_io_memory32_and_memory64_bars() {
        let mut config = MockConfig::default();
        let device = address(0, 1, 0);
        config.add_device(device, 0x1234, 0x5678, 0x01, 0x00, 0x00);
        config.write(device, 0x10, 0x0000_C001);
        config.write(device, 0x14, 0xFEBF_0008);
        config.write(device, 0x18, 0x0000_1004);
        config.write(device, 0x1C, 0x0000_0001);

        let inventory = enumerate_with(&config);
        let discovered = inventory.devices().next().unwrap();

        assert_eq!(discovered.bars[0], PciBar::Io { base: 0xC000 });
        assert_eq!(
            discovered.bars[1],
            PciBar::Memory32 {
                base: 0xFEBF_0000,
                prefetchable: true,
            }
        );
        assert_eq!(
            discovered.bars[2],
            PciBar::Memory64 {
                base: 0x1_0000_1000,
                prefetchable: false,
            }
        );
        assert_eq!(discovered.bars[3], PciBar::UpperHalf64);
    }

    #[test]
    fn enables_io_decoding_and_bus_mastering_without_dropping_command_bits() {
        let mut device = PciDevice::EMPTY;
        device.command = COMMAND_MEMORY_SPACE | (1 << 6);

        assert_eq!(
            enable_legacy_io_bus_mastering(device),
            COMMAND_MEMORY_SPACE | COMMAND_IO_SPACE | COMMAND_BUS_MASTER | (1 << 6)
        );
    }
}
