//! Key management commands implementation for helium CLI
//!
//! This module provides the implementation for all key management operations
//! including key creation, listing, import/export, and deletion.

use crate::cli::{
    AddKeyCmd, DeleteKeyCmd, ExportKeyCmd, ImportKeyCmd, KeysAction, KeysCmd, ListKeysCmd,
    ShowKeyCmd,
};
use helium_keyring::{file::FileKeyring, Keyring, KeyringError};
use helium_log::{debug, info, warn};
use std::io::{self, Write};
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during key operations
#[derive(Error, Debug)]
pub enum KeysError {
    #[error("keyring error: {0}")]
    Keyring(#[from] KeyringError),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("user cancelled operation")]
    Cancelled,

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("key not found: {0}")]
    KeyNotFound(String),

    #[error("key already exists: {0}")]
    KeyExists(String),
}

/// Result type for key operations
pub type KeysResult<T> = Result<T, KeysError>;

/// Key management handler
pub struct KeysHandler {
    keyring_dir: PathBuf,
}

impl KeysHandler {
    /// Create a new keys handler
    pub fn new(home_dir: Option<PathBuf>) -> Self {
        let keyring_dir = if let Some(home) = home_dir {
            home.join(".helium").join("keyring")
        } else {
            FileKeyring::default_dir().unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".helium")
                    .join("keyring")
            })
        };

        Self { keyring_dir }
    }

    /// Create keyring instance with password prompt
    async fn create_keyring(&self) -> KeysResult<FileKeyring> {
        let password = prompt_password("Enter keyring password: ")?;
        let keyring = FileKeyring::new(&self.keyring_dir, password).await?;
        Ok(keyring)
    }

    /// Handle keys command
    pub async fn handle_keys(&self, cmd: KeysCmd) -> KeysResult<()> {
        match cmd.action {
            KeysAction::Add(add_cmd) => self.handle_add(add_cmd).await,
            KeysAction::List(list_cmd) => self.handle_list(list_cmd).await,
            KeysAction::Show(show_cmd) => self.handle_show(show_cmd).await,
            KeysAction::Delete(delete_cmd) => self.handle_delete(delete_cmd).await,
            KeysAction::Import(import_cmd) => self.handle_import(import_cmd).await,
            KeysAction::Export(export_cmd) => self.handle_export(export_cmd).await,
        }
    }

    /// Handle add key command
    #[tracing::instrument(skip(self))]
    async fn handle_add(&self, cmd: AddKeyCmd) -> KeysResult<()> {
        let mut keyring = self.create_keyring().await?;

        let key_info = if cmd.recover {
            // Import from mnemonic
            let mnemonic = if let Some(mnemonic) = cmd.mnemonic {
                mnemonic
            } else if cmd.interactive {
                prompt_input("Enter mnemonic phrase: ")?
            } else {
                return Err(KeysError::InvalidInput(
                    "Mnemonic required for recovery".to_string(),
                ));
            };

            keyring.import_key(&cmd.name, &mnemonic).await?
        } else {
            // Create new key
            if cmd.interactive {
                info!(name = %cmd.name, algorithm = %cmd.get_algo(), "Creating new key");
                if !confirm("Continue?")? {
                    return Err(KeysError::Cancelled);
                }
            }

            keyring.create_key(&cmd.name).await?
        };

        info!(name = %cmd.name, "Key created successfully");
        info!(address = %key_info.address, "Key address");
        debug!(pubkey = ?key_info.pubkey, "Public key");

        if !cmd.recover && cmd.interactive {
            warn!("IMPORTANT: Please save your mnemonic phrase safely!");
            warn!("This is the only way to recover your key if lost.");
            warn!("Mnemonic generation is simplified in this PoC version.");
        }

        Ok(())
    }

    /// Handle list keys command
    #[tracing::instrument(skip(self))]
    async fn handle_list(&self, cmd: ListKeysCmd) -> KeysResult<()> {
        let keyring = self.create_keyring().await?;
        let keys = keyring.list_keys().await?;

        if keys.is_empty() {
            info!("No keys found.");
            return Ok(());
        }

        info!(count = keys.len(), "Keys in keyring");
        for key in keys {
            if cmd.address {
                info!("{}", key.address);
            } else if cmd.pubkey {
                info!("{}: {:?}", key.name, key.pubkey);
            } else {
                info!("- {}: {}", key.name, key.address);
            }
        }

        Ok(())
    }

    /// Handle show key command
    #[tracing::instrument(skip(self))]
    async fn handle_show(&self, cmd: ShowKeyCmd) -> KeysResult<()> {
        let keyring = self.create_keyring().await?;

        let key_info = keyring.get_key(&cmd.name).await.map_err(|e| match e {
            KeyringError::KeyNotFound(_) => KeysError::KeyNotFound(cmd.name.clone()),
            _ => KeysError::Keyring(e),
        })?;

        if cmd.address {
            info!("{}", key_info.address);
        } else if cmd.pubkey {
            info!("{:?}", key_info.pubkey);
        } else {
            info!(name = %key_info.name, address = %key_info.address, "Key information");
            debug!(pubkey = ?key_info.pubkey, "Public key");

            if let Some(prefix) = cmd.bech {
                // For custom bech32 prefix (future enhancement)
                info!(prefix = %prefix, address = %key_info.address, "Custom bech32 format");
            }
        }

        Ok(())
    }

    /// Handle delete key command
    #[tracing::instrument(skip(self))]
    async fn handle_delete(&self, cmd: DeleteKeyCmd) -> KeysResult<()> {
        let mut keyring = self.create_keyring().await?;

        // Check if key exists
        keyring.get_key(&cmd.name).await.map_err(|e| match e {
            KeyringError::KeyNotFound(_) => KeysError::KeyNotFound(cmd.name.clone()),
            _ => KeysError::Keyring(e),
        })?;

        if !cmd.yes && !cmd.force {
            warn!(name = %cmd.name, "This will permanently delete key");
            if !confirm("Are you sure?")? {
                return Err(KeysError::Cancelled);
            }
        }

        keyring.delete_key(&cmd.name).await?;
        info!(name = %cmd.name, "Key deleted successfully");

        Ok(())
    }

    /// Handle import key command
    #[tracing::instrument(skip(self))]
    async fn handle_import(&self, cmd: ImportKeyCmd) -> KeysResult<()> {
        let mut keyring = self.create_keyring().await?;

        let key_info = if let Some(mnemonic) = cmd.mnemonic {
            keyring.import_key(&cmd.name, &mnemonic).await?
        } else if let Some(private_key) = cmd.private_key {
            keyring.import_private_key(&cmd.name, &private_key).await?
        } else {
            let mnemonic = prompt_input("Enter mnemonic phrase: ")?;
            keyring.import_key(&cmd.name, &mnemonic).await?
        };

        info!(name = %cmd.name, "Key imported successfully");
        info!(address = %key_info.address, "Key address");

        Ok(())
    }

    /// Handle export key command
    #[tracing::instrument(skip(self))]
    async fn handle_export(&self, cmd: ExportKeyCmd) -> KeysResult<()> {
        let keyring = self.create_keyring().await?;

        // Check if key exists
        keyring.get_key(&cmd.name).await.map_err(|e| match e {
            KeyringError::KeyNotFound(_) => KeysError::KeyNotFound(cmd.name.clone()),
            _ => KeysError::Keyring(e),
        })?;

        if cmd.unsafe_export_private_key {
            warn!("WARNING: This will export your private key in plain text!");
            warn!("Only do this if you understand the security implications!");
            if !confirm("Continue with private key export?")? {
                return Err(KeysError::Cancelled);
            }

            // Export with private key using the FileKeyring method
            let exported = keyring.export_key(&cmd.name, true).await?;
            warn!("Exported key data (KEEP SAFE!):");
            info!("{}", serde_json::to_string_pretty(&exported)?);
        } else if cmd.mnemonic {
            return Err(KeysError::InvalidInput(
                "Mnemonic export not yet implemented (keys weren't stored with mnemonic)"
                    .to_string(),
            ));
        } else {
            // Export public information only
            let exported = keyring.export_key(&cmd.name, false).await?;
            info!("Public key information:");
            info!("{}", serde_json::to_string_pretty(&exported)?);
        }

        Ok(())
    }
}

