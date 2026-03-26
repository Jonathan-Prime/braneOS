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
use alloc::vec::Vec;

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

#[derive(Debug, PartialEq, Eq)]
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

    // Crypto state
    ephemeral_key: EphemeralKey,
    crypto_engine: Option<SessionCrypto>,

    // Nonce tracking (12 bytes)
    tx_nonce: u64,
    rx_nonce: u64,
}

impl BraneSession {
    pub fn new(socket_id: u32) -> Self {
        Self {
            socket_id,
            state: SessionState::Init,
            ephemeral_key: EphemeralKey::generate(),
            crypto_engine: None,
            tx_nonce: 0,
            rx_nonce: 0,
        }
    }

    /// Build the `HandshakeInit` packet containing our X25519 public key.
    pub fn build_handshake_init(&mut self) -> SessionPacket {
        let mut payload = Vec::new();
        // Send 32-byte public key
        payload.extend_from_slice(&self.ephemeral_key.public.to_bytes());

        self.state = SessionState::WaitResponse;

        SessionPacket {
            ptype: SessionPacketType::HandshakeInit,
            payload,
        }
    }

    /// Process an incoming `HandshakeResponse`.
    pub fn process_handshake_response(&mut self, payload: &[u8]) -> Result<(), &'static str> {
        if self.state != SessionState::WaitResponse {
            return Err("Unexpected handshake response");
        }
        if payload.len() < 32 {
            return Err("Invalid handshake response payload size");
        }

        // Extract peer's public key
        let mut peer_pub_bytes = [0u8; 32];
        peer_pub_bytes.copy_from_slice(&payload[..32]);

        // In salty, PublicKey::from handles [u8; 32] for X25519 when using agreement
        let peer_pub = salty::agreement::PublicKey::from(peer_pub_bytes);

        // Derive shared secret
        let shared_secret = self.ephemeral_key.diffie_hellman(&peer_pub);

        // Initialize AEAD crypto engine
        self.crypto_engine = Some(SessionCrypto::new(&shared_secret));

        // Advance state
        self.state = SessionState::WaitCapability;

        crate::serial_println!("[session] Handshake complete, AEAD established");

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

    /// Encrypt an application-level payload into an EncryptedData packet.
    pub fn encrypt_payload(&mut self, data: &[u8]) -> Option<SessionPacket> {
        if self.state != SessionState::Established {
            return None;
        }

        let nonce = self.get_tx_nonce();
        let engine = self.crypto_engine.as_ref()?;

        let ciphertext = engine.encrypt(&nonce, data)?;

        Some(SessionPacket {
            ptype: SessionPacketType::EncryptedData,
            payload: ciphertext,
        })
    }
}
