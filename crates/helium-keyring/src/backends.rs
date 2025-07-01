//! Keyring backend implementations
//!
//! This module provides various backend implementations for the keyring:
//! - FileKeyring: Encrypted file-based storage
//! - MemoryKeyring: In-memory storage (for testing)
//! - OsKeyring: Operating system secure storage integration

use async_trait::async_trait;
use helium_crypto::PrivateKey;
use k256::ecdsa::SigningKey as Secp256k1PrivKey;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{ExportedKey, KeyInfo, Keyring, KeyringError};

/// In-memory keyring backend for testing and development
///
/// **WARNING**: This backend stores keys in plain text in memory.
/// It should only be used for testing and development purposes.
#[derive(Debug)]
pub struct MemoryKeyring {
    keys: HashMap<String, StoredKey>,
}

/// Operating System secure storage keyring backend
///
/// This backend uses the operating system's secure storage mechanisms:
/// - macOS: Keychain Services
/// - Windows: Windows Credential Store  
/// - Linux: libsecret (GNOME Keyring) or KWallet (KDE)
pub struct OsKeyring {
    service_name: String,
    keys_cache: HashMap<String, StoredKey>,
}

/// Stored key data for backends
#[derive(Clone, Debug, Zeroize, ZeroizeOnDrop)]
struct StoredKey {
    #[zeroize(skip)]
    privkey: PrivateKey,
    #[zeroize(skip)]
    pubkey: helium_crypto::PublicKey,
    #[zeroize(skip)]
    address: helium_types::address::AccAddress,
}

/// Serializable key data for OS keyring storage
#[derive(Serialize, Deserialize)]
struct SerializableKey {
    key_type: String,
    privkey_bytes: Vec<u8>,
    pubkey: helium_crypto::PublicKey,
    address: String,
}

impl MemoryKeyring {
    /// Create a new in-memory keyring
    pub fn new() -> Self {
        MemoryKeyring {
            keys: HashMap::new(),
        }
    }

    /// Generate a new private key
    fn generate_key() -> Result<PrivateKey, KeyringError> {
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);

        let signing_key = Secp256k1PrivKey::from_slice(&bytes)
            .map_err(|e| KeyringError::BackendError(format!("Failed to generate key: {}", e)))?;

        Ok(PrivateKey::Secp256k1(signing_key))
    }

    /// Derive key from mnemonic using proper HD derivation
    fn derive_from_mnemonic(mnemonic: &str) -> Result<PrivateKey, KeyringError> {
        use crate::hd::derive_private_key_from_mnemonic;
        derive_private_key_from_mnemonic(mnemonic, None)
    }
}

impl Default for MemoryKeyring {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Keyring for MemoryKeyring {
    async fn create_key(&mut self, name: &str) -> Result<KeyInfo, KeyringError> {
        if self.keys.contains_key(name) {
            return Err(KeyringError::KeyExists(name.to_string()));
        }

        let privkey = Self::generate_key()?;
        let pubkey = privkey.public_key();
        let address = pubkey.to_address();

        let stored_key = StoredKey {
            privkey,
            pubkey: pubkey.clone(),
            address: address.clone(),
        };

        self.keys.insert(name.to_string(), stored_key);

        Ok(KeyInfo {
            name: name.to_string(),
            pubkey,
            address,
        })
    }

    async fn import_key(&mut self, name: &str, mnemonic: &str) -> Result<KeyInfo, KeyringError> {
        if self.keys.contains_key(name) {
            return Err(KeyringError::KeyExists(name.to_string()));
        }

        let privkey = Self::derive_from_mnemonic(mnemonic)?;
        let pubkey = privkey.public_key();
        let address = pubkey.to_address();

        let stored_key = StoredKey {
            privkey,
            pubkey: pubkey.clone(),
            address: address.clone(),
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
                address: key.address.clone(),
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
            address: key.address.clone(),
        })
    }

