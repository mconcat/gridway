//! Encrypted keystore for gridway private keys.
//!
//! Stores ed25519 private keys encrypted with ChaCha20-Poly1305, using a
//! password-derived key (HMAC-SHA256 iterated KDF). Each key is stored as a
//! JSON file in the keystore directory (default `~/.gridway/keys/`).

use std::fs;
use std::io;
use std::path::PathBuf;

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Number of KDF iterations. Not as strong as scrypt/argon2 but reasonable
/// for a SHA-256 HMAC-based approach and avoids extra dependencies.
const KDF_ITERATIONS: u32 = 100_000;
/// Salt length in bytes.
const SALT_LEN: usize = 32;
/// ChaCha20-Poly1305 nonce length.
const NONCE_LEN: usize = 12;

// ============================================================================
// Error
// ============================================================================

/// Errors that can occur during keystore operations.
#[derive(Debug, thiserror::Error)]
pub enum KeystoreError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("hex decode error: {0}")]
    Hex(#[from] hex::FromHexError),

    #[error("key '{0}' not found")]
    KeyNotFound(String),

    #[error("key '{0}' already exists")]
    KeyAlreadyExists(String),

    #[error("decryption failed (wrong password or corrupted file)")]
    DecryptionFailed,

    #[error("unsupported keystore version: {0}")]
    UnsupportedVersion(u32),

    #[error("invalid key name: {0}")]
    InvalidKeyName(String),
}

// ============================================================================
// Stored key file format
// ============================================================================

/// On-disk JSON format for an encrypted key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedKeyFile {
    /// Format version (currently 1).
    pub version: u32,
    /// Human-readable key name.
    pub name: String,
    /// KDF algorithm identifier.
    pub kdf: String,
    /// Hex-encoded KDF salt.
    pub salt: String,
    /// Hex-encoded AEAD nonce.
    pub nonce: String,
    /// Hex-encoded ciphertext (encrypted private key bytes).
    pub ciphertext: String,
}

// ============================================================================
// KDF
// ============================================================================

/// Derive a 32-byte encryption key from a password and salt using iterated
/// HMAC-SHA256. This is a simplified PBKDF2-HMAC-SHA256.
fn derive_key(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    // PBKDF2-HMAC-SHA256 single block (we only need 32 bytes = 1 block)
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(password).expect("HMAC accepts any key length");
    mac.update(salt);
    mac.update(&1u32.to_be_bytes()); // block index = 1
    let u1 = mac.finalize().into_bytes();

    let mut result = [0u8; 32];
    result.copy_from_slice(&u1);

    let mut prev = u1;
    for _ in 1..iterations {
        let mut mac =
            <HmacSha256 as Mac>::new_from_slice(password).expect("HMAC accepts any key length");
        mac.update(&prev);
        let ui = mac.finalize().into_bytes();
        for (r, u) in result.iter_mut().zip(ui.iter()) {
            *r ^= *u;
        }
        prev = ui;
    }

    result
}

// ============================================================================
// Keystore
// ============================================================================

/// Manages encrypted ed25519 private keys on disk.
///
/// # Example
///
/// ```rust,ignore
/// use gridway_client::keystore::Keystore;
///
/// let ks = Keystore::new(Some("/tmp/my-keys".into()));
/// ks.store_key("alice", &private_key_bytes, "my-password")?;
/// let loaded = ks.load_key("alice", "my-password")?;
/// ```
pub struct Keystore {
    dir: PathBuf,
}

