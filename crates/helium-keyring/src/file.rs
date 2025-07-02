//! File-based keyring backend implementation
//!
//! This module provides a secure file-based storage backend for the keyring.
//! Keys are encrypted using AES-GCM with keys derived from passwords using Argon2.

use async_trait::async_trait;
use helium_crypto::{PrivateKey, PublicKey};
use helium_types::address::AccAddress;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::warn;
use zeroize::{Zeroize, ZeroizeOnDrop};

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use base64::{engine::general_purpose, Engine as _};
use bip39::{Language, Mnemonic};
use k256::ecdsa::SigningKey as Secp256k1PrivKey;
use rand::RngCore;

use crate::{ExportedKey, KeyInfo, Keyring, KeyringError};

/// File-based keyring backend
pub struct FileKeyring {
    /// Directory where key files are stored
    dir: PathBuf,
    /// In-memory cache of decrypted keys
    keys: HashMap<String, StoredKey>,
    /// Password used for encryption (stored securely)
    password: SecureString,
}

/// Securely stored string that gets zeroed on drop
#[derive(Clone, ZeroizeOnDrop)]
struct SecureString(String);

impl From<String> for SecureString {
    fn from(s: String) -> Self {
        SecureString(s)
    }
}

/// Stored key data structure
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
struct StoredKey {
    /// The private key bytes (encrypted when stored)
    #[zeroize(skip)]
    privkey: PrivateKey,
    /// The public key (stored for quick access)
    #[zeroize(skip)]
    pubkey: PublicKey,
    /// The account address
    #[zeroize(skip)]
    address: AccAddress,
}

/// Encrypted key file format
#[derive(Serialize, Deserialize)]
struct EncryptedKeyFile {
    /// Key name
    name: String,
    /// Key type (secp256k1, ed25519)
    key_type: String,
    /// Encrypted private key data (base64 encoded)
    encrypted_data: String,
    /// Encryption nonce (base64 encoded)
    nonce: String,
    /// Argon2 salt (base64 encoded)
    salt: String,
    /// Public key data (not encrypted)
    pubkey: PublicKey,
    /// Account address
    address: String,
}

impl FileKeyring {
    /// Create a new file-based keyring
    pub async fn new(dir: impl AsRef<Path>, password: String) -> Result<Self, KeyringError> {
        let dir = dir.as_ref().to_path_buf();

        // Create directory if it doesn't exist
        fs::create_dir_all(&dir).await.map_err(|e| {
            KeyringError::BackendError(format!("Failed to create keyring directory: {}", e))
        })?;

        let mut keyring = FileKeyring {
            dir,
            keys: HashMap::new(),
            password: SecureString::from(password),
        };

        // Load existing keys
        keyring.load_keys().await?;

        Ok(keyring)
    }

    /// Get the default keyring directory
    pub fn default_dir() -> Result<PathBuf, KeyringError> {
        let home = dirs::home_dir().ok_or_else(|| {
            KeyringError::BackendError("Could not determine home directory".to_string())
        })?;
        Ok(home.join(".helium").join("keyring"))
    }

