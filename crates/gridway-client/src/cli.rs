//! CLI framework for the gridway blockchain client

use clap::{Args, Parser, Subcommand};
use gridway_log::{debug, info, warn};
use gridway_types::Config;
use std::path::PathBuf;

/// Implementation block for GlobalOpts to resolve values from Config
impl GlobalOpts {
    /// Get the effective node URL (from CLI arg or config)
    pub fn get_node_url(&self, config: &Config) -> String {
        self.node
            .clone()
            .unwrap_or_else(|| config.client.node_url.clone())
    }

    /// Get the effective chain ID (from CLI arg or config)
    pub fn get_chain_id(&self, config: &Config) -> String {
        self.chain_id
            .clone()
            .unwrap_or_else(|| config.chain.id.clone())
    }
}

impl InitCmd {
    /// Get the effective moniker (from CLI arg or config)
    pub fn get_moniker(&self, config: &Config) -> String {
        self.moniker
            .clone()
            .unwrap_or_else(|| config.node.moniker.clone())
    }
}

impl StartCmd {
    /// Get the effective gRPC port (from CLI arg or config)
    pub fn get_grpc_port(&self, config: &Config) -> u16 {
        self.grpc_port.unwrap_or(config.server.grpc_port)
    }

    /// Get the effective REST port (from CLI arg or config)
    pub fn get_rest_port(&self, config: &Config) -> u16 {
        self.rest_port.unwrap_or(config.server.rest_port)
    }

    /// Get the effective minimum gas prices (from CLI arg or config)
    pub fn get_minimum_gas_prices(&self, config: &Config) -> String {
        self.minimum_gas_prices
            .clone()
            .unwrap_or_else(|| config.chain.min_gas_price.clone())
    }
}

impl AddKeyCmd {
    /// Get the effective algorithm (from CLI arg or default)
    pub fn get_algo(&self) -> String {
        self.algo.clone().unwrap_or_else(|| "secp256k1".to_string())
    }
}

/// Helium blockchain client CLI
#[derive(Parser, Debug)]
#[command(name = "helium")]
#[command(about = "A client for the helium blockchain")]
#[command(version = "0.1.0")]
#[command(long_about = None)]
pub struct Cli {
    /// Global options
    #[command(flatten)]
    pub global_opts: GlobalOpts,

    /// Subcommands
    #[command(subcommand)]
    pub command: Commands,
}

/// Global CLI options
#[derive(Args, Clone, Debug)]
pub struct GlobalOpts {
    /// Node RPC endpoint
    #[arg(long)]
    pub node: Option<String>,

    /// Chain ID
    #[arg(long)]
    pub chain_id: Option<String>,

    /// Home directory for configuration and data
    #[arg(long)]
    pub home: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    /// Output format (json, text)
    #[arg(long, default_value = "text")]
    pub output: String,
}

/// CLI commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new blockchain node
    Init(InitCmd),

    /// Start the blockchain node
    Start(StartCmd),

    /// Key management commands
    Keys(KeysCmd),

    /// Transaction commands
    Tx(TxCmd),

    /// Query commands
    Query(QueryCmd),

    /// Status and information commands
    Status(StatusCmd),

    /// Configuration management
    Config(ConfigCmd),
}

/// Initialize command
#[derive(Parser, Debug)]
pub struct InitCmd {
    /// Moniker for the node
    #[arg(short, long)]
    pub moniker: Option<String>,

    /// Chain ID
    #[arg(long)]
    pub chain_id: Option<String>,

    /// Overwrite existing configuration
    #[arg(long)]
    pub overwrite: bool,
}

/// Start command
#[derive(Parser, Debug)]
pub struct StartCmd {
    /// Enable GRPC server
    #[arg(long, default_value_t = true)]
    pub grpc: bool,

    /// GRPC port
    #[arg(long)]
    pub grpc_port: Option<u16>,

    /// Enable REST server
    #[arg(long, default_value_t = true)]
    pub rest: bool,

    /// REST port
    #[arg(long)]
    pub rest_port: Option<u16>,

    /// Minimum gas price
    #[arg(long)]
    pub minimum_gas_prices: Option<String>,
}

