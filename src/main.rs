use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use clap::{Parser, Subcommand};
use helium_crypto::PrivateKey;
use helium_keyring::{backends::MemoryKeyring, Keyring};
use k256::ecdsa::SigningKey;
use rand::RngCore;
use std::fs::OpenOptions;
#[cfg(unix)]
use std::fs::Permissions;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "helium", about = "Helium blockchain node", version, author)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Initialize node configuration and data directory")]
    Init {
        #[arg(long, value_name = "ID", help = "Chain ID")]
        chain_id: String,

        #[arg(
            long,
            value_name = "DIR",
            help = "Home directory for configuration and data"
        )]
        home: Option<PathBuf>,

        #[arg(long, value_name = "FILE", help = "Genesis file path")]
        genesis: Option<PathBuf>,
    },

    #[command(about = "Start the node and connect to network")]
    Start {
        #[arg(
            long,
            value_name = "DIR",
            help = "Home directory for configuration and data"
        )]
        home: Option<PathBuf>,

        #[arg(long, value_name = "FILE", help = "Configuration file path")]
        config: Option<PathBuf>,

        #[arg(
            long,
            value_name = "LEVEL",
            help = "Log level (trace, debug, info, warn, error)"
        )]
        log_level: Option<String>,
    },

    #[command(about = "Display version information")]
    Version,

    #[command(about = "Genesis file utilities")]
    Genesis {
        #[command(subcommand)]
        command: GenesisCommands,
    },

    #[command(about = "Key management utilities")]
    Keys {
        #[command(subcommand)]
        command: KeysCommands,
    },

    #[command(about = "Configuration management")]
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Subcommand)]
enum GenesisCommands {
    #[command(about = "Validate genesis file")]
    Validate {
        #[arg(value_name = "FILE", help = "Genesis file path")]
        file: PathBuf,
    },

