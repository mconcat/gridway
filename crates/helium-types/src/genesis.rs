//! Genesis file structure and utilities for helium
//!
//! This module provides types and functions for handling blockchain genesis files,
//! including parsing, validation, and initialization.

use crate::{AccAddress, Config, SdkError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

/// Maximum allowed chain ID length
pub const MAX_CHAIN_ID_LEN: usize = 48;

/// Top-level genesis file structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppGenesis {
    /// Application name
    #[serde(default = "default_app_name")]
    pub app_name: String,

    /// Application version
    #[serde(default = "default_app_version")]
    pub app_version: String,

    /// Genesis time
    pub genesis_time: String,

    /// Chain ID
    pub chain_id: String,

    /// Initial block height
    #[serde(default = "default_initial_height")]
    pub initial_height: i64,

    /// Application hash (usually empty at genesis)
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub app_hash: Vec<u8>,

    /// Application state (module-specific genesis data)
    #[serde(default)]
    pub app_state: AppState,

    /// Consensus parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consensus: Option<ConsensusGenesis>,
}

/// Default application name
fn default_app_name() -> String {
    "helium".to_string()
}

/// Default application version
fn default_app_version() -> String {
    "0.1.0".to_string()
}

/// Default initial height
fn default_initial_height() -> i64 {
    1
}

/// Consensus-related genesis data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusGenesis {
    /// Validators
    #[serde(default)]
    pub validators: Vec<GenesisValidator>,

    /// Consensus parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<ConsensusParams>,
}

/// Genesis validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisValidator {
    /// Validator address
    pub address: String,

    /// Public key
    pub pub_key: PublicKey,

    /// Voting power
    pub power: String,

    /// Optional name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Public key structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKey {
    /// Key type (e.g., "ed25519", "secp256k1")
    #[serde(rename = "type")]
    pub key_type: String,

    /// Base64-encoded public key
    pub value: String,
}

/// Consensus parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusParams {
    /// Block parameters
    pub block: BlockParams,

    /// Evidence parameters
    pub evidence: EvidenceParams,

    /// Validator parameters
    pub validator: ValidatorParams,

    /// Version parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionParams>,
}

/// Block parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockParams {
    /// Maximum block size in bytes
    pub max_bytes: String,

    /// Maximum gas per block
    pub max_gas: String,
}

/// Evidence parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceParams {
    /// Maximum age of evidence in blocks
    pub max_age_num_blocks: String,

    /// Maximum age of evidence in time
    pub max_age_duration: String,

    /// Maximum size of evidence in bytes
    pub max_bytes: String,
}

/// Validator parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorParams {
    /// Allowed public key types
    pub pub_key_types: Vec<String>,
}

/// Version parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionParams {
    /// Application version
    pub app: String,
}

/// Application state containing module-specific genesis data
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppState {
    /// Auth module genesis state
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthGenesis>,

    /// Bank module genesis state
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bank: Option<BankGenesis>,

    /// Additional module states (for extensibility)
    #[serde(flatten)]
    pub modules: HashMap<String, Value>,
}

/// Auth module genesis state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthGenesis {
    /// Module parameters
    pub params: AuthParams,

    /// Genesis accounts
    #[serde(default)]
    pub accounts: Vec<GenesisAccount>,
}

/// Auth module parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthParams {
    /// Maximum memo length
    pub max_memo_characters: String,

    /// Transaction signature limit
    pub tx_sig_limit: String,

    /// Transaction size cost per byte
    pub tx_size_cost_per_byte: String,

    /// ED25519 signature verification cost
    pub sig_verify_cost_ed25519: String,

    /// Secp256k1 signature verification cost
    pub sig_verify_cost_secp256k1: String,
}

/// Genesis account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAccount {
    /// Account type
    #[serde(rename = "@type")]
    pub account_type: String,

    /// Account address
    pub address: String,

    /// Optional public key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pub_key: Option<PublicKey>,

    /// Account number
    pub account_number: String,

    /// Sequence number
    pub sequence: String,
}

