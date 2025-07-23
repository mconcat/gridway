//! Configuration management for the helium client

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Configuration error types
#[derive(Error, Debug)]
pub enum ConfigError {
    /// I/O error
    #[error("io error:: {0}")]
    Io(#[from] std::io::Error),

    /// TOML parsing error
    #[error("toml parsing error:: {0}")]
    Toml(#[from] toml::de::Error),

    /// TOML serialization error
    #[error("toml serialization error:: {0}")]
    TomlSer(#[from] toml::ser::Error),
}

/// Client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    /// Node RPC endpoint
    pub node: String,
    /// Chain ID
    pub chain_id: String,
    /// Output format (json, text)
    pub output: String,
    /// Request timeout in seconds
    pub timeout: u64,
    /// Keyring backend (file, memory, os)
    pub keyring_backend: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            node: "http://localhost:26657".to_string(),
            chain_id: "helium-chain".to_string(),
            output: "text".to_string(),
            timeout: 30,
            keyring_backend: "file".to_string(),
        }
    }
}

impl ClientConfig {
    /// Load configuration from file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let config: ClientConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), ConfigError> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Get default configuration directory
    pub fn default_config_dir() -> PathBuf {
        if let Some(home) = dirs::home_dir() {
            home.join(".helium")
        } else {
            PathBuf::from(".helium")
        }
    }

    /// Get default configuration file path
    pub fn default_config_file() -> PathBuf {
        Self::default_config_dir().join("config.toml")
    }

    /// Load configuration from default location or create default
    pub fn load_or_default() -> Result<Self, ConfigError> {
        let config_path = Self::default_config_file();

        if config_path.exists() {
            Self::load_from_file(config_path)
        } else {
            Ok(Self::default())
        }
    }

    /// Initialize configuration directory and file
    pub fn init(overwrite: bool) -> Result<Self, ConfigError> {
        let config_dir = Self::default_config_dir();
        let config_file = Self::default_config_file();

        // Create config directory if it doesn't exist
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)?;
        }

        // Check if config file exists
        if config_file.exists() && !overwrite {
            return Self::load_from_file(config_file);
        }

        // Create default configuration
        let config = Self::default();
        config.save_to_file(config_file)?;
        Ok(config)
    }

    /// Set a configuration value
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), ConfigError> {
        match key {
            "node" => self.node = value.to_string(),
            "chain_id" => self.chain_id = value.to_string(),
            "output" => self.output = value.to_string(),
            "timeout" => {
                self.timeout = value.parse().map_err(|_| {
                    ConfigError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Invalid timeout value",
                    ))
                })?;
            }
            "keyring_backend" => self.keyring_backend = value.to_string(),
            _ => {
                return Err(ConfigError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Unknown configuration key:: {key}"),
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_config() {
        let config = ClientConfig::default();
        assert_eq!(config.node, "http://localhost:26657");
        assert_eq!(config.chain_id, "helium-chain");
        assert_eq!(config.output, "text");
        assert_eq!(config.timeout, 30);
        assert_eq!(config.keyring_backend, "file");
    }

    #[test]
    fn test_save_and_load_config() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let original_config = ClientConfig {
            node: "http://localhost:8080".to_string(),
            chain_id: "test-chain".to_string(),
            output: "json".to_string(),
            timeout: 60,
            keyring_backend: "memory".to_string(),
        };

        // Save config
        original_config.save_to_file(&config_path).unwrap();

        // Load config
        let loaded_config = ClientConfig::load_from_file(&config_path).unwrap();

        assert_eq!(loaded_config.node, original_config.node);
        assert_eq!(loaded_config.chain_id, original_config.chain_id);
        assert_eq!(loaded_config.output, original_config.output);
        assert_eq!(loaded_config.timeout, original_config.timeout);
        assert_eq!(
            loaded_config.keyring_backend,
            original_config.keyring_backend
        );
    }

    #[test]
    fn test_set_config_values() {
        let mut config = ClientConfig::default();

        config.set("node", "http://localhost:8080").unwrap();
        assert_eq!(config.node, "http://localhost:8080");

        config.set("chain_id", "test-chain").unwrap();
        assert_eq!(config.chain_id, "test-chain");

        config.set("timeout", "60").unwrap();
        assert_eq!(config.timeout, 60);

        // Test invalid key
        assert!(config.set("invalid_key", "value").is_err());

        // Test invalid timeout
        assert!(config.set("timeout", "invalid").is_err());
    }
}