    #[command(about = "Export genesis state")]
    Export {
        #[arg(long, value_name = "DIR", help = "Home directory")]
        home: Option<PathBuf>,

        #[arg(long, value_name = "FILE", help = "Output file path")]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum KeysCommands {
    #[command(about = "Add a new key")]
    Add {
        #[arg(value_name = "NAME", help = "Key name")]
        name: String,

        #[arg(long, help = "Recover key from mnemonic")]
        recover: bool,
    },

    #[command(about = "List all keys")]
    List,

    #[command(about = "Show key details")]
    Show {
        #[arg(value_name = "NAME", help = "Key name")]
        name: String,
    },

    #[command(about = "Delete a key")]
    Delete {
        #[arg(value_name = "NAME", help = "Key name")]
        name: String,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    #[command(about = "Show current configuration")]
    Show {
        #[arg(long, value_name = "DIR", help = "Home directory")]
        home: Option<PathBuf>,
    },

    #[command(about = "Validate configuration")]
    Validate {
        #[arg(value_name = "FILE", help = "Configuration file path")]
        file: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init {
            chain_id,
            home,
            genesis,
        } => init_command(chain_id, home, genesis).await,
        Commands::Start {
            home,
            config,
            log_level,
        } => start_command(home, config, log_level).await,
        Commands::Version => version_command(),
        Commands::Genesis { command } => genesis_command(command).await,
        Commands::Keys { command } => keys_command(command).await,
        Commands::Config { command } => config_command(command).await,
    }
}

async fn init_command(
    chain_id: String,
    home: Option<PathBuf>,
    genesis: Option<PathBuf>,
) -> Result<()> {
    setup_logging(None)?;

    let home_dir = get_home_dir(home)?;

    tracing::info!("Initializing node with chain-id: {}", chain_id);
    tracing::info!("Home directory: {}", home_dir.display());

    // Create directory structure
    create_directories(&home_dir)?;

    // Initialize configuration files
    init_config_files(&home_dir, &chain_id)?;

    // Generate node key if not exists
    init_node_key(&home_dir)?;

    // Validate and copy genesis file if provided
    if let Some(genesis_path) = genesis {
        init_genesis_file(&home_dir, &genesis_path)?;
    }

    tracing::info!("Node initialized successfully");
    Ok(())
}

async fn start_command(
    home: Option<PathBuf>,
    config: Option<PathBuf>,
    log_level: Option<String>,
) -> Result<()> {
    setup_logging(log_level)?;

    let home_dir = get_home_dir(home)?;
    let config_path = config.unwrap_or_else(|| home_dir.join("config").join("app.toml"));

    // Validate config file exists
    if !config_path.exists() {
        return Err(anyhow::anyhow!(
            "Configuration file not found at: {}. Run 'helium init' first.",
            config_path.display()
        ));
    }

    tracing::info!("Starting node...");
    tracing::info!("Home directory: {}", home_dir.display());
    tracing::info!("Config file: {}", config_path.display());

    // Load configuration
    let config = load_config(&config_path)?;

    // Initialize storage backends
    init_storage(&home_dir, &config)?;

    // Load WASM modules
    load_wasm_modules(&config)?;

    // Start ABCI server
    start_abci_server(&home_dir, &config).await?;

    Ok(())
}

fn version_command() -> Result<()> {
    println!("helium {}", env!("CARGO_PKG_VERSION"));
    println!("build: {}", env!("CARGO_PKG_NAME"));
    Ok(())
}

async fn genesis_command(command: GenesisCommands) -> Result<()> {
    setup_logging(None)?;

    match command {
        GenesisCommands::Validate { file } => {
            validate_genesis_file(&file)?;
            tracing::info!("Genesis file is valid");
        }
        GenesisCommands::Export { home, output } => {
            let home_dir = get_home_dir(home)?;
            let output_path = output.unwrap_or_else(|| PathBuf::from("genesis.json"));
            export_genesis(&home_dir, &output_path)?;
            tracing::info!("Genesis exported to: {}", output_path.display());
        }
    }
    Ok(())
}

async fn keys_command(command: KeysCommands) -> Result<()> {
    setup_logging(None)?;

    match command {
        KeysCommands::Add { name, recover } => {
            if recover {
                recover_key(&name).await?;
            } else {
                add_new_key(&name).await?;
            }
        }
        KeysCommands::List => {
            list_keys().await?;
        }
        KeysCommands::Show { name } => {
            show_key(&name).await?;
        }
        KeysCommands::Delete { name } => {
            delete_key(&name).await?;
        }
    }
    Ok(())
}

async fn config_command(command: ConfigCommands) -> Result<()> {
    setup_logging(None)?;

    match command {
        ConfigCommands::Show { home } => {
            let home_dir = get_home_dir(home)?;
            show_config(&home_dir)?;
        }
        ConfigCommands::Validate { file } => {
            validate_config(&file)?;
            tracing::info!("Configuration is valid");
        }
    }
    Ok(())
}

// Helper functions

fn setup_logging(log_level: Option<String>) -> Result<()> {
    let filter = if let Some(level) = log_level {
        EnvFilter::try_new(level)?
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .init();

    Ok(())
}

fn get_home_dir(home: Option<PathBuf>) -> Result<PathBuf> {
    home.or_else(|| {
        directories::ProjectDirs::from("com", "helium", "helium")
            .map(|dirs| dirs.data_dir().to_path_buf())
    })
    .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))
}

fn create_directories(home_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(home_dir)?;
    std::fs::create_dir_all(home_dir.join("config"))?;
    std::fs::create_dir_all(home_dir.join("data"))?;
    std::fs::create_dir_all(home_dir.join("wasm_modules"))?;
    Ok(())
}

fn init_config_files(home_dir: &Path, chain_id: &str) -> Result<()> {
    let app_config_path = home_dir.join("config").join("app.toml");
    let config_path = home_dir.join("config").join("config.toml");

    if !app_config_path.exists() {
        let app_config = generate_app_config(chain_id);
        std::fs::write(&app_config_path, app_config)?;
        tracing::info!("Created app configuration: {}", app_config_path.display());
    }

    if !config_path.exists() {
        let config = generate_config();
        std::fs::write(&config_path, config)?;
        tracing::info!("Created node configuration: {}", config_path.display());
    }

    Ok(())
}

fn generate_app_config(chain_id: &str) -> String {
    format!(
        r#"# Application Configuration
chain_id = "{chain_id}"

[app]
minimum_gas_prices = "0.025uhelium"
pruning = "default"
halt_height = 0

[api]
enable = true
address = "tcp://0.0.0.0:1317"
max_open_connections = 1000
rpc_read_timeout = 10
rpc_write_timeout = 0

[grpc]
enable = true
address = "0.0.0.0:9090"

[wasm]
modules_dir = "./wasm_modules"
cache_size = 100
memory_limit = "512MB"
"#
    )
}

fn generate_config() -> String {
    r#"# CometBFT Configuration
proxy_app = "tcp://127.0.0.1:26658"
moniker = "helium-node"

[rpc]
laddr = "tcp://127.0.0.1:26657"

[p2p]
laddr = "tcp://0.0.0.0:26656"
persistent_peers = ""
"#
    .to_string()
}

fn init_node_key(home_dir: &Path) -> Result<()> {
    let key_path = home_dir.join("config").join("node_key.json");
    if !key_path.exists() {
        // Generate actual node key using helium-crypto
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);

        let signing_key = SigningKey::from_slice(&bytes)?;
        let private_key_bytes = signing_key.to_bytes();
        let private_key = PrivateKey::Secp256k1(signing_key);
        let public_key = private_key.public_key();

        let node_key = serde_json::json!({
            "priv_key": {
                "type": "secp256k1",
                "value": general_purpose::STANDARD.encode(private_key_bytes)
            },
            "pub_key": {
                "type": "secp256k1",
                "value": general_purpose::STANDARD.encode(public_key.to_bytes())
            }
        });

        // Create file with secure permissions (600 - owner read/write only)
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&key_path)?;

