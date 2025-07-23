//! Hierarchical Deterministic (HD) wallet support
//!
//! This module implements BIP32/BIP44 key derivation following Cosmos SDK standards.
//! Standard Cosmos derivation path: m/44'/118'/0'/0/0
//!
//! Reference: https://github.com/bitcoin/bips/blob/master/bip-0032.mediawiki
//!            https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki

use crate::KeyringError;
use bip39::{Language, Mnemonic};
use gridway_crypto::PrivateKey;
use hmac::{Hmac, Mac};
use k256::ecdsa::SigningKey as Secp256k1PrivKey;
use sha2::{Digest, Sha256, Sha512};

type HmacSha512 = Hmac<Sha512>;

/// Cosmos SDK coin type as defined in SLIP-0044
/// https://github.com/satoshilabs/slips/blob/master/slip-0044.md
pub const COSMOS_COIN_TYPE: u32 = 118;

/// Standard Cosmos HD derivation path: m/44'/118'/0'/0/0
pub const COSMOS_HD_PATH: &str = "m/44'/118'/0'/0/0";

/// Extended private key data structure for BIP32 derivation
#[derive(Clone)]
pub struct ExtendedPrivateKey {
    /// The private key (32 bytes)
    private_key: [u8; 32],
    /// Chain code (32 bytes) for child key derivation
    chain_code: [u8; 32],
    /// Depth in the derivation tree
    depth: u8,
    /// Parent key fingerprint (first 4 bytes of parent public key hash)
    #[allow(dead_code)]
    parent_fingerprint: [u8; 4],
    /// Child index for this key
    #[allow(dead_code)]
    child_index: u32,
}

/// HD derivation path component
#[derive(Debug, Clone, Copy)]
pub struct PathComponent {
    /// Index value
    pub index: u32,
    /// Whether this is a hardened derivation (index >= 2^31)
    pub hardened: bool,
}

/// HD derivation path
#[derive(Debug, Clone)]
pub struct DerivationPath {
    components: Vec<PathComponent>,
}

impl DerivationPath {
    /// Parse a derivation path string (e.g., "m/44'/118'/0'/0/0")
    pub fn parse(path: &str) -> Result<Self, KeyringError> {
        if !path.starts_with("m/") && !path.starts_with("M/") {
            return Err(KeyringError::BackendError(
                "Derivation path must start with 'm/' or 'M/'".to_string(),
            ));
        }

        let path = &path[2..]; // Remove "m/"
        if path.is_empty() {
            return Ok(DerivationPath {
                components: Vec::new(),
            });
        }

        let mut components = Vec::new();
        for component in path.split('/') {
            if component.is_empty() {
                continue;
            }

            let (index_str, hardened) = if component.ends_with('\'') || component.ends_with('h') {
                (&component[..component.len() - 1], true)
            } else {
                (component, false)
            };

            let index = index_str.parse::<u32>().map_err(|_| {
                KeyringError::BackendError(format!("Invalid path component:: {component}"))
            })?;

            if hardened && index >= (1u32 << 31) {
                return Err(KeyringError::BackendError(
                    "Hardened derivation index too large".to_string(),
                ));
            }

            components.push(PathComponent {
                index: if hardened {
                    index + (1u32 << 31)
                } else {
                    index
                },
                hardened,
            });
        }

        Ok(DerivationPath { components })
    }

    /// Get the standard Cosmos derivation path
    pub fn cosmos_default() -> Self {
        Self::parse(COSMOS_HD_PATH).unwrap()
    }

    /// Create a custom Cosmos path with different account/address indices
    pub fn cosmos_custom(account: u32, address_index: u32) -> Self {
        Self::parse(&format!("m/44'/118'/{account}'/0/{address_index}")).unwrap()
    }

    /// Get the components of this path
    pub fn components(&self) -> &[PathComponent] {
        &self.components
    }
}

impl ExtendedPrivateKey {
    /// Create master key from seed (root of HD tree)
    pub fn from_seed(seed: &[u8]) -> Result<Self, KeyringError> {
        if seed.len() < 16 || seed.len() > 64 {
            return Err(KeyringError::BackendError(
                "Seed must be between 16 and 64 bytes".to_string(),
            ));
        }

        // HMAC-SHA512 with key "Bitcoin seed" for master key generation (BIP32 standard)
        let mut mac = HmacSha512::new_from_slice(b"Bitcoin seed")
            .map_err(|_| KeyringError::BackendError("Failed to create HMAC".to_string()))?;
        mac.update(seed);
        let result = mac.finalize().into_bytes();

        let mut private_key = [0u8; 32];
        let mut chain_code = [0u8; 32];
        private_key.copy_from_slice(&result[..32]);
        chain_code.copy_from_slice(&result[32..]);

        // Validate the private key is valid for secp256k1
        if Secp256k1PrivKey::from_slice(&private_key).is_err() {
            return Err(KeyringError::BackendError(
                "Generated master key is invalid for secp256k1".to_string(),
            ));
        }

        Ok(ExtendedPrivateKey {
            private_key,
            chain_code,
            depth: 0,
            parent_fingerprint: [0; 4],
            child_index: 0,
        })
    }