    /// Load all keys from disk
    async fn load_keys(&mut self) -> Result<(), KeyringError> {
        let mut entries = fs::read_dir(&self.dir).await.map_err(|e| {
            KeyringError::BackendError(format!("Failed to read keyring directory: {}", e))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            KeyringError::BackendError(format!("Failed to read directory entry: {}", e))
        })? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(name) = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or("Invalid filename")
                {
                    // Try to load the key, but don't fail if one key is corrupted
                    if let Err(e) = self.load_key(name).await {
                        warn!(name = %name, error = %e, "Failed to load key");
                    }
                }
            }
        }

        Ok(())
    }

    /// Load a specific key from disk
    async fn load_key(&mut self, name: &str) -> Result<(), KeyringError> {
        let path = self.key_path(name);
        let data = fs::read_to_string(&path)
            .await
            .map_err(|e| KeyringError::BackendError(format!("Failed to read key file: {}", e)))?;

        let encrypted_file: EncryptedKeyFile = serde_json::from_str(&data)
            .map_err(|e| KeyringError::BackendError(format!("Failed to parse key file: {}", e)))?;

        // Decrypt the private key
        let privkey = self.decrypt_key(&encrypted_file)?;

        let stored_key = StoredKey {
            privkey,
            pubkey: encrypted_file.pubkey,
            address: AccAddress::from_bech32(&encrypted_file.address)
                .map_err(|e| KeyringError::BackendError(format!("Invalid address: {}", e)))?
                .1,
        };

        self.keys.insert(name.to_string(), stored_key);
        Ok(())
    }

    /// Encrypt a private key
    fn encrypt_key(&self, privkey: &PrivateKey) -> Result<EncryptedKeyFile, KeyringError> {
        // Generate salt for password hashing
        let salt = SaltString::generate(&mut OsRng);

        // Derive encryption key from password using Argon2
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(self.password.0.as_bytes(), &salt)
            .map_err(|e| KeyringError::BackendError(format!("Failed to hash password: {}", e)))?;

        // Use the hash as encryption key (take first 32 bytes)
        let hash = password_hash.hash.unwrap();
        let key_bytes = hash.as_bytes();
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes[..32]);

        // Create cipher
        let cipher = Aes256Gcm::new(key);

        // Generate nonce
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

        // Serialize private key
        let privkey_bytes = match privkey {
            PrivateKey::Secp256k1(key) => key.to_bytes().to_vec(),
            PrivateKey::Ed25519(key) => key.to_bytes().to_vec(),
        };

        // Encrypt
        let ciphertext = cipher
            .encrypt(&nonce, privkey_bytes.as_ref())
            .map_err(|e| KeyringError::BackendError(format!("Failed to encrypt key: {}", e)))?;

        let key_type = match privkey {
            PrivateKey::Secp256k1(_) => "secp256k1",
            PrivateKey::Ed25519(_) => "ed25519",
        };

        Ok(EncryptedKeyFile {
            name: String::new(), // Will be set by caller
            key_type: key_type.to_string(),
            encrypted_data: general_purpose::STANDARD.encode(&ciphertext),
            nonce: general_purpose::STANDARD.encode(nonce),
            salt: salt.to_string(),
            pubkey: privkey.public_key(),
            address: privkey.public_key().to_address().to_string(),
        })
    }

    /// Decrypt a private key
    fn decrypt_key(&self, encrypted_file: &EncryptedKeyFile) -> Result<PrivateKey, KeyringError> {
        // Parse salt
        let salt = SaltString::from_b64(&encrypted_file.salt)
            .map_err(|e| KeyringError::BackendError(format!("Invalid salt: {}", e)))?;

        // Derive decryption key from password
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(self.password.0.as_bytes(), &salt)
            .map_err(|e| KeyringError::BackendError(format!("Failed to hash password: {}", e)))?;

        // Use the hash as decryption key
        let hash = password_hash.hash.unwrap();
        let key_bytes = hash.as_bytes();
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes[..32]);

        // Create cipher
        let cipher = Aes256Gcm::new(key);

        // Decode nonce and ciphertext
        let nonce_bytes = general_purpose::STANDARD
            .decode(&encrypted_file.nonce)
            .map_err(|e| KeyringError::BackendError(format!("Invalid nonce: {}", e)))?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = general_purpose::STANDARD
            .decode(&encrypted_file.encrypted_data)
            .map_err(|e| KeyringError::BackendError(format!("Invalid encrypted data: {}", e)))?;

        // Decrypt
        let plaintext = cipher.decrypt(nonce, ciphertext.as_ref()).map_err(|_| {
            KeyringError::BackendError("Failed to decrypt key (wrong password?)".to_string())
        })?;

        // Reconstruct private key based on type
        match encrypted_file.key_type.as_str() {
            "secp256k1" => {
                let key = Secp256k1PrivKey::from_slice(&plaintext).map_err(|e| {
                    KeyringError::BackendError(format!("Invalid secp256k1 key: {}", e))
                })?;
                Ok(PrivateKey::Secp256k1(key))
            }
            "ed25519" => {
                if plaintext.len() != 32 {
                    return Err(KeyringError::BackendError(
                        "Invalid ed25519 key length".to_string(),
                    ));
                }
                let key = ed25519_dalek::SigningKey::from_bytes(&plaintext.try_into().unwrap());
                Ok(PrivateKey::Ed25519(key))
            }
            _ => Err(KeyringError::BackendError(format!(
                "Unknown key type: {}",
                encrypted_file.key_type
            ))),
        }
    }

    /// Get the file path for a key
    fn key_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{}.json", name))
    }

    /// Save a key to disk
    async fn save_key(&self, name: &str, privkey: &PrivateKey) -> Result<(), KeyringError> {
        let mut encrypted_file = self.encrypt_key(privkey)?;
        encrypted_file.name = name.to_string();

        let json = serde_json::to_string_pretty(&encrypted_file)
            .map_err(|e| KeyringError::BackendError(format!("Failed to serialize key: {}", e)))?;

        let path = self.key_path(name);
        fs::write(&path, json)
            .await
            .map_err(|e| KeyringError::BackendError(format!("Failed to write key file: {}", e)))?;

        Ok(())
    }

    /// Generate a new mnemonic phrase
    fn generate_mnemonic() -> Result<Mnemonic, KeyringError> {
        let mut entropy = [0u8; 32];
        OsRng.fill_bytes(&mut entropy);

        Mnemonic::from_entropy(&entropy)
            .map_err(|_| KeyringError::BackendError("Failed to generate mnemonic".to_string()))
    }

    /// Derive a private key from a mnemonic (using Cosmos standard derivation path)
    fn derive_from_mnemonic(mnemonic: &Mnemonic) -> Result<PrivateKey, KeyringError> {
        // Use proper BIP32/BIP44 HD derivation with path m/44'/118'/0'/0/0
        use crate::hd::derive_private_key_from_mnemonic;
        derive_private_key_from_mnemonic(&mnemonic.to_string(), None)
    }

    /// Export a key (WARNING: exports unencrypted private key)
    pub async fn export_key_impl(
        &self,
        name: &str,
        include_privkey: bool,
    ) -> Result<ExportedKey, KeyringError> {
        let key = self
            .keys
            .get(name)
            .ok_or_else(|| KeyringError::KeyNotFound(name.to_string()))?;

        let key_type = match &key.privkey {
            PrivateKey::Secp256k1(_) => "secp256k1",
            PrivateKey::Ed25519(_) => "ed25519",
        };

        let privkey_hex = if include_privkey {
            Some(match &key.privkey {
                PrivateKey::Secp256k1(k) => hex::encode(k.to_bytes()),
                PrivateKey::Ed25519(k) => hex::encode(k.to_bytes()),
            })
        } else {
            None
        };

        Ok(ExportedKey {
            name: name.to_string(),
            key_type: key_type.to_string(),
            mnemonic: None, // We don't store mnemonics
            privkey_hex,
            pubkey: key.pubkey.clone(),
            address: key.address.to_string(),
        })
    }

    /// Import a key from exported format
    pub async fn import_exported_key_impl(
        &mut self,
        exported: &ExportedKey,
    ) -> Result<KeyInfo, KeyringError> {
        // Check if key already exists
        if self.keys.contains_key(&exported.name) || self.key_path(&exported.name).exists() {
            return Err(KeyringError::KeyExists(exported.name.clone()));
        }

        // Import from mnemonic if available
        if let Some(mnemonic) = &exported.mnemonic {
            return self.import_key(&exported.name, mnemonic).await;
        }

        // Import from private key hex
        if let Some(privkey_hex) = &exported.privkey_hex {
            let privkey_bytes = hex::decode(privkey_hex).map_err(|e| {
                KeyringError::BackendError(format!("Invalid hex private key: {}", e))
            })?;

            let privkey = match exported.key_type.as_str() {
                "secp256k1" => {
                    let key = Secp256k1PrivKey::from_slice(&privkey_bytes).map_err(|e| {
                        KeyringError::BackendError(format!("Invalid secp256k1 key: {}", e))
                    })?;
                    PrivateKey::Secp256k1(key)
                }
                "ed25519" => {
                    if privkey_bytes.len() != 32 {
                        return Err(KeyringError::BackendError(
                            "Invalid ed25519 key length".to_string(),
                        ));
                    }
                    let key =
                        ed25519_dalek::SigningKey::from_bytes(&privkey_bytes.try_into().unwrap());
                    PrivateKey::Ed25519(key)
                }
                _ => {
                    return Err(KeyringError::BackendError(format!(
                        "Unknown key type: {}",
                        exported.key_type
                    )))
                }
            };

            let pubkey = privkey.public_key();
            let address = pubkey.to_address();

            // Save to disk
            self.save_key(&exported.name, &privkey).await?;

            // Store in memory
            let stored_key = StoredKey {
                privkey,
                pubkey: pubkey.clone(),
                address,
            };
            self.keys.insert(exported.name.clone(), stored_key);

            Ok(KeyInfo {
                name: exported.name.clone(),
                pubkey,
                address,
            })
        } else {
            Err(KeyringError::BackendError(
                "Exported key has neither mnemonic nor private key".to_string(),
            ))
        }
    }
}