        // Set secure permissions before writing (Unix only)
        #[cfg(unix)]
        file.set_permissions(Permissions::from_mode(0o600))?;

        // Write the key data
        use std::io::Write;
        writeln!(file, "{}", serde_json::to_string_pretty(&node_key)?)?;

        tracing::info!("Generated secure node key: {}", key_path.display());
    }
    Ok(())
}

fn init_genesis_file(home_dir: &Path, genesis_path: &Path) -> Result<()> {
    let dest_path = home_dir.join("config").join("genesis.json");

    // Validate genesis file
    validate_genesis_file(genesis_path)?;

    // Copy to config directory
    std::fs::copy(genesis_path, &dest_path)?;
    tracing::info!("Genesis file copied to: {}", dest_path.display());

    Ok(())
}

fn validate_genesis_file(path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let _: serde_json::Value = serde_json::from_str(&content)?;
    // TODO: Add proper genesis validation using helium types
    Ok(())
}

fn export_genesis(home_dir: &Path, output_path: &Path) -> Result<()> {
    let genesis_path = home_dir.join("config").join("genesis.json");

    // Check if genesis file exists
    if !genesis_path.exists() {
        return Err(anyhow::anyhow!(
            "Genesis file not found at: {}. Initialize the node first or provide a genesis file.",
            genesis_path.display()
        ));
    }

    // Read and validate the genesis file
    let genesis_content = std::fs::read_to_string(&genesis_path)?;
    let _: serde_json::Value = serde_json::from_str(&genesis_content)?;

    // Copy genesis to output path
    std::fs::copy(&genesis_path, output_path)?;

    tracing::info!(
        "Genesis exported from {} to {}",
        genesis_path.display(),
        output_path.display()
    );
    Ok(())
}

