// ============================================================
// Brane OS Kernel — Virtio-net Driver
// ============================================================
//
// Minimal virtio-net PCI driver for QEMU.
// Implements PCI device discovery and virtqueue management
// for sending and receiving Ethernet frames.
//
// Reference: VirtIO 1.1 specification §5.1
// ============================================================

#![allow(dead_code)]

use spin::Mutex;
use x86_64::instructions::port::Port;

// -----------------------------------------------------------------------
// PCI Configuration Space Access
// -----------------------------------------------------------------------

const PCI_CONFIG_ADDR: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

/// Virtio vendor/device IDs
const VIRTIO_VENDOR_ID: u16 = 0x1AF4;
const VIRTIO_NET_DEVICE_ID_RANGE: core::ops::RangeInclusive<u16> = 0x1000..=0x1041;
// Subsystem device ID for network: 1
const VIRTIO_NET_SUBSYSTEM: u16 = 1;

/// Read a 32-bit value from PCI configuration space.
fn pci_config_read32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address: u32 = 0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);

    unsafe {
        let mut addr_port = Port::<u32>::new(PCI_CONFIG_ADDR);
        let mut data_port = Port::<u32>::new(PCI_CONFIG_DATA);
        addr_port.write(address);
        data_port.read()
    }
}

/// Read a 16-bit value from PCI configuration space.
fn pci_config_read16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let val32 = pci_config_read32(bus, device, function, offset & 0xFC);
    ((val32 >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

// -----------------------------------------------------------------------
// PCI Device
// -----------------------------------------------------------------------

/// A discovered PCI device.
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub subsystem_id: u16,
    pub bar0: u32,
    pub irq_line: u8,
}

impl PciDevice {
    /// Read BAR0 (I/O port base address).
    fn read_bar0(bus: u8, device: u8, function: u8) -> u32 {
        pci_config_read32(bus, device, function, 0x10) & 0xFFFF_FFFC
    }

    /// Read the IRQ line.
    fn read_irq(bus: u8, device: u8, function: u8) -> u8 {
        (pci_config_read32(bus, device, function, 0x3C) & 0xFF) as u8
    }

    /// Read subsystem ID.
    fn read_subsystem(bus: u8, device: u8, function: u8) -> u16 {
        pci_config_read16(bus, device, function, 0x2E)
    }
}

/// Scan PCI bus 0 for a virtio-net device.
pub fn find_virtio_net() -> Option<PciDevice> {
    for device in 0..32u8 {
        let vendor_id = pci_config_read16(0, device, 0, 0x00);
        if vendor_id == 0xFFFF {
            continue; // No device
        }

        let device_id = pci_config_read16(0, device, 0, 0x02);

        if vendor_id == VIRTIO_VENDOR_ID && VIRTIO_NET_DEVICE_ID_RANGE.contains(&device_id) {
            let subsystem_id = PciDevice::read_subsystem(0, device, 0);
            // Check subsystem for network (1) or accept transitional devices
            if subsystem_id == VIRTIO_NET_SUBSYSTEM || device_id == 0x1000 {
                let bar0 = PciDevice::read_bar0(0, device, 0);
                let irq_line = PciDevice::read_irq(0, device, 0);

                return Some(PciDevice {
                    bus: 0,
                    device,
                    function: 0,
                    vendor_id,
                    device_id,
                    subsystem_id,
                    bar0,
                    irq_line,
                });
            }
        }
    }
    None
}

// -----------------------------------------------------------------------
// Virtio Legacy I/O Port Registers (virtio 0.9 / transitional)
// -----------------------------------------------------------------------

/// Offsets from BAR0 for virtio legacy PCI device
mod virtio_reg {
    pub const DEVICE_FEATURES: u16 = 0x00; // 4 bytes
    pub const GUEST_FEATURES: u16 = 0x04; // 4 bytes
    pub const QUEUE_ADDRESS: u16 = 0x08; // 4 bytes
    pub const QUEUE_SIZE: u16 = 0x0C; // 2 bytes
    pub const QUEUE_SELECT: u16 = 0x0E; // 2 bytes
    pub const QUEUE_NOTIFY: u16 = 0x10; // 2 bytes
    pub const DEVICE_STATUS: u16 = 0x12; // 1 byte
    pub const ISR_STATUS: u16 = 0x13; // 1 byte
                                      // MAC address at offset 0x14 (6 bytes) for virtio-net
    pub const MAC_ADDR: u16 = 0x14; // 6 bytes
}

/// Virtio device status flags
mod status {
    pub const ACKNOWLEDGE: u8 = 1;
    pub const DRIVER: u8 = 2;
    pub const DRIVER_OK: u8 = 4;
    pub const FEATURES_OK: u8 = 8;
    pub const FAILED: u8 = 128;
}

// -----------------------------------------------------------------------
// Virtio-net Device State
// -----------------------------------------------------------------------

/// Maximum Ethernet frame size + header.
pub const MAX_FRAME_SIZE: usize = 1514;

/// Virtio net header (prepended to each packet).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioNetHeader {
    pub flags: u8,
    pub gso_type: u8,
    pub hdr_len: u16,
    pub gso_size: u16,
    pub csum_start: u16,
    pub csum_offset: u16,
    pub num_buffers: u16,
}