#[async_trait]
impl Keyring for FileKeyring {
    async fn create_key(&mut self, name: &str) -> Result<KeyInfo, KeyringError> {
        // Check if key already exists
        if self.keys.contains_key(name) || self.key_path(name).exists() {
            return Err(KeyringError::KeyExists(name.to_string()));
        }

        // Generate new mnemonic and derive key
        let mnemonic = Self::generate_mnemonic()?;
        let privkey = Self::derive_from_mnemonic(&mnemonic)?;
        let pubkey = privkey.public_key();
        let address = pubkey.to_address();

        // Save to disk
        self.save_key(name, &privkey).await?;

        // Store in memory
        let stored_key = StoredKey {
            privkey,
            pubkey: pubkey.clone(),
            address,
        };
        self.keys.insert(name.to_string(), stored_key);

        Ok(KeyInfo {
            name: name.to_string(),
            pubkey,
            address,
        })
    }

    async fn import_key(&mut self, name: &str, mnemonic: &str) -> Result<KeyInfo, KeyringError> {
        // Check if key already exists
        if self.keys.contains_key(name) || self.key_path(name).exists() {
            return Err(KeyringError::KeyExists(name.to_string()));
        }

        // Parse and validate mnemonic
        let mnemonic = Mnemonic::parse_in(Language::English, mnemonic)
            .map_err(|_| KeyringError::InvalidMnemonic)?;

        // Derive key from mnemonic
        let privkey = Self::derive_from_mnemonic(&mnemonic)?;
        let pubkey = privkey.public_key();
        let address = pubkey.to_address();

        // Save to disk
        self.save_key(name, &privkey).await?;

        // Store in memory
        let stored_key = StoredKey {
            privkey,
            pubkey: pubkey.clone(),
            address,
        };
        self.keys.insert(name.to_string(), stored_key);

        Ok(KeyInfo {
            name: name.to_string(),
            pubkey,
            address,
        })
    }