/// Bank module genesis state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankGenesis {
    /// Module parameters
    pub params: BankParams,

    /// Account balances
    #[serde(default)]
    pub balances: Vec<Balance>,

    /// Total supply
    #[serde(default)]
    pub supply: Vec<Coin>,

    /// Denomination metadata
    #[serde(default)]
    pub denom_metadata: Vec<DenomMetadata>,

    /// Send enabled flags
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub send_enabled: Vec<SendEnabled>,
}

/// Bank module parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankParams {
    /// Whether send is enabled by default
    #[serde(default = "default_true")]
    pub default_send_enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Account balance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    /// Account address
    pub address: String,

    /// Coins held by the account
    pub coins: Vec<Coin>,
}

/// Coin amount
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Coin {
    /// Denomination
    pub denom: String,

    /// Amount
    pub amount: String,
}

/// Denomination metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DenomMetadata {
    /// Base denomination
    pub base: String,

    /// Display denomination
    pub display: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Denomination units
    #[serde(default)]
    pub denom_units: Vec<DenomUnit>,

    /// Name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Symbol
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

/// Denomination unit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DenomUnit {
    /// Denomination
    pub denom: String,

    /// Exponent
    pub exponent: u32,

    /// Aliases
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// Send enabled flag for a denomination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendEnabled {
    /// Denomination
    pub denom: String,

    /// Whether sending is enabled
    pub enabled: bool,
}

impl AppGenesis {
    /// Create a new genesis with default values
    pub fn new(chain_id: String) -> Self {
        Self {
            app_name: default_app_name(),
            app_version: default_app_version(),
            genesis_time: chrono::Utc::now().to_rfc3339(),
            chain_id,
            initial_height: default_initial_height(),
            app_hash: vec![],
            app_state: AppState::default(),
            consensus: None,
        }
    }

    /// Add an account to the genesis
    pub fn add_account(
        &mut self,
        address: String,
        pub_key: Option<PublicKey>,
    ) -> Result<(), SdkError> {
        // Validate address
        AccAddress::from_bech32(&address)
            .map_err(|_| SdkError::InvalidGenesis(format!("Invalid account address: {address}")))?;

        // Initialize auth state if not present
        if self.app_state.auth.is_none() {
            self.app_state.auth = Some(AuthGenesis {
                params: AuthParams {
                    max_memo_characters: "256".to_string(),
                    tx_sig_limit: "7".to_string(),
                    tx_size_cost_per_byte: "10".to_string(),
                    sig_verify_cost_ed25519: "590".to_string(),
                    sig_verify_cost_secp256k1: "1000".to_string(),
                },
                accounts: vec![],
            });
        }

        if let Some(auth) = &mut self.app_state.auth {
            let account_number = auth.accounts.len() as u64;
            auth.accounts.push(GenesisAccount {
                account_type: "/cosmos.auth.v1beta1.BaseAccount".to_string(),
                address,
                pub_key,
                account_number: account_number.to_string(),
                sequence: "0".to_string(),
            });
        }

        Ok(())
    }

