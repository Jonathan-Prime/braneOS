// ============================================================
// Brane OS Kernel — Brane Session Protocol (TCP)
// ============================================================
//
// Implements the Brane Protocol v2 state machine over TCP.
// Negotiates X25519 Ephemeral keys and establishes a
// ChaCha20Poly1305 encrypted E2E tunnel for Capabilities.
//
// Spec reference: ARCHITECTURE.md §6.2
// ============================================================

#![allow(dead_code)]

extern crate alloc;

use crate::crypto::{EphemeralKey, SessionCrypto};
use alloc::string::String;
use alloc::vec::Vec;

// -----------------------------------------------------------------------
// Error Types
// -----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    InvalidState,
    InvalidPayloadSize,
    DecryptionFailed,
    SerializationError,
    CryptoEngineNotReady,
    UnexpectedPacket,
}

impl From<SessionError> for &'static str {
    fn from(err: SessionError) -> Self {
        match err {
            SessionError::InvalidState => "Invalid session state",
            SessionError::InvalidPayloadSize => "Invalid payload size",
            SessionError::DecryptionFailed => "Decryption failed",
            SessionError::SerializationError => "Serialization error",
            SessionError::CryptoEngineNotReady => "Crypto engine not ready",
            SessionError::UnexpectedPacket => "Unexpected packet type",
        }
    }
}

// -----------------------------------------------------------------------
// Capability Negotiation
// -----------------------------------------------------------------------

/// Represents a single capability offer.
#[derive(Debug, Clone)]
pub struct CapabilityOffer {
    pub name: String,
    pub permissions_bits: u32,
    pub risk_level: u8,  // 0=Low, 1=Medium, 2=High, 3=Critical
}

impl CapabilityOffer {
    fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        
        // Name length (1 byte) + name
        buf.push(self.name.len() as u8);
        buf.extend_from_slice(self.name.as_bytes());
        
        // Permissions (4 bytes)
        buf.extend_from_slice(&self.permissions_bits.to_le_bytes());
        
        // Risk level (1 byte)
        buf.push(self.risk_level);
        
        buf
    }

    fn from_bytes(data: &[u8], offset: &mut usize) -> Option<Self> {
        if *offset >= data.len() {
            return None;
        }

        // Read name length
        let name_len = data[*offset] as usize;
        *offset += 1;

        if *offset + name_len > data.len() {
            return None;
        }

        let name = String::from_utf8_lossy(&data[*offset..*offset + name_len]).into_owned();
        *offset += name_len;

        if *offset + 4 + 1 > data.len() {
            return None;
        }

        let mut perm_bytes = [0u8; 4];
        perm_bytes.copy_from_slice(&data[*offset..*offset + 4]);
        let permissions_bits = u32::from_le_bytes(perm_bytes);
        *offset += 4;

        let risk_level = data[*offset];
        *offset += 1;

        Some(Self {
            name,
            permissions_bits,
            risk_level,
        })
    }
}

/// Represents a negotiation of capabilities between two branes.
#[derive(Debug, Clone)]
pub struct CapabilityNegotiation {
    /// List of offered capabilities (server side)
    pub offered: Vec<CapabilityOffer>,
    /// List of required capabilities (client side)
    pub required: Vec<String>,
    /// Node ID of the offering brane (16 bytes)
    pub node_id: [u8; 16],
    /// Timestamp of the negotiation
    pub timestamp: u64,
}

