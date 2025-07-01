//! Simulation application framework for the helium blockchain.
//!
//! This crate provides a simulation environment for testing and benchmarking
//! helium blockchain applications.

use clap::{Parser, Subcommand};
use helium_types::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, info};

/// Simulation error types
#[derive(Error, Debug)]
pub enum SimError {
    /// Configuration error
    #[error("configuration error: {0}")]
    Config(String),

    /// Simulation error
    #[error("simulation error: {0}")]
    Simulation(String),

    /// IO error
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON error
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type for simulation operations
pub type Result<T> = std::result::Result<T, SimError>;

/// Simulation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimConfig {
    /// Number of accounts to simulate
    pub num_accounts: usize,
    /// Number of transactions per block
    pub txs_per_block: usize,
    /// Number of blocks to simulate
    pub num_blocks: usize,
    /// Block time in milliseconds
    pub block_time_ms: u64,
    /// Transaction types and their weights
    pub tx_weights: HashMap<String, u32>,
}

impl Default for SimConfig {
    fn default() -> Self {
        let mut tx_weights = HashMap::new();
        tx_weights.insert("transfer".to_string(), 80);
        tx_weights.insert("delegate".to_string(), 15);
        tx_weights.insert("vote".to_string(), 5);

        Self {
            num_accounts: 100,
            txs_per_block: 50,
            num_blocks: 1000,
            block_time_ms: 5000,
            tx_weights,
        }
    }
}

/// Simulation account
#[derive(Debug, Clone)]
pub struct Account {
    /// Account address
    pub address: String,
    /// Account balance
    pub balance: u64,
    /// Account sequence number
    pub sequence: u64,
}

impl Account {
    /// Create a new account
    pub fn new(address: String, balance: u64) -> Self {
        Self {
            address,
            balance,
            sequence: 0,
        }
    }
}

/// Transaction type
#[derive(Debug, Clone, Serialize)]
pub enum Transaction {
    /// Transfer transaction
    Transfer {
        from: String,
        to: String,
        amount: u64,
    },
    /// Delegate transaction
    Delegate {
        delegator: String,
        validator: String,
        amount: u64,
    },
    /// Vote transaction
    Vote {
        voter: String,
        proposal_id: u64,
        option: String,
    },
}

/// Block containing transactions
#[derive(Debug, Clone)]
pub struct Block {
    /// Block height
    pub height: u64,
    /// Block timestamp
    pub timestamp: Instant,
    /// Transactions in the block
    pub transactions: Vec<Transaction>,
}

/// Simulation statistics
#[derive(Clone, Debug, Serialize)]
pub struct SimStats {
    /// Total blocks processed
    pub blocks_processed: u64,
    /// Total transactions processed
    pub txs_processed: u64,
    /// Average transactions per second
    pub avg_tps: f64,
    /// Simulation duration
    pub duration_ms: u64,
    /// Transaction type counts
    pub tx_type_counts: HashMap<String, u64>,
}

/// Simulation engine
pub struct Simulator {
    config: SimConfig,
    app_config: Config,
    accounts: Vec<Account>,
    stats: SimStats,
    start_time: Option<Instant>,
}

impl Simulator {
    /// Create a new simulator
    pub fn new(config: SimConfig) -> Self {
        Self::with_app_config(config, Config::default())
    }

    /// Create a new simulator with app config
    pub fn with_app_config(config: SimConfig, app_config: Config) -> Self {
        let accounts = (0..config.num_accounts)
            .map(|i| {
                Account::new(
                    format!("account_{}", i),
                    app_config.simulation.default_balance,
                )
            })
            .collect();

        Self {
            config,
            app_config,
            accounts,
            stats: SimStats {
                blocks_processed: 0,
                txs_processed: 0,
                avg_tps: 0.0,
                duration_ms: 0,
                tx_type_counts: HashMap::new(),
            },
            start_time: None,
        }
    }

    /// Generate a random transaction
    fn generate_transaction(&self, rng: &mut fastrand::Rng) -> Transaction {
        let total_weight: u32 = self.config.tx_weights.values().sum();
        let mut random_value = rng.u32(0..total_weight);

        for (tx_type, weight) in &self.config.tx_weights {
            if random_value < *weight {
                return match tx_type.as_str() {
                    "transfer" => {
                        let from_idx = rng.usize(0..self.accounts.len());
                        let to_idx = rng.usize(0..self.accounts.len());
                        let amount = rng.u64(1..1000);

                        Transaction::Transfer {
                            from: self.accounts[from_idx].address.clone(),
                            to: self.accounts[to_idx].address.clone(),
                            amount,
                        }
                    }
                    "delegate" => {
                        let delegator_idx = rng.usize(0..self.accounts.len());
                        let amount = rng.u64(1000..10000);

                        Transaction::Delegate {
                            delegator: self.accounts[delegator_idx].address.clone(),
                            validator: self.app_config.simulation.default_validator.clone(),
                            amount,
                        }
                    }
                    "vote" => {
                        let voter_idx = rng.usize(0..self.accounts.len());
                        let proposal_id = rng.u64(1..100);
                        let options = ["yes", "no", "abstain", "no_with_veto"];
                        let option = options[rng.usize(0..options.len())];

                        Transaction::Vote {
                            voter: self.accounts[voter_idx].address.clone(),
                            proposal_id,
                            option: option.to_string(),
                        }
                    }
                    _ => unreachable!(),
                };
            }
            random_value -= weight;
        }

        unreachable!()
    }