    /// Add balance for an address
    pub fn add_balance(
        &mut self,
        address: String,
        denom: String,
        amount: String,
    ) -> Result<(), SdkError> {
        // Validate address
        AccAddress::from_bech32(&address)
            .map_err(|_| SdkError::InvalidGenesis(format!("Invalid balance address: {address}")))?;

        // Validate amount
        let amount_u128 = amount
            .parse::<u128>()
            .map_err(|_| SdkError::InvalidGenesis(format!("Invalid amount: {amount}")))?;

        if amount_u128 == 0 {
            return Err(SdkError::InvalidGenesis(
                "Amount must be positive".to_string(),
            ));
        }

        // Initialize bank state if not present
        if self.app_state.bank.is_none() {
            self.app_state.bank = Some(BankGenesis {
                params: BankParams {
                    default_send_enabled: true,
                },
                balances: vec![],
                supply: vec![],
                denom_metadata: vec![],
                send_enabled: vec![],
            });
        }

        if let Some(bank) = &mut self.app_state.bank {
            // Find or create balance entry for this address
            if let Some(balance) = bank.balances.iter_mut().find(|b| b.address == address) {
                // Add to existing balance
                if let Some(coin) = balance.coins.iter_mut().find(|c| c.denom == denom) {
                    let existing = coin.amount.parse::<u128>().map_err(|_| {
                        SdkError::InvalidGenesis("Invalid existing amount".to_string())
                    })?;
                    coin.amount = (existing + amount_u128).to_string();
                } else {
                    balance.coins.push(Coin {
                        denom: denom.clone(),
                        amount: amount.clone(),
                    });
                }
            } else {
                // Create new balance entry
                bank.balances.push(Balance {
                    address,
                    coins: vec![Coin {
                        denom: denom.clone(),
                        amount: amount.clone(),
                    }],
                });
            }

            // Update supply
            if let Some(supply_coin) = bank.supply.iter_mut().find(|c| c.denom == denom) {
                let existing = supply_coin
                    .amount
                    .parse::<u128>()
                    .map_err(|_| SdkError::InvalidGenesis("Invalid supply amount".to_string()))?;
                supply_coin.amount = (existing + amount_u128).to_string();
            } else {
                bank.supply.push(Coin { denom, amount });
            }
        }

        Ok(())
    }

    /// Get all accounts from genesis
    pub fn get_accounts(&self) -> Vec<&GenesisAccount> {
        self.app_state
            .auth
            .as_ref()
            .map(|auth| auth.accounts.iter().collect())
            .unwrap_or_default()
    }

    /// Get all balances from genesis
    pub fn get_balances(&self) -> Vec<&Balance> {
        self.app_state
            .bank
            .as_ref()
            .map(|bank| bank.balances.iter().collect())
            .unwrap_or_default()
    }

    /// Initialize genesis with default validator and faucet account
    pub fn with_default_setup(chain_id: String) -> Result<Self, SdkError> {
        let config = Config::default();
        Self::with_setup(chain_id, &config)
    }

    /// Initialize genesis with validator and faucet account from config
    pub fn with_setup(chain_id: String, config: &Config) -> Result<Self, SdkError> {
        let mut genesis = Self::new(chain_id);

        // Add a default validator account
        let validator_address = &config.genesis.validator_address;
        genesis.add_account(validator_address.clone(), None)?;
        genesis.add_balance(
            validator_address.clone(),
            config.chain.default_denom.clone(),
            config.genesis.validator_stake.clone(),
        )?;

        // Add a faucet account
        let faucet_address = &config.genesis.faucet_address;
        genesis.add_account(faucet_address.clone(), None)?;
        genesis.add_balance(
            faucet_address.clone(),
            config.chain.default_denom.clone(),
            config.genesis.faucet_stake.clone(),
        )?;
        genesis.add_balance(
            faucet_address.clone(),
            config.chain.test_denom.clone(),
            config.genesis.faucet_atom.clone(),
        )?;

        Ok(genesis)
    }

    /// Load genesis from a file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, SdkError> {
        let contents = fs::read_to_string(path)
            .map_err(|e| SdkError::InvalidGenesis(format!("Failed to read genesis file: {e}")))?;