impl CapabilityNegotiation {
    pub fn new(node_id: [u8; 16], timestamp: u64) -> Self {
        Self {
            offered: Vec::new(),
            required: Vec::new(),
            node_id,
            timestamp,
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // Node ID (16 bytes)
        buf.extend_from_slice(&self.node_id);

        // Timestamp (8 bytes)
        buf.extend_from_slice(&self.timestamp.to_le_bytes());

        // Number of offered caps (1 byte)
        buf.push(self.offered.len() as u8);

        // Offered capabilities
        for offer in &self.offered {
            buf.extend_from_slice(&offer.to_bytes());
        }

        // Number of required caps (1 byte)
        buf.push(self.required.len() as u8);

        // Required capability names
        for req in &self.required {
            buf.push(req.len() as u8);
            buf.extend_from_slice(req.as_bytes());
        }

        buf
    }

    fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 25 {
            return None; // At least node_id + timestamp + counts
        }

        let mut offset;

        // Parse node_id
        let mut node_id = [0u8; 16];
        node_id.copy_from_slice(&data[0..16]);
        offset = 16;

        // Parse timestamp
        let mut ts_bytes = [0u8; 8];
        ts_bytes.copy_from_slice(&data[offset..offset + 8]);
        let timestamp = u64::from_le_bytes(ts_bytes);
        offset += 8;

        // Parse offered count
        let offered_count = data[offset] as usize;
        offset += 1;

        let mut offered = Vec::new();
        for _ in 0..offered_count {
            if let Some(offer) = CapabilityOffer::from_bytes(data, &mut offset) {
                offered.push(offer);
            } else {
                return None;
            }
        }

        if offset >= data.len() {
            return None;
        }

        // Parse required count
        let required_count = data[offset] as usize;
        offset += 1;

        let mut required = Vec::new();
        for _ in 0..required_count {
            if offset >= data.len() {
                return None;
            }

            let name_len = data[offset] as usize;
            offset += 1;

            if offset + name_len > data.len() {
                return None;
            }

            let name = String::from_utf8_lossy(&data[offset..offset + name_len]).into_owned();
            offset += name_len;
            required.push(name);
        }

        Some(Self {
            offered,
            required,
            node_id,
            timestamp,
        })
    }
}

/// Packet types for the Brane Session Protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionPacketType {
    HandshakeInit = 1,
    HandshakeResponse = 2,
    CapabilityExchange = 3,
    EncryptedData = 4,
    Alert = 5,
    Disconnect = 6,
}

impl TryFrom<u8> for SessionPacketType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::HandshakeInit),
            2 => Ok(Self::HandshakeResponse),
            3 => Ok(Self::CapabilityExchange),
            4 => Ok(Self::EncryptedData),
            5 => Ok(Self::Alert),
            6 => Ok(Self::Disconnect),
            _ => Err(()),
        }
    }
}

/// A serialized Brane Session Packet (Binary format).
/// Header is 4 bytes:
///   [0]   = Packet Type
///   [1]   = Reserved (0)
///   [2-3] = Payload Length (Little Endian u16)
pub struct SessionPacket {
    pub ptype: SessionPacketType,
    pub payload: Vec<u8>,
}

impl SessionPacket {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + self.payload.len());
        buf.push(self.ptype as u8);
        buf.push(0); // Reserved
        let len_bytes = (self.payload.len() as u16).to_le_bytes();
        buf.extend_from_slice(&len_bytes);
        buf.extend_from_slice(&self.payload);
        buf
    }

    pub fn parse(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 4 {
            return None;
        }
        let ptype = SessionPacketType::try_from(data[0]).ok()?;
        let mut len_bytes = [0u8; 2];
        len_bytes.copy_from_slice(&data[2..4]);
        let payload_len = u16::from_le_bytes(len_bytes) as usize;

        if data.len() < 4 + payload_len {
            return None;
        }

        let payload = data[4..4 + payload_len].to_vec();
        Some((Self { ptype, payload }, 4 + payload_len))
    }
}

// -----------------------------------------------------------------------
// State Machine
// -----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Init,
    WaitResponse,
    WaitCapability,
    Established,
    Closed,
}

/// A secure Brane Session running over a TCP socket.
pub struct BraneSession {
    pub socket_id: u32,
    pub state: SessionState,
    pub remote_node_id: Option<[u8; 16]>,

    // Crypto state
    ephemeral_key: EphemeralKey,
    crypto_engine: Option<SessionCrypto>,

    // Capability negotiation state
    local_capabilities: Option<CapabilityNegotiation>,
    remote_capabilities: Option<CapabilityNegotiation>,

    // Nonce tracking (12 bytes)
    tx_nonce: u64,
    rx_nonce: u64,