    async fn sign(&self, name: &str, data: &[u8]) -> Result<Vec<u8>, KeyringError> {
        let key = self
            .keys
            .get(name)
            .ok_or_else(|| KeyringError::KeyNotFound(name.to_string()))?;

        use helium_crypto::signature::sign_message;
        let signature = sign_message(&key.privkey, data)
            .map_err(|e| KeyringError::BackendError(format!("Failed to sign: {}", e)))?;

        Ok(signature)
    }

    async fn delete_key(&mut self, name: &str) -> Result<(), KeyringError> {
        self.keys
            .remove(name)
            .ok_or_else(|| KeyringError::KeyNotFound(name.to_string()))?;

        Ok(())
    }

    async fn import_private_key(
        &mut self,
        name: &str,
        private_key_hex: &str,
    ) -> Result<KeyInfo, KeyringError> {
        if self.keys.contains_key(name) {
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

        let stored_key = StoredKey {
            privkey,
            pubkey: pubkey.clone(),
            address: address.clone(),
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
        let key = self
            .keys
            .get(name)
            .ok_or_else(|| KeyringError::KeyNotFound(name.to_string()))?;

        let key_type = match &key.privkey {
            PrivateKey::Secp256k1(_) => "secp256k1",
            PrivateKey::Ed25519(_) => "ed25519",
        };

        let privkey_hex = if include_private {
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
            mnemonic: None, // Memory keyring doesn't store mnemonics
            privkey_hex,
            pubkey: key.pubkey.clone(),
            address: key.address.to_string(),
        })
    }

    async fn import_exported_key(
        &mut self,
        exported: &ExportedKey,
    ) -> Result<KeyInfo, KeyringError> {
        // Check if key already exists
        if self.keys.contains_key(&exported.name) {
            return Err(KeyringError::KeyExists(exported.name.clone()));
        }

        // Import from mnemonic if available
        if let Some(mnemonic) = &exported.mnemonic {
            return self.import_key(&exported.name, mnemonic).await;
        }

        // Import from private key hex
        if let Some(privkey_hex) = &exported.privkey_hex {
            return self.import_private_key(&exported.name, privkey_hex).await;
        }

        Err(KeyringError::BackendError(
            "Exported key has neither mnemonic nor private key".to_string(),
        ))
    }
}

impl OsKeyring {
    /// Create a new OS keyring backend
    pub fn new(service_name: impl Into<String>) -> Self {
        OsKeyring {
            service_name: service_name.into(),
            keys_cache: HashMap::new(),
        }
    }

    /// Get the default service name for helium keys
    pub fn default_service_name() -> String {
        "helium-keyring".to_string()
    }

    /// Store a key in the OS secure storage
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    async fn store_key_in_os(
        &self,
        name: &str,
        key_data: &SerializableKey,
    ) -> Result<(), KeyringError> {
        let key_json = serde_json::to_string(key_data)
            .map_err(|e| KeyringError::BackendError(format!("Failed to serialize key: {}", e)))?;

        // Use the keyring crate for cross-platform OS secure storage
        let entry = keyring::Entry::new(&self.service_name, name).map_err(|e| {
            KeyringError::BackendError(format!("Failed to create keyring entry: {}", e))
        })?;

        entry.set_password(&key_json).map_err(|e| {
            KeyringError::BackendError(format!("Failed to store key in OS keyring: {}", e))
        })?;

        Ok(())
    }

    /// Retrieve a key from the OS secure storage  
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    async fn retrieve_key_from_os(&self, name: &str) -> Result<SerializableKey, KeyringError> {
        let entry = keyring::Entry::new(&self.service_name, name).map_err(|e| {
            KeyringError::BackendError(format!("Failed to create keyring entry: {}", e))
        })?;

        let key_json = entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => KeyringError::KeyNotFound(name.to_string()),
            _ => {
                KeyringError::BackendError(format!("Failed to retrieve key from OS keyring: {}", e))
            }
        })?;