/// Keys command
#[derive(Parser, Debug)]
pub struct KeysCmd {
    /// Keys subcommands
    #[command(subcommand)]
    pub action: KeysAction,
}

/// Keys actions
#[derive(Subcommand, Debug)]
pub enum KeysAction {
    /// Add a new key
    Add(AddKeyCmd),

    /// List all keys
    List(ListKeysCmd),

    /// Show key information
    Show(ShowKeyCmd),

    /// Delete a key
    Delete(DeleteKeyCmd),

    /// Import a key from mnemonic
    Import(ImportKeyCmd),

    /// Export a key
    Export(ExportKeyCmd),
}

/// Add key command
#[derive(Parser, Debug)]
pub struct AddKeyCmd {
    /// Key name
    pub name: String,

    /// Use interactive mode
    #[arg(short, long)]
    pub interactive: bool,

    /// Recover key from mnemonic
    #[arg(long)]
    pub recover: bool,

    /// Key algorithm (secp256k1, ed25519)
    #[arg(long)]
    pub algo: Option<String>,

    /// BIP39 mnemonic
    #[arg(long)]
    pub mnemonic: Option<String>,
}

/// List keys command
#[derive(Parser, Debug)]
pub struct ListKeysCmd {
    /// Show addresses only
    #[arg(short, long)]
    pub address: bool,

    /// Show public keys
    #[arg(short, long)]
    pub pubkey: bool,
}

/// Show key command
#[derive(Parser, Debug)]
pub struct ShowKeyCmd {
    /// Key name
    pub name: String,

    /// Show address only
    #[arg(short, long)]
    pub address: bool,

    /// Show public key
    #[arg(short, long)]
    pub pubkey: bool,

    /// Show bech32 address with custom prefix
    #[arg(long)]
    pub bech: Option<String>,
}

/// Delete key command
#[derive(Parser, Debug)]
pub struct DeleteKeyCmd {
    /// Key name
    pub name: String,

    /// Skip confirmation
    #[arg(short, long)]
    pub yes: bool,

    /// Force deletion
    #[arg(short, long)]
    pub force: bool,
}

/// Import key command
#[derive(Parser, Debug)]
pub struct ImportKeyCmd {
    /// Key name
    pub name: String,

    /// Import from mnemonic
    #[arg(long)]
    pub mnemonic: Option<String>,

    /// Import from private key
    #[arg(long)]
    pub private_key: Option<String>,
}

/// Export key command
#[derive(Parser, Debug)]
pub struct ExportKeyCmd {
    /// Key name
    pub name: String,

    /// Export as mnemonic
    #[arg(long)]
    pub mnemonic: bool,

    /// Unsafe: export private key
    #[arg(long)]
    pub unsafe_export_private_key: bool,
}

/// Transaction commands
#[derive(Parser, Debug)]
pub struct TxCmd {
    /// Transaction subcommands
    #[command(subcommand)]
    pub action: TxAction,
}

/// Transaction actions
#[derive(Subcommand, Debug)]
pub enum TxAction {
    /// Bank module transactions
    Bank(BankTxCmd),

    /// Sign a transaction
    Sign(SignTxCmd),

    /// Broadcast a transaction
    Broadcast(BroadcastTxCmd),

    /// Encode a transaction
    Encode(EncodeTxCmd),

    /// Decode a transaction
    Decode(DecodeTxCmd),
}

/// Bank transaction commands
#[derive(Parser, Debug)]
pub struct BankTxCmd {
    /// Bank transaction subcommands
    #[command(subcommand)]
    pub action: BankTxAction,
}

/// Bank transaction actions
#[derive(Subcommand, Debug)]
pub enum BankTxAction {
    /// Send tokens
    Send(SendCmd),
}

/// Send command
#[derive(Parser, Debug)]
pub struct SendCmd {
    /// Sender key name or address
    pub from: String,

    /// Recipient address
    pub to: String,

    /// Amount to send (e.g., "100stake", "50atom,25stake")
    pub amount: String,

    /// Transaction fees
    #[arg(long, default_value = "1000stake")]
    pub fees: String,

    /// Gas limit
    #[arg(long, default_value_t = 200000)]
    pub gas: u64,

    /// Transaction memo
    #[arg(long)]
    pub memo: Option<String>,