    // Session metadata
    established_at: u64,
}

impl BraneSession {
    pub fn new(socket_id: u32) -> Self {
        Self {
            socket_id,
            state: SessionState::Init,
            remote_node_id: None,
            ephemeral_key: EphemeralKey::generate(),
            crypto_engine: None,
            local_capabilities: None,
            remote_capabilities: None,
            tx_nonce: 0,
            rx_nonce: 0,
            established_at: 0,
        }
    }

    /// Build the `HandshakeInit` packet containing our X25519 public key.
    /// This is the first step: client sends ephemeral public key to server.
    pub fn build_handshake_init(&mut self) -> SessionPacket {
        crate::serial_println!("[session] Building HandshakeInit packet...");
        
        let mut payload = Vec::new();
        // Send 32-byte X25519 public key
        payload.extend_from_slice(&self.ephemeral_key.public.to_bytes());

        self.state = SessionState::WaitResponse;

        crate::serial_println!("[session] HandshakeInit ready, state -> WaitResponse");

        SessionPacket {
            ptype: SessionPacketType::HandshakeInit,
            payload,
        }
    }

    /// Process an incoming `HandshakeResponse`.
    /// This completes the X25519 key exchange and derives a shared session key.
    /// The server responds with its own ephemeral public key, which we use to compute DH.
    pub fn process_handshake_response(&mut self, payload: &[u8]) -> Result<(), SessionError> {
        crate::serial_println!(
            "[session] Processing HandshakeResponse (len={})",
            payload.len()
        );

        if self.state != SessionState::WaitResponse {
            return Err(SessionError::InvalidState);
        }
        if payload.len() < 32 {
            return Err(SessionError::InvalidPayloadSize);
        }

        // Extract peer's 32-byte X25519 public key
        let mut peer_pub_bytes = [0u8; 32];
        peer_pub_bytes.copy_from_slice(&payload[..32]);

        // In salty, PublicKey::from handles [u8; 32] for X25519 when using agreement
        let peer_pub = salty::agreement::PublicKey::from(peer_pub_bytes);

        // ---------------------------------------------------------------
        // KEY DERIVATION: Diffie-Hellman
        // ---------------------------------------------------------------
        // Derive shared secret using X25519 (32 bytes)
        let shared_secret = self.ephemeral_key.diffie_hellman(&peer_pub);

        crate::serial_println!(
            "[session] DH complete, shared secret: {:02x?}... (first 8 bytes)",
            &shared_secret[..8]
        );

        // Initialize AEAD crypto engine with the shared secret
        // This will be used for all subsequent encrypted communication
        self.crypto_engine = Some(SessionCrypto::new(&shared_secret));

        // Reset nonce counters for fresh encryption state
        self.tx_nonce = 0;
        self.rx_nonce = 0;

        // Advance state machine
        self.state = SessionState::WaitCapability;

        crate::serial_println!(
            "[session] Handshake complete! AEAD engine initialized, state -> WaitCapability"
        );

        Ok(())
    }

    /// Build a `CapabilityExchange` packet from our local capabilities.
    /// This occurs after the handshake and proposes capabilities to the peer.
    pub fn build_capability_exchange(
        &mut self,
        our_node_id: [u8; 16],
    ) -> Result<SessionPacket, SessionError> {
        if self.state != SessionState::WaitCapability {
            return Err(SessionError::InvalidState);
        }

        crate::serial_println!("[session] Building CapabilityExchange packet...");

        let timestamp = crate::get_time_millis();
        let mut neg = CapabilityNegotiation::new(our_node_id, timestamp);

        // Offer default capabilities (server-side perspective)
        neg.offered.push(CapabilityOffer {
            name: "BRANE_CONNECT".into(),
            permissions_bits: (1u32 << 7),  // BRANE_CONNECT = bit 7
            risk_level: 1,  // Medium
        });
        neg.offered.push(CapabilityOffer {
            name: "IPC_SEND".into(),
            permissions_bits: (1u32 << 5),  // IPC_SEND = bit 5
            risk_level: 0,  // Low
        });
        neg.offered.push(CapabilityOffer {
            name: "IPC_RECV".into(),
            permissions_bits: (1u32 << 6),  // IPC_RECV = bit 6
            risk_level: 0,  // Low
        });

        crate::serial_println!(
            "[session] Offered {} capabilities to peer",
            neg.offered.len()
        );

        self.local_capabilities = Some(neg.clone());

        let packet = SessionPacket {
            ptype: SessionPacketType::CapabilityExchange,
            payload: neg.to_bytes(),
        };

        Ok(packet)
    }