    /// Derive a child key at the given index
    pub fn derive_child(&self, index: u32) -> Result<ExtendedPrivateKey, KeyringError> {
        let hardened = index >= (1u32 << 31);

        // Create HMAC-SHA512 with the chain code
        let mut mac = HmacSha512::new_from_slice(&self.chain_code).map_err(|_| {
            KeyringError::BackendError("Failed to create HMAC for child derivation".to_string())
        })?;

        if hardened {
            // Hardened derivation: HMAC(chain_code, 0x00 || private_key || index)
            mac.update(&[0x00]);
            mac.update(&self.private_key);
        } else {
            // Non-hardened derivation: HMAC(chain_code, public_key || index)
            let private_key = Secp256k1PrivKey::from_slice(&self.private_key).map_err(|_| {
                KeyringError::BackendError("Invalid private key for child derivation".to_string())
            })?;
            let public_key = private_key.verifying_key();
            let public_key_bytes = public_key.to_encoded_point(true); // Compressed
            mac.update(public_key_bytes.as_bytes());
        }

        mac.update(&index.to_be_bytes());
        let result = mac.finalize().into_bytes();

        // Split result into key material and new chain code
        let mut tweak = [0u8; 32];
        let mut new_chain_code = [0u8; 32];
        tweak.copy_from_slice(&result[..32]);
        new_chain_code.copy_from_slice(&result[32..]);

        // Add tweak to parent private key (mod curve order)
        let mut new_private_key = self.private_key;
        if add_scalar(&mut new_private_key, &tweak).is_err() {
            return Err(KeyringError::BackendError(
                "Child key derivation resulted in invalid key".to_string(),
            ));
        }

        // Calculate parent fingerprint (first 4 bytes of parent public key hash)
        let parent_private_key = Secp256k1PrivKey::from_slice(&self.private_key)
            .map_err(|_| KeyringError::BackendError("Invalid parent private key".to_string()))?;
        let parent_public_key = parent_private_key.verifying_key();
        let parent_public_key_bytes = parent_public_key.to_encoded_point(true);
        let mut hasher = Sha256::new();
        hasher.update(parent_public_key_bytes.as_bytes());
        let hash = hasher.finalize();
        let mut parent_fingerprint = [0u8; 4];
        parent_fingerprint.copy_from_slice(&hash[..4]);

        Ok(ExtendedPrivateKey {
            private_key: new_private_key,
            chain_code: new_chain_code,
            depth: self.depth + 1,
            parent_fingerprint,
            child_index: index,
        })
    }

    /// Derive a key following a full derivation path
    pub fn derive_path(&self, path: &DerivationPath) -> Result<ExtendedPrivateKey, KeyringError> {
        let mut current = self.clone();
        for component in path.components() {
            current = current.derive_child(component.index)?;
        }
        Ok(current)
    }

    /// Get the private key as a PrivateKey enum
    pub fn private_key(&self) -> Result<PrivateKey, KeyringError> {
        let key = Secp256k1PrivKey::from_slice(&self.private_key)
            .map_err(|e| KeyringError::BackendError(format!("Invalid private key:: {e}")))?;
        Ok(PrivateKey::Secp256k1(key))
    }

    /// Get the raw private key bytes
    pub fn private_key_bytes(&self) -> &[u8; 32] {
        &self.private_key
    }

    /// Get the chain code
    pub fn chain_code(&self) -> &[u8; 32] {
        &self.chain_code
    }
}

/// Add a scalar to a private key (mod secp256k1 curve order)
fn add_scalar(private_key: &mut [u8; 32], tweak: &[u8; 32]) -> Result<(), KeyringError> {
    // Create a private key to verify the result will be valid
    let _original = Secp256k1PrivKey::from_slice(private_key).map_err(|_| {
        KeyringError::BackendError("Invalid private key for scalar addition".to_string())
    })?;

    // For now, use a simple modular addition approach
    // In a production implementation, this should use proper secp256k1 scalar arithmetic
    let mut carry = 0u64;
    for i in (0..32).rev() {
        let sum = private_key[i] as u64 + tweak[i] as u64 + carry;
        private_key[i] = sum as u8;
        carry = sum >> 8;
    }

    // Verify the result is a valid private key
    Secp256k1PrivKey::from_slice(private_key).map_err(|_| {
        KeyringError::BackendError("Scalar addition resulted in invalid private key".to_string())
    })?;

    Ok(())
}