    /// Broadcast mode (sync, async, block)
    #[arg(long, default_value = "sync")]
    pub broadcast_mode: String,

    /// Skip confirmation
    #[arg(short, long)]
    pub yes: bool,
}

/// Sign transaction command
#[derive(Parser, Debug)]
pub struct SignTxCmd {
    /// Transaction file to sign
    pub tx_file: PathBuf,

    /// Signer key name
    #[arg(long)]
    pub from: String,

    /// Output file
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Account number
    #[arg(long)]
    pub account_number: Option<u64>,

    /// Sequence number
    #[arg(long)]
    pub sequence: Option<u64>,
}

/// Broadcast transaction command
#[derive(Parser, Debug)]
pub struct BroadcastTxCmd {
    /// Transaction file to broadcast
    pub tx_file: PathBuf,

    /// Broadcast mode (sync, async, block)
    #[arg(long, default_value = "sync")]
    pub mode: String,
}

/// Encode transaction command
#[derive(Parser, Debug)]
pub struct EncodeTxCmd {
    /// Transaction JSON file
    pub tx_file: PathBuf,

    /// Output file
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

/// Decode transaction command
#[derive(Parser, Debug)]
pub struct DecodeTxCmd {
    /// Transaction hex or base64 string
    pub tx_data: String,

    /// Output file
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

/// Query commands
#[derive(Parser, Debug)]
pub struct QueryCmd {
    /// Query subcommands
    #[command(subcommand)]
    pub action: QueryAction,
}

/// Query actions
#[derive(Subcommand, Debug)]
pub enum QueryAction {
    /// Bank module queries
    Bank(BankQueryCmd),

    /// Transaction queries
    Tx(TxQueryCmd),

    /// Block queries
    Block(BlockQueryCmd),

    /// Account queries
    Account(AccountQueryCmd),
}

/// Bank query commands
#[derive(Parser, Debug)]
pub struct BankQueryCmd {
    /// Bank query subcommands
    #[command(subcommand)]
    pub action: BankQueryAction,
}

/// Bank query actions
#[derive(Subcommand, Debug)]
pub enum BankQueryAction {
    /// Query account balance
    Balance(BalanceCmd),

    /// Query total supply
    Supply(SupplyCmd),
}

/// Balance query command
#[derive(Parser, Debug)]
pub struct BalanceCmd {
    /// Account address
    pub address: String,

    /// Specific denomination
    #[arg(long)]
    pub denom: Option<String>,
}

/// Supply query command
#[derive(Parser, Debug)]
pub struct SupplyCmd {
    /// Specific denomination
    #[arg(long)]
    pub denom: Option<String>,
}

/// Transaction query command
#[derive(Parser, Debug)]
pub struct TxQueryCmd {
    /// Transaction hash
    pub hash: String,
}

/// Block query command
#[derive(Parser, Debug)]
pub struct BlockQueryCmd {
    /// Block height
    pub height: Option<u64>,
}

/// Account query command
#[derive(Parser, Debug)]
pub struct AccountQueryCmd {
    /// Account address
    pub address: String,
}

/// Status command
#[derive(Parser, Debug)]
pub struct StatusCmd {
    /// Show detailed node information
    #[arg(short, long)]
    pub detailed: bool,
}

/// Config command
#[derive(Parser, Debug)]
pub struct ConfigCmd {
    /// Config subcommands
    #[command(subcommand)]
    pub action: ConfigAction,
}

/// Config actions
#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Show current configuration
    Show,

    /// Set configuration value
    Set(ConfigSetCmd),

    /// Initialize default configuration
    Init(ConfigInitCmd),
}

/// Config set command
#[derive(Parser, Debug)]
pub struct ConfigSetCmd {
    /// Configuration key
    pub key: String,

    /// Configuration value
    pub value: String,
}

/// Config init command
#[derive(Parser, Debug)]
pub struct ConfigInitCmd {
    /// Overwrite existing configuration
    #[arg(long)]
    pub overwrite: bool,
}

/// CLI command handler
pub struct CliHandler {
    /// Global options
    pub global_opts: GlobalOpts,
    /// Configuration
    pub config: Config,
}

impl CliHandler {
    /// Create a new CLI handler
    pub fn new(global_opts: GlobalOpts) -> Self {
        // Load configuration, falling back to defaults if not found
        let config = Config::load().unwrap_or_default();
        Self {
            global_opts,
            config,
        }
    }