    /// Process an incoming `CapabilityExchange` packet.
    /// This establishes the agreed-upon capabilities for the session.
    pub fn process_capability_exchange(&mut self, payload: &[u8]) -> Result<(), SessionError> {
        if self.state != SessionState::WaitCapability {
            return Err(SessionError::InvalidState);
        }

        crate::serial_println!(
            "[session] Processing CapabilityExchange (len={})",
            payload.len()
        );

        let neg = CapabilityNegotiation::from_bytes(payload)
            .ok_or(SessionError::SerializationError)?;

        self.remote_node_id = Some(neg.node_id);

        crate::serial_println!(
            "[session] Processed {} capabilities from peer (node: {:02x?})",
            neg.offered.len(),
            &neg.node_id[..4]
        );

        // Validate that peer's offered capabilities are acceptable
        // (In production, this would check against a policy engine)
        for cap in &neg.offered {
            crate::serial_println!(
                "[session]   - Capability: {} (perms=0x{:02x}, risk={})",
                cap.name,
                cap.permissions_bits,
                cap.risk_level
            );
        }

        self.remote_capabilities = Some(neg);

        // Mark session as established and ready for encrypted data
        self.state = SessionState::Established;
        self.established_at = crate::get_time_millis();

        crate::serial_println!(
            "[session] Session established! (peer: {:02x?})",
            &self.remote_node_id.unwrap()[..4]
        );

        Ok(())
    }

    /// Helper to increment and format the 12-byte nonce for ChaCha20.
    fn get_tx_nonce(&mut self) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        let bytes = self.tx_nonce.to_le_bytes();
        nonce[..8].copy_from_slice(&bytes);
        self.tx_nonce += 1;
        nonce
    }

    /// Helper to get the next RX nonce.
    fn get_rx_nonce(&mut self) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        let bytes = self.rx_nonce.to_le_bytes();
        nonce[..8].copy_from_slice(&bytes);
        self.rx_nonce += 1;
        nonce
    }

    /// Encrypt an application-level payload into an EncryptedData packet.
    /// This is the primary method for sending secure data over the Brane session.
    pub fn encrypt_payload(&mut self, data: &[u8]) -> Result<SessionPacket, SessionError> {
        if self.state != SessionState::Established {
            return Err(SessionError::InvalidState);
        }

        // Get nonce BEFORE borrowing crypto_engine to avoid borrow conflict
        let nonce = self.get_tx_nonce();

        let engine = self.crypto_engine.as_ref()
            .ok_or(SessionError::CryptoEngineNotReady)?;

        let ciphertext = engine.encrypt(&nonce, data)
            .ok_or(SessionError::DecryptionFailed)?;

        crate::serial_println!(
            "[session] Encrypted {} bytes (nonce={})",
            data.len(),
            self.tx_nonce - 1
        );

        Ok(SessionPacket {
            ptype: SessionPacketType::EncryptedData,
            payload: ciphertext,
        })
    }

    /// Decrypt a received EncryptedData packet back to plaintext.
    /// This is the primary method for receiving secure data over the Brane session.
    pub fn decrypt_payload(&mut self, data: &[u8]) -> Result<Vec<u8>, SessionError> {
        if self.state != SessionState::Established {
            return Err(SessionError::InvalidState);
        }

        // Get nonce BEFORE borrowing crypto_engine to avoid borrow conflict
        let nonce = self.get_rx_nonce();

        let engine = self.crypto_engine.as_ref()
            .ok_or(SessionError::CryptoEngineNotReady)?;

        let plaintext = engine.decrypt(&nonce, data)
            .ok_or(SessionError::DecryptionFailed)?;

        crate::serial_println!(
            "[session] Decrypted {} bytes (nonce={})",
            plaintext.len(),
            self.rx_nonce - 1
        );

        Ok(plaintext)
    }

    /// Close the session and send a disconnect packet.
    pub fn close(&mut self) -> SessionPacket {
        self.state = SessionState::Closed;
        SessionPacket {
            ptype: SessionPacketType::Disconnect,
            payload: Vec::new(),
        }
    }

    /// Check if the session is established and ready for encrypted communication.
    pub fn is_established(&self) -> bool {
        self.state == SessionState::Established && self.crypto_engine.is_some()
    }

    /// Get a summary of the session state.
    pub fn summary(&self) -> (u32, SessionState, bool) {
        (self.socket_id, self.state.clone(), self.is_established())
    }
}

