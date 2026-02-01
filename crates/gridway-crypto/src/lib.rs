//! Cryptographic primitives for gridway.
//!
//! Wraps commonware-cryptography for ed25519 signing and SHA-256 hashing.
//! Replaces the previous Cosmos-style (secp256k1, bech32) crypto layer.

pub use commonware_cryptography::ed25519::{
    PrivateKey, PublicKey, Signature,
};
pub use commonware_cryptography::{
    Digestible, Hasher, Sha256,
    sha256::Digest,
};

use sha2::Digest as Sha2Digest;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid public key")]
    InvalidPublicKey,
    #[error("signing failed: {0}")]
    SigningFailed(String),
}

/// Compute SHA-256 hash of data
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Address type — derived from ed25519 public key via SHA-256 truncation
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Address(pub [u8; 20]);

impl Address {
    /// Derive address from ed25519 public key (first 20 bytes of SHA-256)
    pub fn from_public_key(pk: &PublicKey) -> Self {
        let pk_bytes: &[u8] = pk.as_ref();
        let hash = sha256(pk_bytes);
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash[..20]);
        Address(addr)
    }

    /// Create from raw bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.len() != 20 {
            return Err(CryptoError::InvalidPublicKey);
        }
        let mut addr = [0u8; 20];
        addr.copy_from_slice(bytes);
        Ok(Address(addr))
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256() {
        let hash = sha256(b"hello world");
        assert_eq!(hash.len(), 32);
        assert_ne!(hash, [0u8; 32]);
    }
}