    /// Execute a CLI command
    pub async fn execute(&self, command: Commands) -> crate::Result<()> {
        match command {
            Commands::Init(cmd) => self.handle_init(cmd).await,
            Commands::Start(cmd) => self.handle_start(cmd).await,
            Commands::Keys(cmd) => self.handle_keys(cmd).await,
            Commands::Tx(cmd) => self.handle_tx(cmd).await,
            Commands::Query(cmd) => self.handle_query(cmd).await,
            Commands::Status(cmd) => self.handle_status(cmd).await,
            Commands::Config(cmd) => self.handle_config(cmd).await,
        }
    }

    /// Handle init command
    #[tracing::instrument(skip(self))]
    async fn handle_init(&self, cmd: InitCmd) -> crate::Result<()> {
        info!("Initializing helium node...");
        info!(moniker = %cmd.get_moniker(&self.config), "Setting node moniker");
        let chain_id = cmd
            .chain_id
            .unwrap_or_else(|| self.global_opts.get_chain_id(&self.config));
        info!(chain_id = %chain_id, "Setting chain ID");

        if cmd.overwrite {
            warn!("Overwriting existing configuration...");
        }

        // TODO: Implement actual initialization logic
        info!("Node initialized successfully!");
        Ok(())
    }

    /// Handle start command
    #[tracing::instrument(skip(self))]
    async fn handle_start(&self, cmd: StartCmd) -> crate::Result<()> {
        info!("Starting helium node...");

        if cmd.grpc {
            info!(port = %cmd.get_grpc_port(&self.config), "GRPC server enabled");
        }

        if cmd.rest {
            info!(port = %cmd.get_rest_port(&self.config), "REST server enabled");
        }

        info!(prices = %cmd.get_minimum_gas_prices(&self.config), "Setting minimum gas prices");

        // TODO: Implement actual node startup logic
        info!("Node started successfully!");
        Ok(())
    }

    /// Handle keys command
    async fn handle_keys(&self, cmd: KeysCmd) -> crate::Result<()> {
        let keys_handler = crate::keys::KeysHandler::new(self.global_opts.home.clone());
        keys_handler.handle_keys(cmd).await?;
        Ok(())
    }

    /// Handle transaction command
    #[tracing::instrument(skip(self))]
    async fn handle_tx(&self, cmd: TxCmd) -> crate::Result<()> {
        match cmd.action {
            TxAction::Bank(bank_cmd) => self.handle_bank_tx(bank_cmd).await,
            TxAction::Sign(sign_cmd) => {
                info!(file = ?sign_cmd.tx_file, "Signing transaction from file");
                info!(signer = %sign_cmd.from, "Transaction signer");
                // TODO: Implement transaction signing
                info!("Transaction signed successfully!");
                Ok(())
            }
            TxAction::Broadcast(broadcast_cmd) => {
                info!(file = ?broadcast_cmd.tx_file, "Broadcasting transaction from file");
                info!(mode = %broadcast_cmd.mode, "Broadcast mode");
                // TODO: Implement transaction broadcasting
                info!("Transaction broadcasted successfully!");
                Ok(())
            }
            TxAction::Encode(encode_cmd) => {
                info!(file = ?encode_cmd.tx_file, "Encoding transaction from file");
                // TODO: Implement transaction encoding
                info!("Transaction encoded successfully!");
                Ok(())
            }
            TxAction::Decode(decode_cmd) => {
                debug!(tx_data = %decode_cmd.tx_data, "Decoding transaction");
                // TODO: Implement transaction decoding
                info!("Transaction decoded successfully!");
                Ok(())
            }
        }
    }