    async fn list_keys(&self) -> Result<Vec<KeyInfo>, KeyringError> {
        Ok(self
            .keys
            .iter()
            .map(|(name, key)| KeyInfo {
                name: name.clone(),
                pubkey: key.pubkey.clone(),
                address: key.address,
            })
            .collect())
    }

    async fn get_key(&self, name: &str) -> Result<KeyInfo, KeyringError> {
        let key = self
            .keys
            .get(name)
            .ok_or_else(|| KeyringError::KeyNotFound(name.to_string()))?;

        Ok(KeyInfo {
            name: name.to_string(),
            pubkey: key.pubkey.clone(),
            address: key.address,
        })
    }

    async fn sign(&self, name: &str, data: &[u8]) -> Result<Vec<u8>, KeyringError> {
        let key = self
            .keys
            .get(name)
            .ok_or_else(|| KeyringError::KeyNotFound(name.to_string()))?;

        // Sign the data using the private key
        use helium_crypto::signature::sign_message;
        let signature = sign_message(&key.privkey, data)
            .map_err(|e| KeyringError::BackendError(format!("Failed to sign: {}", e)))?;

        Ok(signature)
    }

    async fn delete_key(&mut self, name: &str) -> Result<(), KeyringError> {
        // Remove from memory
        let _ = self
            .keys
            .remove(name)
            .ok_or_else(|| KeyringError::KeyNotFound(name.to_string()))?;

        // Delete file
        let path = self.key_path(name);
        if path.exists() {
            fs::remove_file(&path).await.map_err(|e| {
                KeyringError::BackendError(format!("Failed to delete key file: {}", e))
            })?;
        }

        Ok(())
    }