        Self::from_json(&contents)
    }

    /// Load genesis from a reader
    pub fn from_reader<R: Read>(mut reader: R) -> Result<Self, SdkError> {
        let mut contents = String::new();
        reader
            .read_to_string(&mut contents)
            .map_err(|e| SdkError::InvalidGenesis(format!("Failed to read genesis: {e}")))?;

        Self::from_json(&contents)
    }

    /// Parse genesis from JSON string
    pub fn from_json(json_str: &str) -> Result<Self, SdkError> {
        serde_json::from_str(json_str)
            .map_err(|e| SdkError::InvalidGenesis(format!("Failed to parse genesis JSON: {e}")))
    }

    /// Save genesis to a file
    pub fn save_as<P: AsRef<Path>>(&self, path: P) -> Result<(), SdkError> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SdkError::InvalidGenesis(format!("Failed to serialize genesis: {e}")))?;

        fs::write(path, json)
            .map_err(|e| SdkError::InvalidGenesis(format!("Failed to write genesis file: {e}")))
    }

    /// Validate the genesis file
    pub fn validate(&self) -> Result<(), SdkError> {
        // Validate chain ID
        if self.chain_id.is_empty() {
            return Err(SdkError::InvalidGenesis(
                "chain_id cannot be empty".to_string(),
            ));
        }

        if self.chain_id.len() > MAX_CHAIN_ID_LEN {
            return Err(SdkError::InvalidGenesis(format!(
                "chain_id length cannot exceed {MAX_CHAIN_ID_LEN} characters"
            )));
        }

        // Validate initial height
        if self.initial_height < 0 {
            return Err(SdkError::InvalidGenesis(
                "initial_height cannot be negative".to_string(),
            ));
        }

        // Validate genesis time
        if self.genesis_time.is_empty() {
            return Err(SdkError::InvalidGenesis(
                "genesis_time cannot be empty".to_string(),
            ));
        }

        // Validate app state
        self.app_state.validate()?;

        // Validate consensus if present
        if let Some(consensus) = &self.consensus {
            consensus.validate()?;
        }

        Ok(())
    }

    /// Validate and complete the genesis with default values
    pub fn validate_and_complete(&mut self) -> Result<(), SdkError> {
        // Set defaults if needed
        if self.initial_height == 0 {
            self.initial_height = 1;
        }

        if self.genesis_time.is_empty() {
            self.genesis_time = chrono::Utc::now().to_rfc3339();
        }

        // Validate
        self.validate()
    }
}

impl AppState {
    /// Validate the application state
    pub fn validate(&self) -> Result<(), SdkError> {
        // Validate auth genesis if present
        if let Some(auth) = &self.auth {
            auth.validate()?;
        }

        // Validate bank genesis if present
        if let Some(bank) = &self.bank {
            bank.validate()?;
        }

        Ok(())
    }
}

impl AuthGenesis {
    /// Validate auth genesis state
    pub fn validate(&self) -> Result<(), SdkError> {
        // Check for duplicate accounts
        let mut seen_addresses = std::collections::HashSet::new();
        for account in &self.accounts {
            if !seen_addresses.insert(&account.address) {
                return Err(SdkError::InvalidGenesis(format!(
                    "Duplicate account address: {}",
                    account.address
                )));
            }

            // Validate address format
            AccAddress::from_bech32(&account.address).map_err(|_| {
                SdkError::InvalidGenesis(format!("Invalid account address: {}", account.address))
            })?;
        }

        Ok(())
    }
}

impl BankGenesis {
    /// Validate bank genesis state
    pub fn validate(&self) -> Result<(), SdkError> {
        // Check for duplicate balance addresses
        let mut seen_addresses = std::collections::HashSet::new();
        let mut total_supply: HashMap<String, u128> = HashMap::new();

        for balance in &self.balances {
            if !seen_addresses.insert(&balance.address) {
                return Err(SdkError::InvalidGenesis(format!(
                    "Duplicate balance address: {}",
                    balance.address
                )));
            }

            // Validate address format
            AccAddress::from_bech32(&balance.address).map_err(|_| {
                SdkError::InvalidGenesis(format!("Invalid balance address: {}", balance.address))
            })?;

            // Sum up total supply from balances
            for coin in &balance.coins {
                let amount = coin.amount.parse::<u128>().map_err(|_| {
                    SdkError::InvalidGenesis(format!("Invalid coin amount: {}", coin.amount))
                })?;

                *total_supply.entry(coin.denom.clone()).or_insert(0) += amount;
            }
        }

        // Verify supply matches sum of balances
        for supply_coin in &self.supply {
            let expected = supply_coin.amount.parse::<u128>().map_err(|_| {
                SdkError::InvalidGenesis(format!("Invalid supply amount: {}", supply_coin.amount))
            })?;

            let actual = total_supply.get(&supply_coin.denom).copied().unwrap_or(0);

            if expected != actual {
                return Err(SdkError::InvalidGenesis(format!(
                    "Supply mismatch for {}: expected {}, got {}",
                    supply_coin.denom, expected, actual
                )));
            }
        }

        Ok(())
    }
}