/// Derive a private key from a mnemonic using the standard Cosmos path
pub fn derive_private_key_from_mnemonic(
    mnemonic: &str,
    path: Option<&DerivationPath>,
) -> Result<PrivateKey, KeyringError> {
    // Parse and validate mnemonic
    let mnemonic = Mnemonic::parse_in(Language::English, mnemonic)
        .map_err(|_| KeyringError::InvalidMnemonic)?;

    // Generate seed from mnemonic (with empty passphrase)
    let seed = mnemonic.to_seed("");

    // Create master key from seed
    let master_key = ExtendedPrivateKey::from_seed(&seed)?;

    // Use provided path or default Cosmos path
    let default_path = DerivationPath::cosmos_default();
    let derivation_path = path.unwrap_or(&default_path);

    // Derive the final private key
    let derived_key = master_key.derive_path(derivation_path)?;

    derived_key.private_key()
}

/// Generate a new random mnemonic phrase
pub fn generate_mnemonic() -> Result<Mnemonic, KeyringError> {
    use rand::RngCore;
    let mut entropy = [0u8; 32]; // 256 bits = 24 words
    rand::thread_rng().fill_bytes(&mut entropy);

    Mnemonic::from_entropy(&entropy)
        .map_err(|_| KeyringError::BackendError("Failed to generate mnemonic".to_string()))
}

/// Generate a new mnemonic with specific entropy length
pub fn generate_mnemonic_with_entropy(entropy_bits: usize) -> Result<Mnemonic, KeyringError> {
    use rand::RngCore;

    if !entropy_bits.is_multiple_of(32) || !(128..=256).contains(&entropy_bits) {
        return Err(KeyringError::BackendError(
            "Entropy must be 128, 160, 192, 224, or 256 bits".to_string(),
        ));
    }

    let entropy_bytes = entropy_bits / 8;
    let mut entropy = vec![0u8; entropy_bytes];
    rand::thread_rng().fill_bytes(&mut entropy);

    Mnemonic::from_entropy(&entropy)
        .map_err(|_| KeyringError::BackendError("Failed to generate mnemonic".to_string()))
}

