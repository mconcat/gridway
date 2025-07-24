//! Key management for gridway
//!
//! This crate provides secure key storage and management functionality
//! including HD wallet derivation and multiple backend support.

use async_trait::async_trait;
use gridway_crypto::PublicKey;
use gridway_types::address::AccAddress;
use thiserror::Error;

pub mod backends;
pub mod file;
pub mod hd;

#[derive(Error, Debug)]
pub enum KeyringError {
    #[error("key not found:: {0}")]
    KeyNotFound(String),

    #[error("key already exists:: {0}")]
    KeyExists(String),

    #[error("backend error:: {0}")]
    BackendError(String),

    #[error("invalid mnemonic")]
    InvalidMnemonic,
}

/// Information about a stored key
#[derive(Clone, Debug)]
pub struct KeyInfo {
    pub name: String,
    pub pubkey: PublicKey,
    pub address: AccAddress,
}

/// Exported key format (unencrypted)
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ExportedKey {
    /// Key name
    pub name: String,
    /// Key type
    pub key_type: String,
    /// Mnemonic phrase (if available)
    pub mnemonic: Option<String>,
    /// Private key hex (if mnemonic not available)
    pub privkey_hex: Option<String>,
    /// Public key
    pub pubkey: PublicKey,
    /// Address
    pub address: String,
}

/// Trait for keyring implementations
#[async_trait]
pub trait Keyring: Send + Sync {
    /// Create a new key with a generated mnemonic
    async fn create_key(&mut self, name: &str) -> Result<KeyInfo, KeyringError>;

    /// Import a key from a mnemonic phrase
    async fn import_key(&mut self, name: &str, mnemonic: &str) -> Result<KeyInfo, KeyringError>;

    /// Import a key from a private key hex string
    async fn import_private_key(
        &mut self,
        name: &str,
        private_key_hex: &str,
    ) -> Result<KeyInfo, KeyringError>;

    /// List all stored keys
    async fn list_keys(&self) -> Result<Vec<KeyInfo>, KeyringError>;

    /// Get a key by name
    async fn get_key(&self, name: &str) -> Result<KeyInfo, KeyringError>;

    /// Sign data with a key
    async fn sign(&self, name: &str, data: &[u8]) -> Result<Vec<u8>, KeyringError>;

    /// Delete a key
    async fn delete_key(&mut self, name: &str) -> Result<(), KeyringError>;

    /// Export a key (optionally including private key data)
    async fn export_key(
        &self,
        name: &str,
        include_private: bool,
    ) -> Result<ExportedKey, KeyringError>;

    /// Import a key from exported format
    async fn import_exported_key(
        &mut self,
        exported: &ExportedKey,
    ) -> Result<KeyInfo, KeyringError>;
}