    async fn import_private_key(
        &mut self,
        name: &str,
        private_key_hex: &str,
    ) -> Result<KeyInfo, KeyringError> {
        // Check if key already exists
        if self.keys.contains_key(name) || self.key_path(name).exists() {
            return Err(KeyringError::KeyExists(name.to_string()));
        }

        let privkey_bytes = hex::decode(private_key_hex)
            .map_err(|e| KeyringError::BackendError(format!("Invalid hex private key: {}", e)))?;

        // Support both secp256k1 and ed25519 keys based on length
        let privkey = match privkey_bytes.len() {
            32 => {
                // Try secp256k1 first
                if let Ok(key) = Secp256k1PrivKey::from_slice(&privkey_bytes) {
                    PrivateKey::Secp256k1(key)
                } else {
                    // Try ed25519
                    let key =
                        ed25519_dalek::SigningKey::from_bytes(&privkey_bytes.try_into().unwrap());
                    PrivateKey::Ed25519(key)
                }
            }
            _ => {
                return Err(KeyringError::BackendError(
                    "Invalid private key length".to_string(),
                ))
            }
        };

        let pubkey = privkey.public_key();
        let address = pubkey.to_address();

        // Save to disk
        self.save_key(name, &privkey).await?;

        // Store in memory
        let stored_key = StoredKey {
            privkey,
            pubkey: pubkey.clone(),
            address,
        };
        self.keys.insert(name.to_string(), stored_key);

        Ok(KeyInfo {
            name: name.to_string(),
            pubkey,
            address,
        })
    }

    async fn export_key(
        &self,
        name: &str,
        include_private: bool,
    ) -> Result<ExportedKey, KeyringError> {
        self.export_key_impl(name, include_private).await
    }

    async fn import_exported_key(
        &mut self,
        exported: &ExportedKey,
    ) -> Result<KeyInfo, KeyringError> {
        self.import_exported_key_impl(exported).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_file_keyring_basic_operations() {
        let temp_dir = TempDir::new().unwrap();
        let mut keyring = FileKeyring::new(temp_dir.path(), "test_password".to_string())
            .await
            .unwrap();

        // Test key creation
        let key_info = keyring.create_key("test_key").await.unwrap();
        assert_eq!(key_info.name, "test_key");

        // Test listing keys
        let keys = keyring.list_keys().await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].name, "test_key");

        // Test getting a key
        let retrieved = keyring.get_key("test_key").await.unwrap();
        assert_eq!(retrieved.name, key_info.name);
        assert_eq!(retrieved.address, key_info.address);

        // Test signing
        let data = b"test message";
        let signature = keyring.sign("test_key", data).await.unwrap();
        assert!(!signature.is_empty());

        // Test key deletion
        keyring.delete_key("test_key").await.unwrap();
        let keys = keyring.list_keys().await.unwrap();
        assert_eq!(keys.len(), 0);

