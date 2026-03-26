// ============================================================
// Brane OS Kernel — Brane Discovery Protocol (UDP)
// ============================================================
//
// Implements the Brane Discovery Protocol (BDP) over UDP broadcast
// on port 9000. This replaces the old simulated timing discovery.
//
// Spec reference: ARCHITECTURE.md §6.1
// ============================================================

extern crate alloc;

use crate::socket::{self, Protocol, SocketError};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Discovery UDP port.
pub const DISCOVERY_PORT: u16 = 9000;

/// BDP Packet Type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    Announce,
    Discover,
}

impl PacketType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Announce => "ANNOUNCE",
            Self::Discover => "DISCOVER",
        }
    }
}

/// A parsed discovery packet.
pub struct DiscoveryPacket {
    pub ptype: PacketType,
    pub node_id: String,
    pub name: String,
    pub capabilities: String,
}

impl DiscoveryPacket {
    /// Format the packet as a simple ascii string (e.g. "ANNOUNCE node123 my-pc Cap1,Cap2").
    pub fn to_bytes(&self) -> Vec<u8> {
        let s = format!(
            "{} {} {} {}",
            self.ptype.as_str(),
            self.node_id,
            self.name,
            self.capabilities
        );
        s.into_bytes()
    }

    /// Parse a packet from bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let s = core::str::from_utf8(data).ok()?;
        let mut parts = s.splitn(4, ' ');

        let p_type = match parts.next()? {
            "ANNOUNCE" => PacketType::Announce,
            "DISCOVER" => PacketType::Discover,
            _ => return None,
        };

        let node_id = parts.next()?.into();
        let name = parts.next()?.into();
        let capabilities = parts.next().unwrap_or("").into();

        Some(Self {
            ptype: p_type,
            node_id,
            name,
            capabilities,
        })
    }
}

// -----------------------------------------------------------------------
// Discovery Subsystem
// -----------------------------------------------------------------------

/// State for the discovery subsystem
pub struct DiscoverySubsystem {
    udp_socket_id: Option<u32>,
    pub node_id_hex: String,
    pub node_name: String,
}

impl Default for DiscoverySubsystem {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscoverySubsystem {
    pub const fn new() -> Self {
        Self {
            udp_socket_id: None,
            node_id_hex: String::new(),
            node_name: String::new(),
        }
    }

    /// Initialize the discovery subsystem.
    pub fn init(&mut self, node_id_hex: String, node_name: String) -> Result<(), SocketError> {
        self.node_id_hex = node_id_hex;
        self.node_name = node_name;

        // Create a UDP socket (PID 1 = kernel)
        let mut table = socket::SOCKET_TABLE.lock();
        let id = table.create(Protocol::Udp, 1)?;

        // Bind to port 9000
        table.bind(id, [0, 0, 0, 0], DISCOVERY_PORT)?;
        self.udp_socket_id = Some(id);

        Ok(())
    }

    /// Broadcast an ANNOUNCE packet to the network.
    pub fn broadcast_announce(&self) -> Result<(), SocketError> {
        let id = self.udp_socket_id.ok_or(SocketError::InvalidState)?;

        let packet = DiscoveryPacket {
            ptype: PacketType::Announce,
            node_id: self.node_id_hex.clone(),
            name: self.node_name.clone(),
            capabilities: String::from("Core,Session,Network"),
        };

        let data = packet.to_bytes();
        let mut table = socket::SOCKET_TABLE.lock();

        // 255.255.255.255 broadcast port 9000
        table.send_to(id, &data, [255, 255, 255, 255], DISCOVERY_PORT)
    }

    /// Poll the UDP socket for incoming discovery packets.
    pub fn poll(&self) {
        let id = match self.udp_socket_id {
            Some(i) => i,
            None => return,
        };

        let mut buf = [0u8; 1024];
        let mut table = socket::SOCKET_TABLE.lock();

        if let Ok((len, remote)) = table.recv_from(id, &mut buf) {
            if let Some(packet) = DiscoveryPacket::parse(&buf[..len]) {
                // Ignore our own packets just in case loopback catching
                if packet.node_id == self.node_id_hex {
                    return;
                }
                crate::serial_println!(
                    "[bdp] Found peer '{}' at {}.{}.{}.{}",
                    packet.name,
                    remote.addr[0],
                    remote.addr[1],
                    remote.addr[2],
                    remote.addr[3]
                );

                // Add peer to the Brane manager
                let _ = crate::brane::BRANE.lock().register_discovered(
                    packet.name.as_str(),
                    crate::brane::BraneType::Peer,
                    crate::brane::Transport::TcpIp,
                    0,
                    100,
                );
            }
        }
    }
}

pub static DISCOVERY: spin::Mutex<DiscoverySubsystem> = spin::Mutex::new(DiscoverySubsystem::new());
