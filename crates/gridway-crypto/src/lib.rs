//! Cryptographic primitives for gridway.
//!
//! Wraps commonware-cryptography for ed25519 signing and SHA-256 hashing.
//! Replaces the previous Cosmos-style (secp256k1, bech32) crypto layer.

pub use commonware_cryptography::ed25519::{PrivateKey, PublicKey, Signature};
pub use commonware_cryptography::{sha256::Digest, Digestible, Hasher, Sha256, Signer, Verifier};

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

/// Namespace used for transaction signing/verification.
/// Prevents cross-domain replay attacks by binding signatures to gridway TX context.
pub const TX_NAMESPACE: &[u8] = b"gridway-tx";

/// Sign a transaction body using the gridway TX namespace.
///
/// `body_bytes` should be the canonical JSON serialization of the TX body.
pub fn sign_tx_body(private_key: &PrivateKey, body_bytes: &[u8]) -> Signature {
    private_key.sign(TX_NAMESPACE, body_bytes)
}

/// Verify a transaction body signature using the gridway TX namespace.
///
/// `body_bytes` should be the canonical JSON serialization of the TX body.
pub fn verify_tx_body(public_key: &PublicKey, body_bytes: &[u8], signature: &Signature) -> bool {
    public_key.verify(TX_NAMESPACE, body_bytes, signature)
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
    use commonware_cryptography::Signer as _;

    #[test]
    fn test_sha256() {
        let hash = sha256(b"hello world");
        assert_eq!(hash.len(), 32);
        assert_ne!(hash, [0u8; 32]);
    }

    #[test]
    fn test_sign_and_verify_tx_body() {
        let private_key = PrivateKey::from_seed(42);
        let public_key = private_key.public_key();
        let body = b"test transaction body";

        let sig = sign_tx_body(&private_key, body);
        assert!(verify_tx_body(&public_key, body, &sig));

        // Wrong body should fail
        assert!(!verify_tx_body(&public_key, b"wrong body", &sig));
    }

    #[test]
    fn test_address_from_public_key() {
        let private_key = PrivateKey::from_seed(1);
        let public_key = private_key.public_key();
        let addr = Address::from_public_key(&public_key);
        assert_eq!(addr.to_hex().len(), 40);
    }
}