impl ConsensusGenesis {
    /// Validate consensus genesis
    pub fn validate(&self) -> Result<(), SdkError> {
        // Validate validators
        for validator in &self.validators {
            let power = validator.power.parse::<u64>().map_err(|_| {
                SdkError::InvalidGenesis(format!("Invalid validator power: {}", validator.power))
            })?;

            if power == 0 {
                return Err(SdkError::InvalidGenesis(format!(
                    "Validator {} has zero power",
                    validator.address
                )));
            }
        }

        Ok(())
    }
}

/// Default genesis for testing
impl Default for AppGenesis {
    fn default() -> Self {
        Self::new("test-chain".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_new() {
        let genesis = AppGenesis::new("my-chain".to_string());
        assert_eq!(genesis.chain_id, "my-chain");
        assert_eq!(genesis.app_name, "helium");
        assert_eq!(genesis.initial_height, 1);
        assert!(genesis.app_hash.is_empty());
    }

    #[test]
    fn test_genesis_validation() {
        let mut genesis = AppGenesis::new("".to_string());
        assert!(genesis.validate().is_err());

        genesis.chain_id = "valid-chain".to_string();
        assert!(genesis.validate().is_ok());

        genesis.chain_id = "a".repeat(MAX_CHAIN_ID_LEN + 1);
        assert!(genesis.validate().is_err());

        genesis.chain_id = "valid-chain".to_string();
        genesis.initial_height = -1;
        assert!(genesis.validate().is_err());
    }

    #[test]
    fn test_genesis_json_roundtrip() {
        let mut genesis = AppGenesis::new("test-chain".to_string());

        // Add some auth accounts
        genesis.app_state.auth = Some(AuthGenesis {
            params: AuthParams {
                max_memo_characters: "256".to_string(),
                tx_sig_limit: "7".to_string(),
                tx_size_cost_per_byte: "10".to_string(),
                sig_verify_cost_ed25519: "590".to_string(),
                sig_verify_cost_secp256k1: "1000".to_string(),
            },
            accounts: vec![GenesisAccount {
                account_type: "/cosmos.auth.v1beta1.BaseAccount".to_string(),
                address: "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux".to_string(),
                pub_key: None,
                account_number: "0".to_string(),
                sequence: "0".to_string(),
            }],
        });

        // Add some bank balances
        genesis.app_state.bank = Some(BankGenesis {
            params: BankParams {
                default_send_enabled: true,
            },
            balances: vec![Balance {
                address: "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux".to_string(),
                coins: vec![Coin {
                    denom: "stake".to_string(),
                    amount: "1000000".to_string(),
                }],
            }],
            supply: vec![Coin {
                denom: "stake".to_string(),
                amount: "1000000".to_string(),
            }],
            denom_metadata: vec![],
            send_enabled: vec![],
        });

        let json = serde_json::to_string_pretty(&genesis).unwrap();
        let parsed = AppGenesis::from_json(&json).unwrap();

        assert_eq!(genesis.chain_id, parsed.chain_id);
        assert_eq!(
            genesis.app_state.auth.unwrap().accounts.len(),
            parsed.app_state.auth.unwrap().accounts.len()
        );
    }

    #[test]
    fn test_bank_validation() {
        let bank = BankGenesis {
            params: BankParams {
                default_send_enabled: true,
            },
            balances: vec![Balance {
                address: "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux".to_string(),
                coins: vec![Coin {
                    denom: "stake".to_string(),
                    amount: "1000000".to_string(),
                }],
            }],
            supply: vec![Coin {
                denom: "stake".to_string(),
                amount: "999999".to_string(), // Mismatch!
            }],
            denom_metadata: vec![],
            send_enabled: vec![],
        };

        assert!(bank.validate().is_err());
    }

    #[test]
    fn test_add_account() {
        let mut genesis = AppGenesis::new("test-chain".to_string());

        // Add account
        let address = "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux";
        genesis.add_account(address.to_string(), None).unwrap();

        // Verify account was added
        let accounts = genesis.get_accounts();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].address, address);
        assert_eq!(accounts[0].account_number, "0");
        assert_eq!(accounts[0].sequence, "0");

        // Add another account
        let address2 = "cosmos1fl48vsnmsdzcv85q5d2q4z5ajdha8yu34mf0eh";
        genesis.add_account(address2.to_string(), None).unwrap();

        let accounts = genesis.get_accounts();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[1].account_number, "1");