fn load_config(path: &Path) -> Result<AppConfig> {
    let content = std::fs::read_to_string(path)?;
    let config: AppConfig = toml::from_str(&content)?;
    Ok(config)
}

fn init_storage(_home_dir: &Path, _config: &AppConfig) -> Result<()> {
    // TODO: Initialize storage backends using helium-store
    tracing::info!("Storage backends initialized");
    Ok(())
}

fn load_wasm_modules(_config: &AppConfig) -> Result<()> {
    // TODO: Load WASM modules using wasmtime
    tracing::info!("WASM modules loaded");
    Ok(())
}

async fn start_abci_server(_home_dir: &Path, _config: &AppConfig) -> Result<()> {
    // TODO: Start ABCI server using helium-server
    tracing::info!("ABCI server started");

    // Keep the server running
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down...");

    Ok(())
}

// Key management functions

async fn add_new_key(name: &str) -> Result<()> {
    let mut keyring = get_keyring().await?;

    match keyring.create_key(name).await {
        Ok(key_info) => {
            println!("Key '{name}' added successfully");
            println!("Address: {}", key_info.address);
            println!("Public Key: {:?}", key_info.pubkey);
            Ok(())
        }
        Err(helium_keyring::KeyringError::KeyExists(_)) => {
            Err(anyhow::anyhow!("Key '{}' already exists", name))
        }
        Err(e) => Err(anyhow::anyhow!("Failed to create key: {}", e)),
    }
}

async fn recover_key(name: &str) -> Result<()> {
    use std::io::{self, Write};

    print!("Enter mnemonic phrase: ");
    io::stdout().flush()?;

    let mut mnemonic = String::new();
    io::stdin().read_line(&mut mnemonic)?;
    let mnemonic = mnemonic.trim();

    let mut keyring = get_keyring().await?;

    match keyring.import_key(name, mnemonic).await {
        Ok(key_info) => {
            println!("Key '{name}' recovered successfully");
            println!("Address: {}", key_info.address);
            println!("Public Key: {:?}", key_info.pubkey);
            Ok(())
        }
        Err(helium_keyring::KeyringError::KeyExists(_)) => {
            Err(anyhow::anyhow!("Key '{}' already exists", name))
        }
        Err(helium_keyring::KeyringError::InvalidMnemonic) => {
            Err(anyhow::anyhow!("Invalid mnemonic phrase"))
        }
        Err(e) => Err(anyhow::anyhow!("Failed to recover key: {}", e)),
    }
}

async fn list_keys() -> Result<()> {
    let keyring = get_keyring().await?;

    match keyring.list_keys().await {
        Ok(keys) => {
            if keys.is_empty() {
                println!("No keys found");
            } else {
                println!("{:<20} {:<10} ADDRESS", "NAME", "TYPE");
                println!("{}", "-".repeat(60));
                for key in keys {
                    let key_type = match key.pubkey {
                        helium_crypto::PublicKey::Secp256k1(_) => "secp256k1",
                        helium_crypto::PublicKey::Ed25519(_) => "ed25519",
                    };
                    println!("{:<20} {:<10} {}", key.name, key_type, key.address);
                }
            }
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("Failed to list keys: {}", e)),
    }
}

async fn show_key(name: &str) -> Result<()> {
    let keyring = get_keyring().await?;

    match keyring.get_key(name).await {
        Ok(key_info) => {
            let key_type = match key_info.pubkey {
                helium_crypto::PublicKey::Secp256k1(_) => "secp256k1",
                helium_crypto::PublicKey::Ed25519(_) => "ed25519",
            };

            println!("Key details for '{name}':");
            println!("  Name: {}", key_info.name);
            println!("  Type: {key_type}");
            println!("  Address: {}", key_info.address);
            println!(
                "  Public Key: {}",
                serde_json::to_string_pretty(&key_info.pubkey)?
            );
            Ok(())
        }
        Err(helium_keyring::KeyringError::KeyNotFound(_)) => {
            Err(anyhow::anyhow!("Key '{}' not found", name))
        }
        Err(e) => Err(anyhow::anyhow!("Failed to get key: {}", e)),
    }
}