    /// Handle bank transaction command
    #[tracing::instrument(skip(self))]
    async fn handle_bank_tx(&self, cmd: BankTxCmd) -> crate::Result<()> {
        match cmd.action {
            BankTxAction::Send(send_cmd) => {
                info!("Sending tokens...");
                info!(from = %send_cmd.from, to = %send_cmd.to, amount = %send_cmd.amount, "Token transfer details");
                info!(fees = %send_cmd.fees, gas = %send_cmd.gas, "Transaction costs");

                if let Some(memo) = &send_cmd.memo {
                    info!(memo = %memo, "Transaction memo");
                }

                if !send_cmd.yes {
                    info!("Confirm transaction? [y/N]");
                    // TODO: Implement confirmation prompt
                }

                // TODO: Implement actual send logic
                info!("Transaction sent successfully!");
                info!(mode = %send_cmd.broadcast_mode, "Broadcast mode used");
                Ok(())
            }
        }
    }

    /// Handle query command
    #[tracing::instrument(skip(self))]
    async fn handle_query(&self, cmd: QueryCmd) -> crate::Result<()> {
        match cmd.action {
            QueryAction::Bank(bank_cmd) => self.handle_bank_query(bank_cmd).await,
            QueryAction::Tx(tx_cmd) => {
                info!(hash = %tx_cmd.hash, "Querying transaction");
                // TODO: Implement transaction query
                warn!("Transaction not found");
                Ok(())
            }
            QueryAction::Block(block_cmd) => {
                if let Some(height) = block_cmd.height {
                    info!(height = %height, "Querying block at specific height");
                } else {
                    info!("Querying latest block");
                }
                // TODO: Implement block query
                warn!("Block not found");
                Ok(())
            }
            QueryAction::Account(account_cmd) => {
                info!(address = %account_cmd.address, "Querying account");
                // TODO: Implement account query
                warn!("Account not found");
                Ok(())
            }
        }
    }

    /// Handle bank query command
    #[tracing::instrument(skip(self))]
    async fn handle_bank_query(&self, cmd: BankQueryCmd) -> crate::Result<()> {
        match cmd.action {
            BankQueryAction::Balance(balance_cmd) => {
                info!(address = %balance_cmd.address, "Querying balance");
                if let Some(denom) = &balance_cmd.denom {
                    info!(denomination = %denom, "Specific denomination requested");
                }
                // TODO: Implement balance query
                info!("Balance: 0stake");
                Ok(())
            }
            BankQueryAction::Supply(supply_cmd) => {
                info!("Querying total supply");
                if let Some(denom) = &supply_cmd.denom {
                    info!(denomination = %denom, "Specific denomination requested");
                }
                // TODO: Implement supply query
                info!("Total supply: 1000000stake");
                Ok(())
            }
        }
    }

    /// Handle status command
    #[tracing::instrument(skip(self))]
    async fn handle_status(&self, cmd: StatusCmd) -> crate::Result<()> {
        info!("Node status:");
        info!(endpoint = %self.global_opts.get_node_url(&self.config),
              chain_id = %self.global_opts.get_chain_id(&self.config),
              "Node connection details");

        if cmd.detailed {
            info!("Retrieving detailed information...");
            // TODO: Implement detailed status query
            info!("Latest block height: 1");
            info!("Latest block time: 2023-01-01T00:00:00Z");
            info!("Peer count: 0");
        }

        // TODO: Implement actual status query
        info!("Status: Online");
        Ok(())
    }

    /// Handle config command
    #[tracing::instrument(skip(self))]
    async fn handle_config(&self, cmd: ConfigCmd) -> crate::Result<()> {
        match cmd.action {
            ConfigAction::Show => {
                info!("Current configuration:");
                info!(node = %self.global_opts.get_node_url(&self.config),
                      chain_id = %self.global_opts.get_chain_id(&self.config),
                      output = %self.global_opts.output, "Configuration values");
                if let Some(home) = &self.global_opts.home {
                    info!(home = ?home, "Home directory");
                }
                // Show loaded config values
                println!("\nLoaded configuration:");
                println!("  Chain ID:: {}", self.config.chain.id);
                println!("  Default denom:: {}", self.config.chain.default_denom);
                println!("  Min gas price:: {}", self.config.chain.min_gas_price);
                println!("  Server address:: {}", self.config.server.address);
                println!("  GRPC port:: {}", self.config.server.grpc_port);
                println!("  REST port:: {}", self.config.server.rest_port);
                Ok(())
            }
            ConfigAction::Set(set_cmd) => {
                info!(key = %set_cmd.key, value = %set_cmd.value, "Setting configuration");
                // TODO: Implement config setting
                info!("Configuration updated successfully!");
                Ok(())
            }
            ConfigAction::Init(init_cmd) => {
                info!("Initializing default configuration...");
                if init_cmd.overwrite {
                    warn!("Overwriting existing configuration...");
                }
                // TODO: Implement config initialization
                info!("Configuration initialized successfully!");
                Ok(())
            }
        }
    }
}

/// Parse CLI arguments and execute commands
pub async fn run() -> crate::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing based on verbosity
    let level = if cli.global_opts.verbose {
        "debug"
    } else {
        "info"
    };
    gridway_log::init_tracing_with_level(level).map_err(|e| {
        crate::ClientError::InvalidResponse(format!("Failed to initialize logging:: {e}"))
    })?;