        // Invalid address should fail
        assert!(genesis.add_account("invalid".to_string(), None).is_err());
    }

    #[test]
    fn test_add_balance() {
        let mut genesis = AppGenesis::new("test-chain".to_string());

        // Add balance
        let address = "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux";
        genesis
            .add_balance(
                address.to_string(),
                "stake".to_string(),
                "1000000".to_string(),
            )
            .unwrap();

        // Verify balance was added
        let balances = genesis.get_balances();
        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].address, address);
        assert_eq!(balances[0].coins.len(), 1);
        assert_eq!(balances[0].coins[0].denom, "stake");
        assert_eq!(balances[0].coins[0].amount, "1000000");

        // Verify supply was updated
        let bank = genesis.app_state.bank.as_ref().unwrap();
        assert_eq!(bank.supply.len(), 1);
        assert_eq!(bank.supply[0].amount, "1000000");

        // Add more balance to same address
        genesis
            .add_balance(
                address.to_string(),
                "stake".to_string(),
                "500000".to_string(),
            )
            .unwrap();
        let balances = genesis.get_balances();
        assert_eq!(balances[0].coins[0].amount, "1500000");
        let bank = genesis.app_state.bank.as_ref().unwrap();
        assert_eq!(bank.supply[0].amount, "1500000");

        // Add different denom
        genesis
            .add_balance(address.to_string(), "atom".to_string(), "100".to_string())
            .unwrap();
        let balances = genesis.get_balances();
        assert_eq!(balances[0].coins.len(), 2);

        // Invalid address should fail
        assert!(genesis
            .add_balance(
                "invalid".to_string(),
                "stake".to_string(),
                "1000".to_string()
            )
            .is_err());

        // Zero amount should fail
        assert!(genesis
            .add_balance(address.to_string(), "stake".to_string(), "0".to_string())
            .is_err());

        // Invalid amount should fail
        assert!(genesis
            .add_balance(
                address.to_string(),
                "stake".to_string(),
                "not-a-number".to_string()
            )
            .is_err());
    }

    #[test]
    fn test_with_default_setup() {
        let genesis = AppGenesis::with_default_setup("test-chain".to_string()).unwrap();

        // Check chain ID
        assert_eq!(genesis.chain_id, "test-chain");

        // Check accounts
        let accounts = genesis.get_accounts();
        assert_eq!(accounts.len(), 2);

        // Check balances
        let balances = genesis.get_balances();
        assert_eq!(balances.len(), 2);

        // Verify validator balance
        let validator_balance = balances
            .iter()
            .find(|b| b.address == "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux")
            .unwrap();
        assert_eq!(validator_balance.coins.len(), 1);
        assert_eq!(validator_balance.coins[0].denom, "stake");
        assert_eq!(validator_balance.coins[0].amount, "1000000000");

        // Verify faucet balance
        let faucet_balance = balances
            .iter()
            .find(|b| b.address == "cosmos1fl48vsnmsdzcv85q5d2q4z5ajdha8yu34mf0eh")
            .unwrap();
        assert_eq!(faucet_balance.coins.len(), 2);

        // Validate genesis
        assert!(genesis.validate().is_ok());
    }
}