async fn delete_key(name: &str) -> Result<()> {
    use std::io::{self, Write};

    print!("Are you sure you want to delete key '{name}'? [y/N]: ");
    io::stdout().flush()?;

    let mut confirmation = String::new();
    io::stdin().read_line(&mut confirmation)?;
    let confirmation = confirmation.trim().to_lowercase();

    if confirmation != "y" && confirmation != "yes" {
        println!("Key deletion cancelled");
        return Ok(());
    }

    let mut keyring = get_keyring().await?;

    match keyring.delete_key(name).await {
        Ok(()) => {
            println!("Key '{name}' deleted successfully");
            Ok(())
        }
        Err(helium_keyring::KeyringError::KeyNotFound(_)) => {
            Err(anyhow::anyhow!("Key '{}' not found", name))
        }
        Err(e) => Err(anyhow::anyhow!("Failed to delete key: {}", e)),
    }
}

// Keyring helper function
async fn get_keyring() -> Result<MemoryKeyring> {
    // TODO: Replace with persistent keyring (FileKeyring or OsKeyring)
    // MemoryKeyring doesn't persist keys between CLI invocations
    // This is a temporary solution for now
    Ok(MemoryKeyring::new())
}

// Config functions

fn show_config(home_dir: &Path) -> Result<()> {
    let app_config_path = home_dir.join("config").join("app.toml");
    let content = std::fs::read_to_string(app_config_path)?;
    println!("{content}");
    Ok(())
}

fn validate_config(path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let _: AppConfig = toml::from_str(&content)?;
    Ok(())
}

// Configuration structures

#[derive(Debug, serde::Deserialize)]
struct AppConfig {
    chain_id: String,
    app: AppSection,
    api: ApiSection,
    grpc: GrpcSection,
    wasm: WasmSection,
}

#[derive(Debug, serde::Deserialize)]
struct AppSection {
    minimum_gas_prices: String,
    pruning: String,
    halt_height: u64,
}

#[derive(Debug, serde::Deserialize)]
struct ApiSection {
    enable: bool,
    address: String,
    max_open_connections: u32,
    rpc_read_timeout: u32,
    rpc_write_timeout: u32,
}

#[derive(Debug, serde::Deserialize)]
struct GrpcSection {
    enable: bool,
    address: String,
}

#[derive(Debug, Clone)]
struct MemoryLimit(u64);

impl<'de> serde::Deserialize<'de> for MemoryLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let limit = parse_memory_limit(&s).map_err(serde::de::Error::custom)?;
        Ok(MemoryLimit(limit))
    }
}

#[derive(Debug, serde::Deserialize)]
struct WasmSection {
    modules_dir: String,
    cache_size: u32,
    memory_limit: MemoryLimit,
}

fn parse_memory_limit(s: &str) -> Result<u64, String> {
    let s = s.trim().to_uppercase();

    if s.ends_with("KB") {
        let num: u64 = s[..s.len() - 2]
            .parse()
            .map_err(|e| format!("Invalid number: {e}"))?;
        Ok(num * 1024)
    } else if s.ends_with("MB") {
        let num: u64 = s[..s.len() - 2]
            .parse()
            .map_err(|e| format!("Invalid number: {e}"))?;
        Ok(num * 1024 * 1024)
    } else if s.ends_with("GB") {
        let num: u64 = s[..s.len() - 2]
            .parse()
            .map_err(|e| format!("Invalid number: {e}"))?;
        Ok(num * 1024 * 1024 * 1024)
    } else if s.ends_with("B") {
        let num: u64 = s[..s.len() - 1]
            .parse()
            .map_err(|e| format!("Invalid number: {e}"))?;
        Ok(num)
    } else {
        // Assume bytes if no unit
        s.parse()
            .map_err(|e| format!("Invalid memory limit format: {e}"))
    }
}
