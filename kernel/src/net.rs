// ============================================================
// Brane OS Kernel — Network Stack
// ============================================================
//
// Network interface management using smoltcp.
// Provides a TCP/IP stack over virtio-net for QEMU.
//
// Features:
//   - Static IPv4 configuration (10.0.2.15/24)
//   - smoltcp Interface for Ethernet + ARP + IPv4
//   - Packet TX/RX abstraction
//
// Reference: smoltcp documentation, ARCHITECTURE.md §5.4
// ============================================================

#![allow(dead_code)]

use spin::Mutex;

use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address};

// -----------------------------------------------------------------------
// Network Configuration
// -----------------------------------------------------------------------

/// Default static IP for QEMU user-mode networking.
pub const DEFAULT_IP: [u8; 4] = [10, 0, 2, 15];
pub const DEFAULT_GATEWAY: [u8; 4] = [10, 0, 2, 2];
pub const DEFAULT_PREFIX: u8 = 24;

// -----------------------------------------------------------------------
// Network Interface State
// -----------------------------------------------------------------------

/// Summary of network interface state.
#[derive(Debug, Clone)]
pub struct NetInfo {
    pub mac: [u8; 6],
    pub ip: [u8; 4],
    pub gateway: [u8; 4],
    pub prefix: u8,
    pub link_up: bool,
    pub packets_tx: u64,
    pub packets_rx: u64,
}

/// Global network state.
pub struct NetStack {
    pub info: NetInfo,
    pub initialized: bool,
}

impl NetStack {
    const fn new() -> Self {
        Self {
            info: NetInfo {
                mac: [0; 6],
                ip: DEFAULT_IP,
                gateway: DEFAULT_GATEWAY,
                prefix: DEFAULT_PREFIX,
                link_up: false,
                packets_tx: 0,
                packets_rx: 0,
            },
            initialized: false,
        }
    }

    /// Initialize the network stack with the given MAC address.
    pub fn init(&mut self, mac: [u8; 6]) {
        self.info.mac = mac;
        self.info.ip = DEFAULT_IP;
        self.info.gateway = DEFAULT_GATEWAY;
        self.info.prefix = DEFAULT_PREFIX;
        self.info.link_up = true;
        self.info.packets_tx = 0;
        self.info.packets_rx = 0;
        self.initialized = true;
    }

    /// Get the smoltcp-compatible Ethernet address.
    pub fn ethernet_addr(&self) -> EthernetAddress {
        EthernetAddress(self.info.mac)
    }

    /// Get the IP CIDR.
    pub fn ip_cidr(&self) -> IpCidr {
        IpCidr::new(
            IpAddress::Ipv4(Ipv4Address::from_bytes(&self.info.ip)),
            self.info.prefix,
        )
    }

    /// Get the gateway address.
    pub fn gateway_addr(&self) -> Ipv4Address {
        Ipv4Address::from_bytes(&self.info.gateway)
    }

    /// Record a transmitted packet.
    pub fn record_tx(&mut self) {
        self.info.packets_tx += 1;
    }

    /// Record a received packet.
    pub fn record_rx(&mut self) {
        self.info.packets_rx += 1;
    }

    /// Format the IP address.
    pub fn ip_str(&self, buf: &mut [u8; 18]) -> usize {
        use core::fmt::Write;
        struct SliceBuf<'a> {
            buf: &'a mut [u8],
            pos: usize,
        }
        impl<'a> Write for SliceBuf<'a> {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                let bytes = s.as_bytes();
                let n = bytes.len().min(self.buf.len() - self.pos);
                self.buf[self.pos..self.pos + n].copy_from_slice(&bytes[..n]);
                self.pos += n;
                Ok(())
            }
        }
        let mut w = SliceBuf { buf, pos: 0 };
        let _ = write!(
            w,
            "{}.{}.{}.{}/{}",
            self.info.ip[0], self.info.ip[1], self.info.ip[2], self.info.ip[3], self.info.prefix
        );
        w.pos
    }
}

/// Global network stack.
pub static NET_STACK: Mutex<NetStack> = Mutex::new(NetStack::new());

/// Initialize the network subsystem.
///
/// Scans for a virtio-net device, reads the MAC, and sets up the stack.
pub fn init() -> bool {
    use crate::virtio;

    // Try to find a virtio-net device on PCI bus
    if let Some(pci_dev) = virtio::find_virtio_net() {
        crate::serial_println!(
            "[net]  virtio-net: found at PCI {:02x}:{:02x}.{} (bar0=0x{:X}, irq={})",
            pci_dev.bus,
            pci_dev.device,
            pci_dev.function,
            pci_dev.bar0,
            pci_dev.irq_line,
        );

        // Initialize the virtio device
        let mut vdev = virtio::VIRTIO_NET.lock();
        vdev.init(pci_dev);

        let mut mac_buf = [0u8; 17];
        let mac_str = vdev.mac_str(&mut mac_buf);
        crate::serial_println!("[net]  MAC address: {}", mac_str);

        let mac = vdev.mac;
        drop(vdev);

        // Initialize the network stack with the MAC
        let mut stack = NET_STACK.lock();
        stack.init(mac);

        let mut ip_buf = [0u8; 18];
        let ip_len = stack.ip_str(&mut ip_buf);
        let ip_str = core::str::from_utf8(&ip_buf[..ip_len]).unwrap_or("?");
        crate::serial_println!(
            "[net]  Network interface ready: {} (gateway {}.{}.{}.{})",
            ip_str,
            DEFAULT_GATEWAY[0],
            DEFAULT_GATEWAY[1],
            DEFAULT_GATEWAY[2],
            DEFAULT_GATEWAY[3],
        );

        true
    } else {
        crate::serial_println!("[net]  No virtio-net device found. Networking disabled.");
        false
    }
}
