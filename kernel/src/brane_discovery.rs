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
use crate::brane_session::BraneSession;
use alloc::collections::BTreeMap;
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
    // TCP sessions for Brane Protocol v2
    pub sessions: BTreeMap<u32, BraneSession>, // socket_id -> session
    pub node_id_bytes: [u8; 16],
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
            sessions: BTreeMap::new(),
            node_id_bytes: [0u8; 16],
        }
    }

    /// Initialize the discovery subsystem.
    pub fn init(&mut self, node_id_hex: String, node_name: String) -> Result<(), SocketError> {
        self.node_id_hex = node_id_hex.clone();
        self.node_name = node_name;

        // Convert hex string to 16-byte node ID
        self.parse_node_id_hex(&node_id_hex);

        // Create a UDP socket (PID 1 = kernel)
        let mut table = socket::SOCKET_TABLE.lock();
        let id = table.create(Protocol::Udp, 1)?;

        // Bind to port 9000
        table.bind(id, [0, 0, 0, 0], DISCOVERY_PORT)?;
        self.udp_socket_id = Some(id);

        Ok(())
    }

    /// Parse a hex string into a 16-byte node ID.
    fn parse_node_id_hex(&mut self, hex: &str) {
        // Simple parsing: take first 16 bytes of hex string as node ID
        let mut idx = 0;
        for (i, _c) in hex.chars().take(32).enumerate() {
            if i % 2 == 1 {
                if let Ok(byte) = u8::from_str_radix(&hex[i - 1..=i], 16) {
                    if idx < 16 {
                        self.node_id_bytes[idx] = byte;
                        idx += 1;
                    }
                }
            }
        }
        // Pad with zeros if necessary
        while idx < 16 {
            self.node_id_bytes[idx] = 0;
            idx += 1;
        }
    }

    /// Initiate a Brane Protocol v2 handshake with a peer over TCP.
    /// Called after discovering a peer via UDP.
    pub fn initiate_handshake(&mut self, peer_ip: [u8; 4], peer_port: u16) -> Result<u32, SocketError> {
        // Create a TCP socket
        let mut table = socket::SOCKET_TABLE.lock();
        let tcp_socket_id = table.create(Protocol::Tcp, 1)?;
        drop(table);

        // Create a new BraneSession for this TCP connection
        let mut session = BraneSession::new(tcp_socket_id);

        // Build the HandshakeInit packet
        let handshake_pkt = session.build_handshake_init();

        // Send it over TCP
        let mut table = socket::SOCKET_TABLE.lock();
        table.send_to(tcp_socket_id, &handshake_pkt.to_bytes(), peer_ip, peer_port)?;

        // Store the session
        self.sessions.insert(tcp_socket_id, session);

        crate::serial_println!(
            "[bdp] Initiated Brane handshake with {}.{}.{}.{}:{}",
            peer_ip[0],
            peer_ip[1],
            peer_ip[2],
            peer_ip[3],
            peer_port
        );

        Ok(tcp_socket_id)
    }

    /// Process incoming data on an active session.
    pub fn process_session_data(
        &mut self,
        socket_id: u32,
        data: &[u8],
    ) -> Result<(), &'static str> {
        if let Some(session) = self.sessions.get_mut(&socket_id) {
            crate::serial_println!(
                "[bdp] Processing {} bytes on session {} (state: {:?})",
                data.len(),
                socket_id,
                session.state
            );

            // Parse the incoming packet
            if let Some((pkt, _len)) = crate::brane_session::SessionPacket::parse(data) {
                match pkt.ptype {
                    crate::brane_session::SessionPacketType::HandshakeResponse => {
                        session.process_handshake_response(&pkt.payload)?;
                        Ok(())
                    }
                    crate::brane_session::SessionPacketType::CapabilityExchange => {
                        session.process_capability_exchange(&pkt.payload)?;
                        Ok(())
                    }
                    crate::brane_session::SessionPacketType::EncryptedData => {
                        // Decrypt and process application data
                        match session.decrypt_payload(&pkt.payload) {
                            Ok(plaintext) => {
                                crate::serial_println!(
                                    "[bdp] Decrypted {} bytes of application data",
                                    plaintext.len()
                                );
                                Ok(())
                            }
                            Err(_) => Err("Decryption failed"),
                        }
                    }
                    _ => Err("Unexpected packet type"),
                }
            } else {
                Err("Failed to parse packet")
            }
        } else {
            Err("Session not found")
        }
    }

    /// Build a CapabilityExchange packet for a session.
    pub fn send_capabilities(&mut self, socket_id: u32) -> Result<(), &'static str> {
        if let Some(session) = self.sessions.get_mut(&socket_id) {
            let pkt = session
                .build_capability_exchange(self.node_id_bytes)
                .ok()
                .ok_or("Failed to build capability exchange")?;

            // Send over TCP (using send_to with remote endpoint placeholder)
            let mut table = socket::SOCKET_TABLE.lock();
            let _ = table.send(socket_id, &pkt.to_bytes());

            crate::serial_println!("[bdp] Sent CapabilityExchange on socket {}", socket_id);

            Ok(())
        } else {
            Err("Session not found")
        }
    }

    /// Get the status of a session.
    pub fn session_status(&self, socket_id: u32) -> Option<(u32, String, bool)> {
        self.sessions.get(&socket_id).map(|s| {
            let (id, state, established) = s.summary();
            (id, format!("{:?}", state), established)
        })
    }

    /// List all active sessions.
    pub fn list_sessions(&self) -> Vec<(u32, bool)> {
        self.sessions
            .iter()
            .map(|(id, session)| (*id, session.is_established()))
            .collect()
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