/// Prompt user for password input (hidden)
fn prompt_password(prompt: &str) -> KeysResult<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let password = rpassword::read_password()?;
    Ok(password)
}

/// Prompt user for text input
fn prompt_input(prompt: &str) -> KeysResult<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

/// Prompt user for confirmation
fn confirm(prompt: &str) -> KeysResult<bool> {
    print!("{prompt} [y/N]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_lowercase() == "y" || input.trim().to_lowercase() == "yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_handler() -> (KeysHandler, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let handler = KeysHandler::new(Some(temp_dir.path().to_path_buf()));
        (handler, temp_dir)
    }

    #[tokio::test]
    async fn test_keys_handler_creation() {
        let (handler, _temp_dir) = create_test_handler().await;
        assert!(handler.keyring_dir.to_string_lossy().contains(".helium"));
    }

    #[tokio::test]
    async fn test_add_key_non_interactive() {
        let (_handler, _temp_dir) = create_test_handler().await;
        let add_cmd = AddKeyCmd {
            name: "test_key".to_string(),
            interactive: false,
            recover: false,
            algo: Some("secp256k1".to_string()),
            mnemonic: None,
        };

        // This would normally prompt for password, so we can't fully test it
        // without mocking the password input. For now, just test the command parsing.
        assert_eq!(add_cmd.name, "test_key");
        assert!(!add_cmd.interactive);
        assert!(!add_cmd.recover);
    }

    #[tokio::test]
    async fn test_list_keys_empty() {
        let (_handler, _temp_dir) = create_test_handler().await;
        let list_cmd = ListKeysCmd {
            address: false,
            pubkey: false,
        };

        // Test command structure
        assert!(!list_cmd.address);
        assert!(!list_cmd.pubkey);
    }

    #[tokio::test]
    async fn test_show_key_cmd() {
        let show_cmd = ShowKeyCmd {
            name: "test_key".to_string(),
            address: true,
            pubkey: false,
            bech: None,
        };

        assert_eq!(show_cmd.name, "test_key");
        assert!(show_cmd.address);
        assert!(!show_cmd.pubkey);
        assert!(show_cmd.bech.is_none());
    }

    #[tokio::test]
    async fn test_delete_key_cmd() {
        let delete_cmd = DeleteKeyCmd {
            name: "test_key".to_string(),
            yes: false,
            force: false,
        };

        assert_eq!(delete_cmd.name, "test_key");
        assert!(!delete_cmd.yes);
        assert!(!delete_cmd.force);
    }

    #[tokio::test]
    async fn test_import_key_cmd() {
        let import_cmd = ImportKeyCmd {
            name: "imported_key".to_string(),
            mnemonic: Some("test mnemonic phrase".to_string()),
            private_key: None,
        };

        assert_eq!(import_cmd.name, "imported_key");
        assert!(import_cmd.mnemonic.is_some());
        assert!(import_cmd.private_key.is_none());
    }

    #[tokio::test]
    async fn test_export_key_cmd() {
        let export_cmd = ExportKeyCmd {
            name: "test_key".to_string(),
            mnemonic: false,
            unsafe_export_private_key: false,
        };

        assert_eq!(export_cmd.name, "test_key");
        assert!(!export_cmd.mnemonic);
        assert!(!export_cmd.unsafe_export_private_key);
    }

    #[test]
    fn test_errors() {
        let err = KeysError::KeyNotFound("test".to_string());
        assert!(err.to_string().contains("key not found: test"));

        let err = KeysError::Cancelled;
        assert!(err.to_string().contains("cancelled"));
    }
}