impl Keystore {
    /// Create a new Keystore. If `dir` is `None`, uses `~/.gridway/keys/`.
    pub fn new(dir: Option<PathBuf>) -> Self {
        let dir = dir.unwrap_or_else(|| {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".gridway").join("keys")
        });
        Self { dir }
    }

    /// Ensure the keystore directory exists (create if needed).
    fn ensure_dir(&self) -> Result<(), KeystoreError> {
        if !self.dir.exists() {
            fs::create_dir_all(&self.dir)?;
        }
        Ok(())
    }

    /// Validate a key name (alphanumeric, hyphens, underscores only).
    fn validate_name(name: &str) -> Result<(), KeystoreError> {
        if name.is_empty() {
            return Err(KeystoreError::InvalidKeyName(
                "name cannot be empty".to_string(),
            ));
        }
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(KeystoreError::InvalidKeyName(format!(
                "name '{}' contains invalid characters (use alphanumeric, -, _)",
                name
            )));
        }
        Ok(())
    }

    /// Path to the key file for a given name.
    fn key_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{}.json", name))
    }

    /// Store a private key encrypted with the given password.
    ///
    /// `key_bytes` should be the raw ed25519 private key bytes (typically 64
    /// bytes from `commonware_cryptography::ed25519::PrivateKey::encode()`).
    pub fn store_key(
        &self,
        name: &str,
        key_bytes: &[u8],
        password: &str,
    ) -> Result<(), KeystoreError> {
        Self::validate_name(name)?;
        self.ensure_dir()?;

        let path = self.key_path(name);
        if path.exists() {
            return Err(KeystoreError::KeyAlreadyExists(name.to_string()));
        }

        let encrypted = encrypt(key_bytes, password)?;
        let file = EncryptedKeyFile {
            version: 1,
            name: name.to_string(),
            kdf: "pbkdf2-hmac-sha256".to_string(),
            salt: encrypted.salt,
            nonce: encrypted.nonce,
            ciphertext: encrypted.ciphertext,
        };

        let json = serde_json::to_string_pretty(&file)?;

        // Write with restrictive permissions (0600) on Unix to protect private keys.
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?;
            f.write_all(json.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            fs::write(&path, json)?;
        }
        Ok(())
    }

    /// Load and decrypt a private key by name.
    ///
    /// Returns the raw private key bytes.
    pub fn load_key(&self, name: &str, password: &str) -> Result<Vec<u8>, KeystoreError> {
        Self::validate_name(name)?;
        let path = self.key_path(name);
        if !path.exists() {
            return Err(KeystoreError::KeyNotFound(name.to_string()));
        }

        let json = fs::read_to_string(&path)?;
        let file: EncryptedKeyFile = serde_json::from_str(&json)?;

        if file.version != 1 {
            return Err(KeystoreError::UnsupportedVersion(file.version));
        }

        let encrypted = EncryptedData {
            salt: file.salt,
            nonce: file.nonce,
            ciphertext: file.ciphertext,
        };

        decrypt(&encrypted, password)
    }

    /// List all stored key names.
    pub fn list_keys(&self) -> Result<Vec<String>, KeystoreError> {
        self.ensure_dir()?;

        let mut names = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        Ok(names)
    }

    /// Delete a stored key by name.
    pub fn delete_key(&self, name: &str) -> Result<(), KeystoreError> {
        Self::validate_name(name)?;
        let path = self.key_path(name);
        if !path.exists() {
            return Err(KeystoreError::KeyNotFound(name.to_string()));
        }
        fs::remove_file(&path)?;
        Ok(())
    }

    /// Export a key: decrypt and return as hex string.
    pub fn export_key(&self, name: &str, password: &str) -> Result<String, KeystoreError> {
        let key_bytes = self.load_key(name, password)?;
        Ok(hex::encode(&key_bytes))
    }

    /// Import a key from a hex string, encrypting with the given password.
    pub fn import_key(
        &self,
        name: &str,
        key_hex: &str,
        password: &str,
    ) -> Result<(), KeystoreError> {
        let key_bytes = hex::decode(key_hex)?;
        self.store_key(name, &key_bytes, password)
    }
}

// ============================================================================
// Encryption internals
// ============================================================================

struct EncryptedData {
    salt: String,
    nonce: String,
    ciphertext: String,
}

fn encrypt(plaintext: &[u8], password: &str) -> Result<EncryptedData, KeystoreError> {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let key = derive_key(password.as_bytes(), &salt, KDF_ITERATIONS);

    let cipher = ChaCha20Poly1305::new_from_slice(&key)
        .expect("key is 32 bytes, always valid for ChaCha20Poly1305");
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| KeystoreError::DecryptionFailed)?;

    Ok(EncryptedData {
        salt: hex::encode(salt),
        nonce: hex::encode(nonce_bytes),
        ciphertext: hex::encode(ciphertext),
    })
}