        // Test error on non-existent key
        assert!(matches!(
            keyring.get_key("non_existent").await,
            Err(KeyringError::KeyNotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_file_keyring_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let keyring_path = temp_dir.path();

        // Create keyring and add a key
        let key_info = {
            let mut keyring = FileKeyring::new(keyring_path, "test_password".to_string())
                .await
                .unwrap();
            let info = keyring.create_key("persistent_key").await.unwrap();
            info
        };

        // Create new keyring instance and check if key persists
        let keyring = FileKeyring::new(keyring_path, "test_password".to_string())
            .await
            .unwrap();
        let retrieved = keyring.get_key("persistent_key").await.unwrap();
        assert_eq!(retrieved.name, key_info.name);
        assert_eq!(retrieved.address, key_info.address);
    }

    #[tokio::test]
    async fn test_file_keyring_wrong_password() {
        let temp_dir = TempDir::new().unwrap();
        let keyring_path = temp_dir.path();

        // Create keyring with one password
        {
            let mut keyring = FileKeyring::new(keyring_path, "correct_password".to_string())
                .await
                .unwrap();
            keyring.create_key("test_key").await.unwrap();
        }

        // Try to load with wrong password
        let result = FileKeyring::new(keyring_path, "wrong_password".to_string()).await;
        // The keyring should load but fail to decrypt keys
        assert!(result.is_ok());
        let keyring = result.unwrap();
        // No keys should be loaded due to decryption failure
        let keys = keyring.list_keys().await.unwrap();
        assert_eq!(keys.len(), 0);
    }

    #[tokio::test]
    async fn test_file_keyring_import_mnemonic() {
        let temp_dir = TempDir::new().unwrap();
        let mut keyring = FileKeyring::new(temp_dir.path(), "test_password".to_string())
            .await
            .unwrap();

        // Test mnemonic from Cosmos SDK tests
        let mnemonic = "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius";

        let key_info = keyring.import_key("imported_key", mnemonic).await.unwrap();
        assert_eq!(key_info.name, "imported_key");

        // Verify the key can be used for signing
        let signature = keyring.sign("imported_key", b"test message").await.unwrap();
        assert!(!signature.is_empty());
    }

    #[tokio::test]
    async fn test_file_keyring_export_import() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        // Create key in first keyring
        let mut keyring1 = FileKeyring::new(temp_dir.path(), "password1".to_string())
            .await
            .unwrap();
        let created_key = keyring1.create_key("export_test").await.unwrap();

        // Export with private key
        let exported = keyring1.export_key_impl("export_test", true).await.unwrap();
        assert!(exported.privkey_hex.is_some());
        assert_eq!(exported.name, "export_test");
        assert_eq!(exported.address, created_key.address.to_string());

        // Import into second keyring
        let mut keyring2 = FileKeyring::new(temp_dir2.path(), "password2".to_string())
            .await
            .unwrap();
        let imported = keyring2.import_exported_key_impl(&exported).await.unwrap();

        // Verify imported key matches
        assert_eq!(imported.address, created_key.address);
        assert_eq!(imported.pubkey, created_key.pubkey);

        // Verify both keyrings can sign and produce same signature
        let message = b"test export/import";
        let sig1 = keyring1.sign("export_test", message).await.unwrap();
        let sig2 = keyring2.sign("export_test", message).await.unwrap();
        assert_eq!(sig1, sig2);
    }

    #[tokio::test]
    async fn test_file_keyring_export_without_privkey() {
        let temp_dir = TempDir::new().unwrap();
        let mut keyring = FileKeyring::new(temp_dir.path(), "test_password".to_string())
            .await
            .unwrap();

        keyring.create_key("test_key").await.unwrap();

        // Export without private key
        let exported = keyring.export_key_impl("test_key", false).await.unwrap();
        assert!(exported.privkey_hex.is_none());
        assert!(exported.mnemonic.is_none());

        // Should not be able to import without private key
        let temp_dir2 = TempDir::new().unwrap();
        let mut keyring2 = FileKeyring::new(temp_dir2.path(), "password2".to_string())
            .await
            .unwrap();
        let result = keyring2.import_exported_key_impl(&exported).await;
        assert!(matches!(result, Err(KeyringError::BackendError(_))));
    }
}
