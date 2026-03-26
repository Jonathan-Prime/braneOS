// ============================================================
// Brane OS Kernel — Cryptography Subsystem
// ============================================================
//
// Provides hardware RNG (via RDRAND) and cryptographic primitives
// (X25519, Ed25519, ChaCha20Poly1305) for the Brane Protocol.
//
// Spec reference: ARCHITECTURE.md §5.3 (planned)
// ============================================================

extern crate alloc;

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand_core::{CryptoRng, RngCore};
use salty::agreement::{PublicKey as X25519PublicKey, SecretKey as X25519SecretKey};
use salty::constants::SECRETKEY_SEED_LENGTH;
use salty::Keypair;

// -----------------------------------------------------------------------
// Hardware RNG (x86_64 RDRAND)
// -----------------------------------------------------------------------

/// A Cryptographically Secure Pseudorandom Number Generator using x86 RDRAND.
pub struct HardwareRng;

impl Default for HardwareRng {
    fn default() -> Self {
        Self::new()
    }
}

impl HardwareRng {
    pub const fn new() -> Self {
        Self
    }
}

impl RngCore for HardwareRng {
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }

    fn next_u64(&mut self) -> u64 {
        let mut val: u64 = 0;
        unsafe {
            // Spin until RDRAND succeeds
            while core::arch::x86_64::_rdrand64_step(&mut val) != 1 {
                core::arch::x86_64::_mm_pause();
            }
        }
        val
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let mut i = 0;
        while i < dest.len() {
            let rnd = self.next_u64();
            let bytes = rnd.to_ne_bytes();
            let chunk = core::cmp::min(8, dest.len() - i);
            dest[i..i + chunk].copy_from_slice(&bytes[..chunk]);
            i += chunk;
        }
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for HardwareRng {}

// -----------------------------------------------------------------------
// Node Identity & Signatures (Ed25519)
// -----------------------------------------------------------------------

/// Generates a new Ed25519 keypair for the kernel node identity.
pub fn generate_node_identity() -> Keypair {
    let mut rng = HardwareRng::new();
    let mut seed = [0u8; SECRETKEY_SEED_LENGTH];
    rng.fill_bytes(&mut seed);
    Keypair::from(&seed)
}

// -----------------------------------------------------------------------
// Key Exchange (X25519 / Diffie-Hellman)
// -----------------------------------------------------------------------

/// An ephemeral X25519 keypair for session handshakes.
pub struct EphemeralKey {
    pub secret: X25519SecretKey,
    pub public: X25519PublicKey,
}

impl EphemeralKey {
    /// Generate a new ephemeral X25519 keypair.
    pub fn generate() -> Self {
        let mut rng = HardwareRng::new();
        let mut seed = [0u8; SECRETKEY_SEED_LENGTH];
        rng.fill_bytes(&mut seed);
        let secret = X25519SecretKey::from_seed(&seed);
        let public = secret.public();
        Self { secret, public }
    }

    /// Perform Diffie-Hellman key exchange with a peer's X25519 public key.
    /// Returns 32-byte shared secret.
    pub fn diffie_hellman(&self, peer_pub: &X25519PublicKey) -> [u8; 32] {
        let shared = self.secret.agree(peer_pub);
        shared.to_bytes()
    }
}

// -----------------------------------------------------------------------
// Symmetric Encryption (ChaCha20-Poly1305)
// -----------------------------------------------------------------------

/// Brane Session AEAD encryptor.
pub struct SessionCrypto {
    cipher: ChaCha20Poly1305,
}

impl SessionCrypto {
    /// Initialize with a 32-byte shared secret (from Diffie-Hellman).
    pub fn new(shared_secret: &[u8; 32]) -> Self {
        let key = Key::from_slice(shared_secret);
        Self {
            cipher: ChaCha20Poly1305::new(key),
        }
    }

    /// Encrypt a payload in-place. Requires a 12-byte nonce (e.g. packet counter).
    /// Returns the ciphertext appended with the 16-byte Poly1305 MAC.
    pub fn encrypt(&self, nonce_bytes: &[u8; 12], plaintext: &[u8]) -> Option<alloc::vec::Vec<u8>> {
        let nonce = Nonce::from_slice(nonce_bytes);
        self.cipher.encrypt(nonce, plaintext).ok()
    }

    /// Decrypt a payload. `ciphertext` must include the 16-byte MAC.
    pub fn decrypt(
        &self,
        nonce_bytes: &[u8; 12],
        ciphertext: &[u8],
    ) -> Option<alloc::vec::Vec<u8>> {
        let nonce = Nonce::from_slice(nonce_bytes);
        self.cipher.decrypt(nonce, ciphertext).ok()
    }
}