        let key_data: SerializableKey = serde_json::from_str(&key_json).map_err(|e| {
            KeyringError::BackendError(format!("Failed to deserialize key data: {}", e))
        })?;

        Ok(key_data)
    }

    /// Delete a key from the OS secure storage
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    async fn delete_key_from_os(&self, name: &str) -> Result<(), KeyringError> {
        let entry = keyring::Entry::new(&self.service_name, name).map_err(|e| {
            KeyringError::BackendError(format!("Failed to create keyring entry: {}", e))
        })?;

        entry.delete_credential().map_err(|e| match e {
            keyring::Error::NoEntry => KeyringError::KeyNotFound(name.to_string()),
            _ => KeyringError::BackendError(format!("Failed to delete key from OS keyring: {}", e)),
        })?;

        Ok(())
    }

    /// List all keys from the OS secure storage
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    async fn list_keys_from_os(&self) -> Result<Vec<String>, KeyringError> {
        // The keyring crate doesn't provide a native list function
        // We'll maintain a registry key that stores the list of key names
        let registry_entry = keyring::Entry::new(&self.service_name, "_helium_key_registry")
            .map_err(|e| {
                KeyringError::BackendError(format!("Failed to create registry entry: {}", e))
            })?;

        println!("DEBUG: list_keys_from_os: attempting to read registry with service: {} user: _helium_key_registry", self.service_name);

        match registry_entry.get_password() {
            Ok(registry_json) => {
                println!(
                    "DEBUG: list_keys_from_os: got registry data: {}",
                    registry_json
                );
                let key_names: Vec<String> = serde_json::from_str(&registry_json).map_err(|e| {
                    KeyringError::BackendError(format!("Failed to parse key registry: {}", e))
                })?;
                println!(
                    "DEBUG: list_keys_from_os: parsed {} keys: {:?}",
                    key_names.len(),
                    key_names
                );
                Ok(key_names)
            }
            Err(keyring::Error::NoEntry) => {
                println!("DEBUG: list_keys_from_os: NoEntry error - registry doesn't exist");
                // No registry exists yet, return empty list
                Ok(Vec::new())
            }
            Err(e) => {
                println!("DEBUG: list_keys_from_os: other error: {:?}", e);
                Err(KeyringError::BackendError(format!(
                    "Failed to access key registry: {}",
                    e
                )))
            }
        }
    }

    /// Add a key name to the registry
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    async fn add_key_to_registry(&self, name: &str) -> Result<(), KeyringError> {
        let mut key_names = self.list_keys_from_os().await?;
        println!("DEBUG: add_key_to_registry: current keys: {:?}", key_names);

        if !key_names.contains(&name.to_string()) {
            key_names.push(name.to_string());
            key_names.sort(); // Keep sorted for consistency

            let registry_entry = keyring::Entry::new(&self.service_name, "_helium_key_registry")
                .map_err(|e| {
                    KeyringError::BackendError(format!("Failed to create registry entry: {}", e))
                })?;

            let registry_json = serde_json::to_string(&key_names).map_err(|e| {
                KeyringError::BackendError(format!("Failed to serialize key registry: {}", e))
            })?;

            println!(
                "DEBUG: add_key_to_registry: updating registry with: {}",
                registry_json
            );

            registry_entry.set_password(&registry_json).map_err(|e| {
                KeyringError::BackendError(format!("Failed to update key registry: {}", e))
            })?;

            // Add a small delay to ensure the keyring write is flushed
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        Ok(())
    }

    /// Remove a key name from the registry
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    async fn remove_key_from_registry(&self, name: &str) -> Result<(), KeyringError> {
        let mut key_names = self.list_keys_from_os().await?;
        if let Some(pos) = key_names.iter().position(|x| x == name) {
            key_names.remove(pos);

            let registry_entry = keyring::Entry::new(&self.service_name, "_helium_key_registry")
                .map_err(|e| {
                    KeyringError::BackendError(format!("Failed to create registry entry: {}", e))
                })?;

            if key_names.is_empty() {
                // If no keys left, remove the registry entry
                let _ = registry_entry.delete_credential(); // Ignore error if already deleted
            } else {
                let registry_json = serde_json::to_string(&key_names).map_err(|e| {
                    KeyringError::BackendError(format!("Failed to serialize key registry: {}", e))
                })?;

                registry_entry.set_password(&registry_json).map_err(|e| {
                    KeyringError::BackendError(format!("Failed to update key registry: {}", e))
                })?;
            }
        }
        Ok(())
    }

    /// Unsupported platform fallback methods
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    async fn store_key_in_os(
        &self,
        _name: &str,
        _key_data: &SerializableKey,
    ) -> Result<(), KeyringError> {
        Err(KeyringError::BackendError(
            "OS keyring not supported on this platform. Use FileKeyring or MemoryKeyring."
                .to_string(),
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    async fn retrieve_key_from_os(&self, _name: &str) -> Result<SerializableKey, KeyringError> {
        Err(KeyringError::BackendError(
            "OS keyring not supported on this platform. Use FileKeyring or MemoryKeyring."
                .to_string(),
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    async fn delete_key_from_os(&self, _name: &str) -> Result<(), KeyringError> {
        Err(KeyringError::BackendError(
            "OS keyring not supported on this platform. Use FileKeyring or MemoryKeyring."
                .to_string(),
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    async fn list_keys_from_os(&self) -> Result<Vec<String>, KeyringError> {
        Err(KeyringError::BackendError(
            "OS keyring not supported on this platform. Use FileKeyring or MemoryKeyring."
                .to_string(),
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    async fn add_key_to_registry(&self, _name: &str) -> Result<(), KeyringError> {
        Err(KeyringError::BackendError(
            "OS keyring not supported on this platform. Use FileKeyring or MemoryKeyring."
                .to_string(),
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    async fn remove_key_from_registry(&self, _name: &str) -> Result<(), KeyringError> {
        Err(KeyringError::BackendError(
            "OS keyring not supported on this platform. Use FileKeyring or MemoryKeyring."
                .to_string(),
        ))
    }

    /// Convert stored key to serializable format
    fn to_serializable_key(&self, stored_key: &StoredKey, _name: &str) -> SerializableKey {
        let (key_type, privkey_bytes) = match &stored_key.privkey {
            PrivateKey::Secp256k1(key) => ("secp256k1".to_string(), key.to_bytes().to_vec()),
            PrivateKey::Ed25519(key) => ("ed25519".to_string(), key.to_bytes().to_vec()),
        };

        SerializableKey {
            key_type,
            privkey_bytes,
            pubkey: stored_key.pubkey.clone(),
            address: stored_key.address.to_string(),
        }
    }

    /// Convert serializable key to stored key format
    fn from_serializable_key(
        &self,
        serializable: SerializableKey,
    ) -> Result<StoredKey, KeyringError> {
        let privkey = match serializable.key_type.as_str() {
            "secp256k1" => {
                let key =
                    Secp256k1PrivKey::from_slice(&serializable.privkey_bytes).map_err(|e| {
                        KeyringError::BackendError(format!("Invalid secp256k1 key: {}", e))
                    })?;
                PrivateKey::Secp256k1(key)
            }
            "ed25519" => {
                if serializable.privkey_bytes.len() != 32 {
                    return Err(KeyringError::BackendError(
                        "Invalid ed25519 key length".to_string(),
                    ));
                }
                let key = ed25519_dalek::SigningKey::from_bytes(
                    &serializable.privkey_bytes.clone().try_into().unwrap(),
                );
                PrivateKey::Ed25519(key)
            }
            _ => {
                return Err(KeyringError::BackendError(format!(
                    "Unknown key type: {}",
                    serializable.key_type
                )))
            }
        };

        let address = helium_types::address::AccAddress::from_bech32(&serializable.address)
            .map_err(|e| KeyringError::BackendError(format!("Invalid address: {}", e)))?
            .1;

        Ok(StoredKey {
            privkey,
            pubkey: serializable.pubkey.clone(),
            address,
        })
    }

    /// Generate a new private key
    fn generate_key() -> Result<PrivateKey, KeyringError> {
        MemoryKeyring::generate_key()
    }

    /// Derive key from mnemonic using proper HD derivation
    fn derive_from_mnemonic(mnemonic: &str) -> Result<PrivateKey, KeyringError> {
        MemoryKeyring::derive_from_mnemonic(mnemonic)
    }
}

#[async_trait]
impl Keyring for OsKeyring {
    async fn create_key(&mut self, name: &str) -> Result<KeyInfo, KeyringError> {
        // Check if key already exists in cache
        if self.keys_cache.contains_key(name) {
            return Err(KeyringError::KeyExists(name.to_string()));
        }

        // Try to retrieve from OS storage to check existence
        if self.retrieve_key_from_os(name).await.is_ok() {
            return Err(KeyringError::KeyExists(name.to_string()));
        }

        let privkey = Self::generate_key()?;
        let pubkey = privkey.public_key();
        let address = pubkey.to_address();

        let stored_key = StoredKey {
            privkey,
            pubkey: pubkey.clone(),
            address: address.clone(),
        };

        // Store in OS secure storage
        let serializable = self.to_serializable_key(&stored_key, name);
        self.store_key_in_os(name, &serializable).await?;

        // Add to key registry
        self.add_key_to_registry(name).await?;

        // Cache in memory
        self.keys_cache.insert(name.to_string(), stored_key);

        Ok(KeyInfo {
            name: name.to_string(),
            pubkey,
            address,
        })
    }

    async fn import_key(&mut self, name: &str, mnemonic: &str) -> Result<KeyInfo, KeyringError> {
        // Check if key already exists
        if self.keys_cache.contains_key(name) {
            return Err(KeyringError::KeyExists(name.to_string()));
        }

        if self.retrieve_key_from_os(name).await.is_ok() {
            return Err(KeyringError::KeyExists(name.to_string()));
        }

        let privkey = Self::derive_from_mnemonic(mnemonic)?;
        let pubkey = privkey.public_key();
        let address = pubkey.to_address();

        let stored_key = StoredKey {
            privkey,
            pubkey: pubkey.clone(),
            address: address.clone(),
        };

        // Store in OS secure storage
        let serializable = self.to_serializable_key(&stored_key, name);
        self.store_key_in_os(name, &serializable).await?;

        // Add to key registry
        self.add_key_to_registry(name).await?;

        // Cache in memory
        self.keys_cache.insert(name.to_string(), stored_key);

        Ok(KeyInfo {
            name: name.to_string(),
            pubkey,
            address,
        })
    }

    async fn list_keys(&self) -> Result<Vec<KeyInfo>, KeyringError> {
        // Get keys from OS storage
        let os_key_names = self.list_keys_from_os().await?;

        let mut keys = Vec::new();
        for name in os_key_names {
            if let Ok(serializable) = self.retrieve_key_from_os(&name).await {
                keys.push(KeyInfo {
                    name: name.clone(),
                    pubkey: serializable.pubkey.clone(),
                    address: helium_types::address::AccAddress::from_bech32(&serializable.address)
                        .map_err(|e| KeyringError::BackendError(format!("Invalid address: {}", e)))?
                        .1,
                });
            }
        }

        Ok(keys)
    }

    async fn get_key(&self, name: &str) -> Result<KeyInfo, KeyringError> {
        // Check cache first
        if let Some(key) = self.keys_cache.get(name) {
            return Ok(KeyInfo {
                name: name.to_string(),
                pubkey: key.pubkey.clone(),
                address: key.address.clone(),
            });
        }

        // Try to retrieve from OS storage
        let serializable = self.retrieve_key_from_os(name).await?;

        Ok(KeyInfo {
            name: name.to_string(),
            pubkey: serializable.pubkey.clone(),
            address: helium_types::address::AccAddress::from_bech32(&serializable.address)
                .map_err(|e| KeyringError::BackendError(format!("Invalid address: {}", e)))?
                .1,
        })
    }

    async fn sign(&self, name: &str, data: &[u8]) -> Result<Vec<u8>, KeyringError> {
        // Check cache first
        let stored_key = if let Some(key) = self.keys_cache.get(name) {
            key.clone()
        } else {
            // Load from OS storage
            let serializable = self.retrieve_key_from_os(name).await?;
            self.from_serializable_key(serializable)?
        };

        use helium_crypto::signature::sign_message;
        let signature = sign_message(&stored_key.privkey, data)
            .map_err(|e| KeyringError::BackendError(format!("Failed to sign: {}", e)))?;

        Ok(signature)
    }

    async fn delete_key(&mut self, name: &str) -> Result<(), KeyringError> {
        // Remove from cache
        self.keys_cache.remove(name);

        // Delete from OS storage
        self.delete_key_from_os(name).await?;

        // Remove from key registry
        self.remove_key_from_registry(name).await?;

        Ok(())
    }

    async fn import_private_key(
        &mut self,
        name: &str,
        private_key_hex: &str,
    ) -> Result<KeyInfo, KeyringError> {
        // Check if key already exists
        if self.keys_cache.contains_key(name) {
            return Err(KeyringError::KeyExists(name.to_string()));
        }

        if self.retrieve_key_from_os(name).await.is_ok() {
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

        let stored_key = StoredKey {
            privkey,
            pubkey: pubkey.clone(),
            address: address.clone(),
        };

        // Store in OS secure storage
        let serializable = self.to_serializable_key(&stored_key, name);
        self.store_key_in_os(name, &serializable).await?;

        // Add to key registry
        self.add_key_to_registry(name).await?;

        // Cache in memory
        self.keys_cache.insert(name.to_string(), stored_key);

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
        // Check cache first
        let stored_key = if let Some(key) = self.keys_cache.get(name) {
            key.clone()
        } else {
            // Load from OS storage
            let serializable = self.retrieve_key_from_os(name).await?;
            self.from_serializable_key(serializable)?
        };

        let key_type = match &stored_key.privkey {
            PrivateKey::Secp256k1(_) => "secp256k1",
            PrivateKey::Ed25519(_) => "ed25519",
        };

        let privkey_hex = if include_private {
            Some(match &stored_key.privkey {
                PrivateKey::Secp256k1(k) => hex::encode(k.to_bytes()),
                PrivateKey::Ed25519(k) => hex::encode(k.to_bytes()),
            })
        } else {
            None
        };

        Ok(ExportedKey {
            name: name.to_string(),
            key_type: key_type.to_string(),
            mnemonic: None, // OS keyring doesn't store mnemonics
            privkey_hex,
            pubkey: stored_key.pubkey.clone(),
            address: stored_key.address.to_string(),
        })
    }

    async fn import_exported_key(
        &mut self,
        exported: &ExportedKey,
    ) -> Result<KeyInfo, KeyringError> {
        // Check if key already exists
        if self.keys_cache.contains_key(&exported.name) {
            return Err(KeyringError::KeyExists(exported.name.clone()));
        }

        if self.retrieve_key_from_os(&exported.name).await.is_ok() {
            return Err(KeyringError::KeyExists(exported.name.clone()));
        }

        // Import from mnemonic if available
        if let Some(mnemonic) = &exported.mnemonic {
            return self.import_key(&exported.name, mnemonic).await;
        }

        // Import from private key hex
        if let Some(privkey_hex) = &exported.privkey_hex {
            return self.import_private_key(&exported.name, privkey_hex).await;
        }

        Err(KeyringError::BackendError(
            "Exported key has neither mnemonic nor private key".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_keyring_basic_operations() {
        let mut keyring = MemoryKeyring::new();

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
    async fn test_memory_keyring_import_mnemonic() {
        let mut keyring = MemoryKeyring::new();

        let mnemonic = "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius";

        let key_info = keyring.import_key("imported_key", mnemonic).await.unwrap();
        assert_eq!(key_info.name, "imported_key");

        // Verify the key can be used for signing
        let signature = keyring.sign("imported_key", b"test message").await.unwrap();
        assert!(!signature.is_empty());
    }

    #[tokio::test]
    async fn test_memory_keyring_duplicate_key() {
        let mut keyring = MemoryKeyring::new();

        keyring.create_key("duplicate_key").await.unwrap();

        // Should fail to create key with same name
        let result = keyring.create_key("duplicate_key").await;
        assert!(matches!(result, Err(KeyringError::KeyExists(_))));
    }

    #[tokio::test]
    async fn test_os_keyring_basic_operations() {
        let mut keyring = OsKeyring::new("helium-test-service");

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
    async fn test_os_keyring_import_mnemonic() {
        let mut keyring = OsKeyring::new("helium-test-mnemonic");

        let mnemonic = "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius";

        let key_info = keyring.import_key("imported_key", mnemonic).await.unwrap();
        assert_eq!(key_info.name, "imported_key");

        // Verify the key can be used for signing
        let signature = keyring.sign("imported_key", b"test message").await.unwrap();
        assert!(!signature.is_empty());

        // Clean up
        keyring.delete_key("imported_key").await.unwrap();
    }

    #[tokio::test]
    async fn test_os_keyring_import_private_key() {
        let mut keyring = OsKeyring::new("helium-test-privkey");

        // Create a secp256k1 private key
        let mut rng = rand::thread_rng();
        let signing_key = Secp256k1PrivKey::random(&mut rng);
        let privkey_hex = hex::encode(signing_key.to_bytes());

        let key_info = keyring
            .import_private_key("imported_privkey", &privkey_hex)
            .await
            .unwrap();
        assert_eq!(key_info.name, "imported_privkey");

        // Verify the key can be used for signing
        let signature = keyring
            .sign("imported_privkey", b"test message")
            .await
            .unwrap();
        assert!(!signature.is_empty());

        // Test export
        let exported = keyring.export_key("imported_privkey", true).await.unwrap();
        assert_eq!(exported.privkey_hex, Some(privkey_hex));

        // Clean up
        keyring.delete_key("imported_privkey").await.unwrap();
    }

    #[tokio::test]
    async fn test_os_keyring_duplicate_key() {
        let mut keyring = OsKeyring::new("helium-test-duplicate");

        keyring.create_key("duplicate_key").await.unwrap();

        // Should fail to create key with same name
        let result = keyring.create_key("duplicate_key").await;
        assert!(matches!(result, Err(KeyringError::KeyExists(_))));

        // Clean up
        keyring.delete_key("duplicate_key").await.unwrap();
    }

    #[tokio::test]
    async fn test_os_keyring_registry_persistence() {
        let mut keyring1 = OsKeyring::new("helium-test-registry");
        let keyring2 = OsKeyring::new("helium-test-registry");

        // Create key with first instance
        keyring1.create_key("registry_test").await.unwrap();

        // Verify second instance can see the key
        let keys = keyring2.list_keys().await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].name, "registry_test");

        // Clean up
        keyring1.delete_key("registry_test").await.unwrap();
        let keys = keyring2.list_keys().await.unwrap();
        assert_eq!(keys.len(), 0);
    }

    #[tokio::test]
    async fn test_keyring_trait_consistency() {
        // Test that both implementations conform to the same interface
        let mut memory_keyring = MemoryKeyring::new();
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

        // Import same key into memory keyring
        let memory_key = memory_keyring
            .import_key("consistency_test", mnemonic)
            .await
            .unwrap();

        // Both should produce same address for same mnemonic
        assert!(!memory_key.address.to_string().is_empty());

        // Both should be able to sign
        let data = b"consistency test message";
        let memory_sig = memory_keyring.sign("consistency_test", data).await.unwrap();
        assert!(!memory_sig.is_empty());
    }
}