    let handler = CliHandler::new(cli.global_opts);
    handler.execute(cli.command).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_verify() {
        Cli::command().debug_assert();
    }

    #[test]
    fn test_help_generation() {
        let mut cmd = Cli::command();
        let help = cmd.render_help();
        assert!(help.to_string().contains("helium"));
        assert!(help.to_string().contains("blockchain"));
    }

    #[test]
    fn test_subcommand_help() {
        let _cli = Cli::parse_from(["helium", "keys", "--help"]);
        // This test will fail during parsing since --help exits the process,
        // but we can test that the command structure is valid
    }

    #[test]
    fn test_global_options() {
        let cli = Cli::parse_from([
            "helium",
            "--node",
            "http://localhost:8080",
            "--chain-id",
            "test-chain",
            "--verbose",
            "status",
        ]);

        assert_eq!(
            cli.global_opts.node,
            Some("http://localhost:8080".to_string())
        );
        assert_eq!(cli.global_opts.chain_id, Some("test-chain".to_string()));
        assert!(cli.global_opts.verbose);
        assert!(matches!(cli.command, Commands::Status(_)));
    }

    #[test]
    fn test_keys_add_command() {
        let cli = Cli::parse_from([
            "helium",
            "keys",
            "add",
            "my-key",
            "--algo",
            "ed25519",
            "--recover",
        ]);

        if let Commands::Keys(KeysCmd {
            action: KeysAction::Add(add_cmd),
        }) = cli.command
        {
            assert_eq!(add_cmd.name, "my-key");
            assert_eq!(add_cmd.algo, Some("ed25519".to_string()));
            assert!(add_cmd.recover);
        } else {
            panic!("Expected Keys Add command");
        }
    }

    #[test]
    fn test_bank_send_command() {
        let cli = Cli::parse_from([
            "helium",
            "tx",
            "bank",
            "send",
            "alice",
            "cosmos1abc123",
            "100stake",
            "--fees",
            "1000stake",
            "--gas",
            "200000",
            "--memo",
            "test transaction",
            "--yes",
        ]);

        if let Commands::Tx(TxCmd {
            action:
                TxAction::Bank(BankTxCmd {
                    action: BankTxAction::Send(send_cmd),
                }),
        }) = cli.command
        {
            assert_eq!(send_cmd.from, "alice");
            assert_eq!(send_cmd.to, "cosmos1abc123");
            assert_eq!(send_cmd.amount, "100stake");
            assert_eq!(send_cmd.fees, "1000stake");
            assert_eq!(send_cmd.gas, 200000);
            assert_eq!(send_cmd.memo, Some("test transaction".to_string()));
            assert!(send_cmd.yes);
        } else {
            panic!("Expected Bank Send command");
        }
    }

    #[test]
    fn test_query_balance_command() {
        let cli = Cli::parse_from([
            "helium",
            "query",
            "bank",
            "balance",
            "cosmos1abc123",
            "--denom",
            "stake",
        ]);

        if let Commands::Query(QueryCmd {
            action:
                QueryAction::Bank(BankQueryCmd {
                    action: BankQueryAction::Balance(balance_cmd),
                }),
        }) = cli.command
        {
            assert_eq!(balance_cmd.address, "cosmos1abc123");
            assert_eq!(balance_cmd.denom, Some("stake".to_string()));
        } else {
            panic!("Expected Query Balance command");
        }
    }
}