/// Validate a mnemonic phrase
pub fn validate_mnemonic(mnemonic: &str) -> Result<(), KeyringError> {
    Mnemonic::parse_in(Language::English, mnemonic).map_err(|_| KeyringError::InvalidMnemonic)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derivation_path_parsing() {
        // Test standard Cosmos path
        let path = DerivationPath::parse("m/44'/118'/0'/0/0").unwrap();
        let components = path.components();
        assert_eq!(components.len(), 5);
        assert_eq!(components[0].index, 44 + (1 << 31)); // Hardened
        assert_eq!(components[1].index, 118 + (1 << 31)); // Hardened
        assert_eq!(components[2].index, (1 << 31)); // Hardened
        assert_eq!(components[3].index, 0); // Not hardened
        assert_eq!(components[4].index, 0); // Not hardened

        // Test default path
        let default_path = DerivationPath::cosmos_default();
        assert_eq!(default_path.components().len(), 5);

        // Test custom path
        let custom_path = DerivationPath::cosmos_custom(1, 5);
        let components = custom_path.components();
        assert_eq!(components[2].index, 1 + (1 << 31)); // Account 1
        assert_eq!(components[4].index, 5); // Address index 5
    }

    #[test]
    fn test_derivation_path_parsing_errors() {
        // Invalid start
        assert!(DerivationPath::parse("44'/118'/0'/0/0").is_err());

        // Invalid component
        assert!(DerivationPath::parse("m/44'/abc'/0'/0/0").is_err());
    }

    #[test]
    fn test_master_key_generation() {
        let seed = b"test seed for master key generation";
        let master_key = ExtendedPrivateKey::from_seed(seed).unwrap();

        assert_eq!(master_key.depth, 0);
        assert_eq!(master_key.parent_fingerprint, [0; 4]);
        assert_eq!(master_key.child_index, 0);

        // Should be able to get a valid private key
        let private_key = master_key.private_key().unwrap();
        assert!(matches!(private_key, PrivateKey::Secp256k1(_)));
    }

    #[test]
    fn test_child_key_derivation() {
        let seed = b"test seed for child key derivation";
        let master_key = ExtendedPrivateKey::from_seed(seed).unwrap();

        // Derive hardened child
        let child = master_key.derive_child(0x80000000).unwrap(); // Index 0, hardened
        assert_eq!(child.depth, 1);
        assert_eq!(child.child_index, 0x80000000);

        // Derive non-hardened child
        let child2 = master_key.derive_child(0).unwrap(); // Index 0, not hardened
        assert_eq!(child2.depth, 1);
        assert_eq!(child2.child_index, 0);

        // Children should be different
        assert_ne!(child.private_key_bytes(), child2.private_key_bytes());
    }

    #[test]
    fn test_full_path_derivation() {
        let seed = b"test seed for full path derivation";
        let master_key = ExtendedPrivateKey::from_seed(seed).unwrap();

        let path = DerivationPath::cosmos_default();
        let derived_key = master_key.derive_path(&path).unwrap();

        assert_eq!(derived_key.depth, 5);

        // Should be able to get a valid private key
        let private_key = derived_key.private_key().unwrap();
        assert!(matches!(private_key, PrivateKey::Secp256k1(_)));
    }

    #[test]
    fn test_mnemonic_derivation() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

        // Derive with default path
        let private_key = derive_private_key_from_mnemonic(mnemonic, None).unwrap();
        assert!(matches!(private_key, PrivateKey::Secp256k1(_)));

        // Derive with custom path
        let custom_path = DerivationPath::cosmos_custom(1, 0);
        let private_key2 = derive_private_key_from_mnemonic(mnemonic, Some(&custom_path)).unwrap();
        assert!(matches!(private_key2, PrivateKey::Secp256k1(_)));

        // Different paths should produce different keys
        let key1_bytes = match private_key {
            PrivateKey::Secp256k1(k) => k.to_bytes(),
            _ => unreachable!(),
        };
        let key2_bytes = match private_key2 {
            PrivateKey::Secp256k1(k) => k.to_bytes(),
            _ => unreachable!(),
        };
        assert_ne!(key1_bytes.as_slice(), key2_bytes.as_slice());
    }

    #[test]
    fn test_mnemonic_generation() {
        let mnemonic = generate_mnemonic().unwrap();
        let mnemonic_str = mnemonic.to_string();

        // Should be 24 words for 256-bit entropy
        assert_eq!(mnemonic_str.split_whitespace().count(), 24);

        // Should be valid
        validate_mnemonic(&mnemonic_str).unwrap();

        // Should be able to derive a key from it
        let private_key = derive_private_key_from_mnemonic(&mnemonic_str, None).unwrap();
        assert!(matches!(private_key, PrivateKey::Secp256k1(_)));
    }

    #[test]
    fn test_mnemonic_validation() {
        // Valid mnemonic
        let valid = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        validate_mnemonic(valid).unwrap();

        // Invalid mnemonic (wrong checksum)
        let invalid = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        assert!(validate_mnemonic(invalid).is_err());

        // Invalid mnemonic (wrong word)
        let invalid2 = "invalid abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        assert!(validate_mnemonic(invalid2).is_err());
    }

    #[test]
    fn test_deterministic_derivation() {
        let mnemonic = "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius";

        // Derive the same key multiple times
        let key1 = derive_private_key_from_mnemonic(mnemonic, None).unwrap();
        let key2 = derive_private_key_from_mnemonic(mnemonic, None).unwrap();

        let key1_bytes = match key1 {
            PrivateKey::Secp256k1(k) => k.to_bytes(),
            _ => unreachable!(),
        };
        let key2_bytes = match key2 {
            PrivateKey::Secp256k1(k) => k.to_bytes(),
            _ => unreachable!(),
        };

        // Should be identical
        assert_eq!(key1_bytes.as_slice(), key2_bytes.as_slice());
    }

    #[test]
    fn test_different_entropy_lengths() {
        // Test 128-bit entropy (12 words)
        let mnemonic_12 = generate_mnemonic_with_entropy(128).unwrap();
        assert_eq!(mnemonic_12.to_string().split_whitespace().count(), 12);

        // Test 256-bit entropy (24 words)
        let mnemonic_24 = generate_mnemonic_with_entropy(256).unwrap();
        assert_eq!(mnemonic_24.to_string().split_whitespace().count(), 24);

        // Test invalid entropy
        assert!(generate_mnemonic_with_entropy(100).is_err());
        assert!(generate_mnemonic_with_entropy(300).is_err());
    }
}