    /// Generate a block of transactions
    fn generate_block(&self, height: u64, rng: &mut fastrand::Rng) -> Block {
        let transactions = (0..self.config.txs_per_block)
            .map(|_| self.generate_transaction(rng))
            .collect();

        Block {
            height,
            timestamp: Instant::now(),
            transactions,
        }
    }

    /// Process a block
    fn process_block(&mut self, block: Block) {
        for tx in &block.transactions {
            let tx_type = match tx {
                Transaction::Transfer { .. } => "transfer",
                Transaction::Delegate { .. } => "delegate",
                Transaction::Vote { .. } => "vote",
            };

            *self
                .stats
                .tx_type_counts
                .entry(tx_type.to_string())
                .or_insert(0) += 1;
        }

        self.stats.blocks_processed += 1;
        self.stats.txs_processed += block.transactions.len() as u64;
    }

    /// Run the simulation
    pub async fn run(&mut self) -> Result<SimStats> {
        info!(blocks = %self.config.num_blocks, "Starting simulation");

        self.start_time = Some(Instant::now());
        let mut rng = fastrand::Rng::new();

        for height in 1..=self.config.num_blocks {
            let block = self.generate_block(height as u64, &mut rng);
            self.process_block(block);

            if height % 100 == 0 {
                debug!(height = %height, "Processed blocks");
            }

            // Simulate block time
            tokio::time::sleep(Duration::from_millis(self.config.block_time_ms)).await;
        }

        let duration = self.start_time.unwrap().elapsed();
        self.stats.duration_ms = duration.as_millis() as u64;
        self.stats.avg_tps = self.stats.txs_processed as f64 / duration.as_secs_f64();

        info!("Simulation completed!");
        info!(blocks = %self.stats.blocks_processed, txs = %self.stats.txs_processed,
              avg_tps = %self.stats.avg_tps, "Simulation results");

        Ok(self.stats.clone())
    }

    /// Export simulation results
    pub fn export_results(&self, path: &str) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.stats)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

/// CLI for running simulations
#[derive(Parser)]
#[command(name = "helium-sim")]
#[command(about = "Helium blockchain simulation tool")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// CLI commands
#[derive(Subcommand)]
pub enum Commands {
    /// Run a simulation
    Run {
        /// Configuration file path
        #[arg(short, long)]
        config: Option<String>,
        /// Output file for results
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Generate a default configuration file
    Config {
        /// Output path for config file
        #[arg(short, long, default_value = "sim_config.json")]
        output: String,
    },
}

/// Run the CLI
pub async fn run_cli() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { config, output } => {
            let sim_config = if let Some(config_path) = config {
                let config_data = std::fs::read_to_string(config_path)?;
                serde_json::from_str(&config_data)?
            } else {
                SimConfig::default()
            };

            let mut simulator = Simulator::new(sim_config);
            let stats = simulator.run().await?;
            if let Some(output_path) = output {
                simulator.export_results(&output_path)?;
                info!(path = %output_path, "Results exported");
            }

            Ok(())
        }
        Commands::Config { output } => {
            let default_config = SimConfig::default();
            let json = serde_json::to_string_pretty(&default_config)?;
            std::fs::write(&output, json)?;
            info!(path = %output, "Default configuration written");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_creation() {
        let account = Account::new("test_account".to_string(), 1000);
        assert_eq!(account.address, "test_account");
        assert_eq!(account.balance, 1000);
        assert_eq!(account.sequence, 0);
    }

    #[test]
    fn test_sim_config_default() {
        let config = SimConfig::default();
        assert_eq!(config.num_accounts, 100);
        assert_eq!(config.txs_per_block, 50);
        assert_eq!(config.num_blocks, 1000);
        assert!(config.tx_weights.contains_key("transfer"));
    }

    #[tokio::test]
    async fn test_simulator_creation() {
        let config = SimConfig::default();
        let simulator = Simulator::new(config);
        assert_eq!(simulator.accounts.len(), 100);
        assert_eq!(simulator.stats.blocks_processed, 0);
    }
}
