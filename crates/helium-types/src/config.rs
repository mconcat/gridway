//! Configuration management for Helium blockchain
//!
//! This module provides centralized configuration management to replace
//! hardcoded values throughout the codebase.

use helium_store::StorageConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

/// Configuration errors
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Configuration file not found: {0}")]
    FileNotFound(String),
    #[error("Failed to read configuration file: {0}")]
    ReadError(String),
    #[error("Failed to parse configuration: {0}")]
    ParseError(String),
    #[error("Invalid configuration value: {0}")]
    InvalidValue(String),
    #[error("Home directory not found")]
    HomeDirectoryNotFound,
}

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub chain: ChainConfig,
    pub server: ServerConfig,
    pub client: ClientConfig,
    pub gas: GasConfig,
    pub fees: FeesConfig,
    pub auth: AuthConfig,
    pub node: NodeConfig,
    pub genesis: GenesisConfig,
    pub simulation: SimulationConfig,
    pub app: AppConfig,
    pub storage: StorageConfig,
}

/// Chain-related configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    pub id: String,
    pub network: String,
    pub test_id: String,
    pub default_denom: String,
    pub test_denom: String,
    pub min_gas_price: String,
    pub max_id_length: u32,
    pub genesis_time: String,
    pub initial_block_height: u64,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub address: String,
    pub grpc_port: u16,
    pub grpc_bind_address: String,
    pub rest_port: u16,
    pub abci_bind_address: String,
    pub max_request_size: usize,
}

/// Client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub node_url: String,
    pub rpc_endpoint: String,
    pub timeout_seconds: u64,
    pub request_timeout_seconds: u64,
}

/// Gas-related configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasConfig {
    pub default_price: String,
    pub adjustment_factor: f64,
    pub min_limit: u64,
    pub max_limit: u64,
    pub default_limit: u64,
    pub ante_handler_cost: u64,
    pub signature_verification_cost: u64,
    pub fee_deduction_cost: u64,
    pub sequence_increment_cost: u64,
    pub min_price_unit: u64,
    pub base_cost: u64,
    pub per_message_cost: u64,
    pub default_amount: u64,
}

/// Fee configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeesConfig {
    pub default_amount: String,
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub max_memo_chars: String,
    pub tx_sig_limit: String,
    pub tx_size_cost_per_byte: String,
    pub sig_verify_cost_ed25519: String,
    pub sig_verify_cost_secp256k1: String,
}

/// Node configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub id: String,
    pub moniker: String,
}

/// Genesis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub validator_address: String,
    pub faucet_address: String,
    pub validator_stake: String,
    pub faucet_stake: String,
    pub faucet_atom: String,
}