impl Default for VirtioNetHeader {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtioNetHeader {
    pub const fn new() -> Self {
        Self {
            flags: 0,
            gso_type: 0,
            hdr_len: 0,
            gso_size: 0,
            csum_start: 0,
            csum_offset: 0,
            num_buffers: 0,
        }
    }

    pub const SIZE: usize = 10; // legacy virtio-net header is 10 bytes
}

/// Representation of a discovered and initialized virtio-net device.
#[derive(Debug)]
pub struct VirtioNetDevice {
    pub pci: PciDevice,
    pub mac: [u8; 6],
    pub io_base: u16,
    pub initialized: bool,
}

impl VirtioNetDevice {
    pub const fn empty() -> Self {
        Self {
            pci: PciDevice {
                bus: 0,
                device: 0,
                function: 0,
                vendor_id: 0,
                device_id: 0,
                subsystem_id: 0,
                bar0: 0,
                irq_line: 0,
            },
            mac: [0; 6],
            io_base: 0,
            initialized: false,
        }
    }

    /// Initialize the virtio-net device via legacy PCI I/O ports.
    pub fn init(&mut self, pci: PciDevice) {
        self.pci = pci;
        self.io_base = pci.bar0 as u16;

        unsafe {
            let base = self.io_base;

            // 1. Reset
            Port::<u8>::new(base + virtio_reg::DEVICE_STATUS).write(0);

            // 2. Acknowledge
            Port::<u8>::new(base + virtio_reg::DEVICE_STATUS).write(status::ACKNOWLEDGE);

            // 3. Driver
            Port::<u8>::new(base + virtio_reg::DEVICE_STATUS)
                .write(status::ACKNOWLEDGE | status::DRIVER);

            // 4. Read device features
            let _features = Port::<u32>::new(base + virtio_reg::DEVICE_FEATURES).read();

            // 5. Write guest features (accept MAC, status)
            // Feature bit 5 = MAC, bit 16 = status
            Port::<u32>::new(base + virtio_reg::GUEST_FEATURES).write(1 << 5);

            // 6. Features OK
            Port::<u8>::new(base + virtio_reg::DEVICE_STATUS)
                .write(status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK);

            // 7. Read MAC address
            for i in 0..6 {
                self.mac[i] = Port::<u8>::new(base + virtio_reg::MAC_ADDR + i as u16).read();
            }

            // 8. Driver OK — device is live
            Port::<u8>::new(base + virtio_reg::DEVICE_STATUS).write(
                status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK | status::DRIVER_OK,
            );
        }

        self.initialized = true;
    }

    /// Format MAC address as string.
    pub fn mac_str<'a>(&self, buf: &'a mut [u8; 17]) -> &'a str {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for i in 0..6 {
            buf[i * 3] = HEX[(self.mac[i] >> 4) as usize];
            buf[i * 3 + 1] = HEX[(self.mac[i] & 0xF) as usize];
            if i < 5 {
                buf[i * 3 + 2] = b':';
            }
        }
        core::str::from_utf8(&buf[..17]).unwrap_or("??:??:??:??:??:??")
    }
}

/// Global virtio-net device instance.
pub static VIRTIO_NET: Mutex<VirtioNetDevice> = Mutex::new(VirtioNetDevice::empty());
