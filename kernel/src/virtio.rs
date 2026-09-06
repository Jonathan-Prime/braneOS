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

pub use crate::pci::PciDevice;

/// Virtio vendor/device IDs
const VIRTIO_VENDOR_ID: u16 = 0x1AF4;
const VIRTIO_NET_TRANSITIONAL_DEVICE_ID: u16 = 0x1000;
const VIRTIO_NET_MODERN_DEVICE_ID: u16 = 0x1041;
const VIRTIO_BLOCK_TRANSITIONAL_DEVICE_ID: u16 = 0x1001;
const VIRTIO_BLOCK_MODERN_DEVICE_ID: u16 = 0x1042;
const VIRTIO_NET_SUBSYSTEM: u16 = 1;
const VIRTIO_BLOCK_SUBSYSTEM: u16 = 2;

/// Find a virtio-net function in the shared PCI inventory.
pub fn find_virtio_net() -> Option<PciDevice> {
    crate::pci::find_device(|device| {
        device.vendor_id == VIRTIO_VENDOR_ID
            && (device.device_id == VIRTIO_NET_MODERN_DEVICE_ID
                || (device.device_id == VIRTIO_NET_TRANSITIONAL_DEVICE_ID
                    && device.subsystem_id == VIRTIO_NET_SUBSYSTEM))
    })
}

/// Find a virtio block controller in the shared PCI inventory.
pub fn find_virtio_block() -> Option<PciDevice> {
    crate::pci::find_device(|device| {
        device.vendor_id == VIRTIO_VENDOR_ID
            && (device.device_id == VIRTIO_BLOCK_MODERN_DEVICE_ID
                || (device.device_id == VIRTIO_BLOCK_TRANSITIONAL_DEVICE_ID
                    && device.subsystem_id == VIRTIO_BLOCK_SUBSYSTEM))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtioInitError {
    MissingLegacyIoBar,
    IoBaseOutOfRange,
}

/// Return BAR0 as a legacy virtio I/O-port base.
pub fn legacy_io_base(device: PciDevice) -> Result<u16, VirtioInitError> {
    let Some(crate::pci::PciBar::Io { base }) = device.bar(0) else {
        return Err(VirtioInitError::MissingLegacyIoBar);
    };
    u16::try_from(base).map_err(|_| VirtioInitError::IoBaseOutOfRange)
}

// -----------------------------------------------------------------------
// Virtio Legacy I/O Port Registers (virtio 0.9 / transitional)
// -----------------------------------------------------------------------

/// Offsets from BAR0 for virtio legacy PCI device
pub(crate) mod virtio_reg {
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
pub(crate) mod status {
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
            pci: PciDevice::EMPTY,
            mac: [0; 6],
            io_base: 0,
            initialized: false,
        }
    }

    /// Initialize the virtio-net device via legacy PCI I/O ports.
    pub fn init(&mut self, pci: PciDevice) -> Result<(), VirtioInitError> {
        self.pci = pci;
        self.io_base = legacy_io_base(pci)?;

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
        Ok(())
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