// -----------------------------------------------------------------------
// Unit Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_packet_serialization() {
        let pkt = SessionPacket {
            ptype: SessionPacketType::HandshakeInit,
            payload: alloc::vec![1, 2, 3, 4, 5],
        };

        let bytes = pkt.to_bytes();
        assert_eq!(bytes[0], 1); // HandshakeInit = 1
        assert_eq!(bytes[1], 0); // Reserved
        assert_eq!(bytes[2], 5); // Length (little-endian)
        assert_eq!(bytes[3], 0);
        assert_eq!(&bytes[4..], &[1, 2, 3, 4, 5]);

        // Parse it back
        let (parsed, len) = SessionPacket::parse(&bytes).expect("Failed to parse");
        assert_eq!(parsed.ptype, SessionPacketType::HandshakeInit);
        assert_eq!(parsed.payload, alloc::vec![1, 2, 3, 4, 5]);
        assert_eq!(len, 9);
    }

    #[test]
    fn test_capability_offer_serialization() {
        let offer = CapabilityOffer {
            name: "TEST_CAP".into(),
            permissions_bits: 0x12345678,
            risk_level: 2,
        };

        let bytes = offer.to_bytes();
        let mut offset = 0;

        let parsed = CapabilityOffer::from_bytes(&bytes, &mut offset).expect("Failed to parse");
        assert_eq!(parsed.name, "TEST_CAP");
        assert_eq!(parsed.permissions_bits, 0x12345678);
        assert_eq!(parsed.risk_level, 2);
    }

    #[test]
    fn test_capability_negotiation_serialization() {
        let mut neg = CapabilityNegotiation::new([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16], 1234567890);

        neg.offered.push(CapabilityOffer {
            name: "CAP1".into(),
            permissions_bits: 0x01,
            risk_level: 0,
        });
        neg.required.push("CAP2".into());

        let bytes = neg.to_bytes();
        let parsed = CapabilityNegotiation::from_bytes(&bytes).expect("Failed to parse");

        assert_eq!(parsed.node_id, neg.node_id);
        assert_eq!(parsed.timestamp, neg.timestamp);
        assert_eq!(parsed.offered.len(), 1);
        assert_eq!(parsed.offered[0].name, "CAP1");
        assert_eq!(parsed.required.len(), 1);
        assert_eq!(parsed.required[0], "CAP2");
    }

    #[test]
    fn test_session_state_machine() {
        let mut session = BraneSession::new(42);

        // Initial state should be Init
        assert_eq!(session.state, SessionState::Init);
        assert!(!session.is_established());

        // Build handshake
        let pkt = session.build_handshake_init();
        assert_eq!(pkt.ptype, SessionPacketType::HandshakeInit);
        assert_eq!(session.state, SessionState::WaitResponse);

        // Simulate receiving response (would normally come from peer)
        // For testing, we create another session and use its public key
        let mut peer_session = BraneSession::new(43);
        let peer_handshake = peer_session.build_handshake_init();

        // Client processes peer's handshake response
        let response_result = session.process_handshake_response(&peer_handshake.payload);
        assert!(response_result.is_ok());
        assert_eq!(session.state, SessionState::WaitCapability);
    }

    #[test]
    fn test_session_established_after_capability_exchange() {
        let mut session = BraneSession::new(42);

        // Step 1: Session builds HandshakeInit (Init → WaitResponse)
        let _init_pkt = session.build_handshake_init();

        // Step 2: Peer creates its handshake and we process it as a response
        let mut peer_session = BraneSession::new(43);
        let peer_handshake = peer_session.build_handshake_init();
        let _ = session.process_handshake_response(&peer_handshake.payload);

        // Now capability exchange
        let node_id = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
        ];
        let _cap_pkt = session
            .build_capability_exchange(node_id)
            .expect("Failed to build cap exchange");

        // Simulate peer's capability exchange
        let mut peer_neg = CapabilityNegotiation::new(node_id, 0);
        peer_neg.offered.push(CapabilityOffer {
            name: "TEST_CAP".into(),
            permissions_bits: 0x01,
            risk_level: 0,
        });

        let _ = session.process_capability_exchange(&peer_neg.to_bytes());

        // Session should now be established
        assert_eq!(session.state, SessionState::Established);
    }

    #[test]
    fn test_nonce_generation() {
        let mut session = BraneSession::new(1);
        session.tx_nonce = 0;
        session.rx_nonce = 0;

        let nonce1 = session.get_tx_nonce();
        let nonce2 = session.get_tx_nonce();

        // Nonces should be different
        assert_ne!(nonce1, nonce2);
        // tx_nonce should have incremented
        assert_eq!(session.tx_nonce, 2);
    }

    #[test]
    fn test_session_close() {
        let mut session = BraneSession::new(1);
        session.state = SessionState::Established;

        let close_pkt = session.close();
        assert_eq!(close_pkt.ptype, SessionPacketType::Disconnect);
        assert_eq!(session.state, SessionState::Closed);
    }

    #[test]
    fn test_session_summary() {
        let mut session = BraneSession::new(99);
        session.state = SessionState::WaitCapability;

        let (id, state, is_est) = session.summary();
        assert_eq!(id, 99);
        assert_eq!(state, SessionState::WaitCapability);
        assert!(!is_est);

        session.state = SessionState::Established;
        session.crypto_engine = Some(SessionCrypto::new(&[0u8; 32]));

        let (id, state, is_est) = session.summary();
        assert_eq!(id, 99);
        assert_eq!(state, SessionState::Established);
        assert!(is_est);
    }

    #[test]
    fn test_encryption_in_established_state() {
        let mut session = BraneSession::new(1);
        session.state = SessionState::Established;
        session.crypto_engine = Some(SessionCrypto::new(&[0u8; 32]));

        let plaintext = b"Hello, Brane!";
        let result = session.encrypt_payload(plaintext);
        assert!(result.is_ok());

        let encrypted_pkt = result.unwrap();
        assert_eq!(encrypted_pkt.ptype, SessionPacketType::EncryptedData);
    }

    #[test]
    fn test_encryption_in_wrong_state() {
        let mut session = BraneSession::new(1);
        session.state = SessionState::Init; // Not established

        let plaintext = b"Hello, Brane!";
        let result = session.encrypt_payload(plaintext);
        assert!(result.is_err());
        assert_eq!(result.err(), Some(SessionError::InvalidState));
    }

    #[test]
    fn test_decryption_in_established_state() {
        let mut session = BraneSession::new(1);
        session.state = SessionState::Established;
        session.crypto_engine = Some(SessionCrypto::new(&[0u8; 32]));

        let plaintext = b"Test message for decryption";

        // First encrypt it
        let encrypted_pkt = session
            .encrypt_payload(plaintext)
            .expect("Encryption failed");

        // Now try to decrypt (with a fresh nonce counter for RX)
        let result = session.decrypt_payload(&encrypted_pkt.payload);
        assert!(result.is_ok());

        let decrypted = result.unwrap();
        assert_eq!(decrypted, plaintext);
    }
}