fn decrypt(data: &EncryptedData, password: &str) -> Result<Vec<u8>, KeystoreError> {
    let salt = hex::decode(&data.salt)?;
    let nonce_bytes = hex::decode(&data.nonce)?;
    let ciphertext = hex::decode(&data.ciphertext)?;

    let key = derive_key(password.as_bytes(), &salt, KDF_ITERATIONS);

    let cipher = ChaCha20Poly1305::new_from_slice(&key)
        .expect("key is 32 bytes, always valid for ChaCha20Poly1305");
    let nonce = Nonce::from_slice(&nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| KeystoreError::DecryptionFailed)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_keystore() -> (Keystore, TempDir) {
        let tmp = TempDir::new().unwrap();
        let ks = Keystore::new(Some(tmp.path().to_path_buf()));
        (ks, tmp)
    }

    /// Fake 64-byte key for testing (mimics ed25519 private key encoding).
    fn fake_key() -> Vec<u8> {
        let mut key = vec![0u8; 64];
        for (i, b) in key.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(42);
        }
        key
    }

    #[test]
    fn test_store_and_load_roundtrip() {
        let (ks, _tmp) = test_keystore();
        let key = fake_key();
        let password = "hunter2";

        ks.store_key("alice", &key, password).unwrap();
        let loaded = ks.load_key("alice", password).unwrap();
        assert_eq!(loaded, key);
    }

    #[test]
    fn test_wrong_password_fails() {
        let (ks, _tmp) = test_keystore();
        let key = fake_key();

        ks.store_key("bob", &key, "correct-password").unwrap();
        let result = ks.load_key("bob", "wrong-password");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            KeystoreError::DecryptionFailed
        ));
    }

    #[test]
    fn test_list_keys() {
        let (ks, _tmp) = test_keystore();
        let key = fake_key();

        assert_eq!(ks.list_keys().unwrap(), Vec::<String>::new());

        ks.store_key("charlie", &key, "pw1").unwrap();
        ks.store_key("alice", &key, "pw2").unwrap();
        ks.store_key("bob", &key, "pw3").unwrap();

        let mut names = ks.list_keys().unwrap();
        names.sort();
        assert_eq!(names, vec!["alice", "bob", "charlie"]);
    }

    #[test]
    fn test_delete_key() {
        let (ks, _tmp) = test_keystore();
        let key = fake_key();

        ks.store_key("doomed", &key, "pw").unwrap();
        assert_eq!(ks.list_keys().unwrap().len(), 1);

        ks.delete_key("doomed").unwrap();
        assert_eq!(ks.list_keys().unwrap().len(), 0);

        // Deleting again should fail
        let result = ks.delete_key("doomed");
        assert!(matches!(result.unwrap_err(), KeystoreError::KeyNotFound(_)));
    }

    #[test]
    fn test_import_export_roundtrip() {
        let (ks, _tmp) = test_keystore();
        let key = fake_key();
        let key_hex = hex::encode(&key);
        let password = "export-test";

        ks.import_key("exported", &key_hex, password).unwrap();
        let exported_hex = ks.export_key("exported", password).unwrap();
        assert_eq!(exported_hex, key_hex);
    }

    #[test]
    fn test_duplicate_key_name_fails() {
        let (ks, _tmp) = test_keystore();
        let key = fake_key();

        ks.store_key("unique", &key, "pw").unwrap();
        let result = ks.store_key("unique", &key, "pw");
        assert!(matches!(
            result.unwrap_err(),
            KeystoreError::KeyAlreadyExists(_)
        ));
    }

    #[test]
    fn test_load_nonexistent_key() {
        let (ks, _tmp) = test_keystore();
        let result = ks.load_key("ghost", "pw");
        assert!(matches!(result.unwrap_err(), KeystoreError::KeyNotFound(_)));
    }

    #[test]
    fn test_invalid_key_name() {
        let (ks, _tmp) = test_keystore();
        let key = fake_key();

        let result = ks.store_key("", &key, "pw");
        assert!(matches!(
            result.unwrap_err(),
            KeystoreError::InvalidKeyName(_)
        ));

        let result = ks.store_key("has spaces", &key, "pw");
        assert!(matches!(
            result.unwrap_err(),
            KeystoreError::InvalidKeyName(_)
        ));

        let result = ks.store_key("../escape", &key, "pw");
        assert!(matches!(
            result.unwrap_err(),
            KeystoreError::InvalidKeyName(_)
        ));
    }

    #[test]
    fn test_kdf_deterministic() {
        let key1 = derive_key(b"password", b"salt", 1000);
        let key2 = derive_key(b"password", b"salt", 1000);
        assert_eq!(key1, key2);

        let key3 = derive_key(b"different", b"salt", 1000);
        assert_ne!(key1, key3);

        let key4 = derive_key(b"password", b"different-salt", 1000);
        assert_ne!(key1, key4);
    }

    #[test]
    fn test_encrypted_file_format() {
        let (ks, _tmp) = test_keystore();
        let key = fake_key();
        ks.store_key("format-test", &key, "pw").unwrap();

        let path = ks.key_path("format-test");
        let json = fs::read_to_string(&path).unwrap();
        let file: EncryptedKeyFile = serde_json::from_str(&json).unwrap();

        assert_eq!(file.version, 1);
        assert_eq!(file.name, "format-test");
        assert_eq!(file.kdf, "pbkdf2-hmac-sha256");
        assert_eq!(hex::decode(&file.salt).unwrap().len(), SALT_LEN);
        assert_eq!(hex::decode(&file.nonce).unwrap().len(), NONCE_LEN);
        // Ciphertext should be longer than plaintext (includes 16-byte Poly1305 tag)
        assert!(hex::decode(&file.ciphertext).unwrap().len() > key.len());
    }
}