/// Simulation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    pub num_accounts: u32,
    pub txs_per_block: u32,
    pub num_blocks: u32,
    pub block_time_ms: u64,
    pub default_balance: u64,
    pub default_validator: String,
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            chain: ChainConfig {
                id: "helium-mainnet".to_string(),
                network: "helium".to_string(),
                test_id: "helium-testnet".to_string(),
                default_denom: "stake".to_string(),
                test_denom: "uatom".to_string(),
                min_gas_price: "0.001stake".to_string(),
                max_id_length: 48,
                genesis_time: "2024-01-01T00:00:00Z".to_string(),
                initial_block_height: 1,
            },
            server: ServerConfig {
                address: "127.0.0.1:26657".to_string(),
                grpc_port: 9090,
                grpc_bind_address: "[::1]:9090".to_string(),
                rest_port: 1317,
                abci_bind_address: "127.0.0.1:26658".to_string(),
                max_request_size: 1024 * 1024, // 1MB
            },
            client: ClientConfig {
                node_url: "http://localhost:26657".to_string(),
                rpc_endpoint: "http://localhost:26657".to_string(),
                timeout_seconds: 30,
                request_timeout_seconds: 30,
            },
            gas: GasConfig {
                default_price: "0.025stake".to_string(),
                adjustment_factor: 1.3,
                min_limit: 50_000,
                max_limit: 2_000_000,
                default_limit: 200_000,
                ante_handler_cost: 10_000,
                signature_verification_cost: 5_000,
                fee_deduction_cost: 3_000,
                sequence_increment_cost: 2_000,
                min_price_unit: 1,
                base_cost: 50_000,
                per_message_cost: 25_000,
                default_amount: 200_000,
            },
            fees: FeesConfig {
                default_amount: "1000stake".to_string(),
            },
            auth: AuthConfig {
                max_memo_chars: "256".to_string(),
                tx_sig_limit: "7".to_string(),
                tx_size_cost_per_byte: "10".to_string(),
                sig_verify_cost_ed25519: "590".to_string(),
                sig_verify_cost_secp256k1: "1000".to_string(),
            },
            node: NodeConfig {
                id: "node123".to_string(),
                moniker: "helium-node".to_string(),
            },
            genesis: GenesisConfig {
                validator_address: "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux".to_string(),
                faucet_address: "cosmos1fl48vsnmsdzcv85q5d2q4z5ajdha8yu34mf0eh".to_string(),
                validator_stake: "1000000000".to_string(),
                faucet_stake: "10000000000".to_string(),
                faucet_atom: "100000000".to_string(),
            },
            simulation: SimulationConfig {
                num_accounts: 100,
                txs_per_block: 50,
                num_blocks: 1000,
                block_time_ms: 5000,
                default_balance: 1_000_000,
                default_validator: "validator_1".to_string(),
            },
            app: AppConfig {
                version: "0.1.0".to_string(),
            },
            storage: StorageConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from file, falling back to default if file doesn't exist
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = Self::default_config_path()?;

        if config_path.exists() {
            Self::load_from_file(&config_path)
        } else {
            // Return default configuration if file doesn't exist
            Ok(Self::default())
        }
    }

    /// Load configuration from a specific file
    pub fn load_from_file(path: &PathBuf) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::ReadError(format!("{}: {}", path.display(), e)))?;

        let config: Config = toml::from_str(&content)
            .map_err(|e| ConfigError::ParseError(format!("{}: {}", path.display(), e)))?;

        config.validate()?;
        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<(), ConfigError> {
        let config_path = Self::default_config_path()?;

        // Create parent directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ConfigError::ReadError(format!("Failed to create config directory: {e}"))
            })?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::ParseError(format!("Failed to serialize config: {e}")))?;

        std::fs::write(&config_path, content)
            .map_err(|e| ConfigError::ReadError(format!("Failed to write config file: {e}")))?;

        Ok(())
    }

    /// Get the default configuration file path
    pub fn default_config_path() -> Result<PathBuf, ConfigError> {
        let home = dirs::home_dir().ok_or(ConfigError::HomeDirectoryNotFound)?;
        Ok(home.join(".helium").join("config.toml"))
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate chain ID length
        if self.chain.id.len() > self.chain.max_id_length as usize {
            return Err(ConfigError::InvalidValue(format!(
                "Chain ID '{}' exceeds maximum length of {}",
                self.chain.id, self.chain.max_id_length
            )));
        }

        // Validate gas limits
        if self.gas.min_limit > self.gas.max_limit {
            return Err(ConfigError::InvalidValue(
                "Gas min_limit cannot be greater than max_limit".to_string(),
            ));
        }

        if self.gas.default_limit < self.gas.min_limit
            || self.gas.default_limit > self.gas.max_limit
        {
            return Err(ConfigError::InvalidValue(
                "Gas default_limit must be between min_limit and max_limit".to_string(),
            ));
        }

        // Validate adjustment factor
        if self.gas.adjustment_factor <= 0.0 {
            return Err(ConfigError::InvalidValue(
                "Gas adjustment_factor must be positive".to_string(),
            ));
        }

        // Validate ports
        if self.server.grpc_port == 0 || self.server.rest_port == 0 {
            return Err(ConfigError::InvalidValue(
                "Server ports must be non-zero".to_string(),
            ));
        }

        Ok(())
    }

    /// Get gas price as a parsed value and denom
    pub fn parse_gas_price(&self) -> Result<(f64, String), ConfigError> {
        let price_str = &self.gas.default_price;
        let mut chars = price_str.chars().peekable();
        let mut number_part = String::new();

        // Extract number part
        while let Some(ch) = chars.peek() {
            if ch.is_ascii_digit() || *ch == '.' {
                number_part.push(*ch);
                chars.next();
            } else {
                break;
            }
        }

        // Extract denom part
        let denom_part: String = chars.collect();

        if number_part.is_empty() || denom_part.is_empty() {
            return Err(ConfigError::InvalidValue(format!(
                "Invalid gas price format: {price_str}. Expected format: '0.025stake'"
            )));
        }

        let price = number_part.parse::<f64>().map_err(|_| {
            ConfigError::InvalidValue(format!("Invalid gas price number: {number_part}"))
        })?;

        Ok((price, denom_part))
    }

    /// Get timeout as Duration
    pub fn client_timeout(&self) -> Duration {
        Duration::from_secs(self.client.timeout_seconds)
    }

    /// Get request timeout as Duration
    pub fn client_request_timeout(&self) -> Duration {
        Duration::from_secs(self.client.request_timeout_seconds)
    }

    /// Get block time as Duration
    pub fn simulation_block_time(&self) -> Duration {
        Duration::from_millis(self.simulation.block_time_ms)
    }

    /// Get server address with port
    pub fn server_full_address(&self) -> String {
        self.server.address.clone()
    }

    /// Get gRPC server address
    pub fn grpc_server_address(&self) -> String {
        format!(
            "{}:{}",
            self.server.address.split(':').next().unwrap_or("127.0.0.1"),
            self.server.grpc_port
        )
    }

    /// Get REST server address
    pub fn rest_server_address(&self) -> String {
        format!(
            "{}:{}",
            self.server.address.split(':').next().unwrap_or("127.0.0.1"),
            self.server.rest_port
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.chain.id, "helium-mainnet");
        assert_eq!(config.gas.default_limit, 200_000);
        assert_eq!(config.server.grpc_port, 9090);
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        assert!(config.validate().is_ok());

        // Test invalid gas limits
        config.gas.min_limit = 1000;
        config.gas.max_limit = 500;
        assert!(config.validate().is_err());

        // Reset and test chain ID length
        config = Config::default();
        config.chain.id = "a".repeat(100);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_gas_price_parsing() {
        let config = Config::default();
        let (price, denom) = config.parse_gas_price().unwrap();
        assert_eq!(price, 0.025);
        assert_eq!(denom, "stake");

        // Test invalid format
        let mut invalid_config = config.clone();
        invalid_config.gas.default_price = "invalid".to_string();
        assert!(invalid_config.parse_gas_price().is_err());
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed_config: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(config.chain.id, parsed_config.chain.id);
        assert_eq!(config.gas.default_limit, parsed_config.gas.default_limit);
        assert_eq!(config.storage.cache_size, parsed_config.storage.cache_size);
    }

    #[test]
    fn test_config_save_load() {
        let config = Config::default();
        let temp_file = NamedTempFile::new().unwrap();
        let temp_path = temp_file.path().to_path_buf();

        // Save config
        let content = toml::to_string_pretty(&config).unwrap();
        std::fs::write(&temp_path, content).unwrap();

        // Load config
        let loaded_config = Config::load_from_file(&temp_path).unwrap();
        assert_eq!(config.chain.id, loaded_config.chain.id);
        assert_eq!(config.server.grpc_port, loaded_config.server.grpc_port);
    }

    #[test]
    fn test_utility_methods() {
        let config = Config::default();

        assert_eq!(config.client_timeout(), Duration::from_secs(30));
        assert_eq!(config.simulation_block_time(), Duration::from_millis(5000));
        assert!(config.grpc_server_address().contains("9090"));
        assert!(config.rest_server_address().contains("1317"));
    }
}
