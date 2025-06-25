use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "helium",
    about = "Helium blockchain node",
    version,
    author
)]
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
        
        #[arg(long, value_name = "DIR", help = "Home directory for configuration and data")]
        home: Option<PathBuf>,
        
        #[arg(long, value_name = "FILE", help = "Genesis file path")]
        genesis: Option<PathBuf>,
    },
    
    #[command(about = "Start the node and connect to network")]
    Start {
        #[arg(long, value_name = "DIR", help = "Home directory for configuration and data")]
        home: Option<PathBuf>,
        
        #[arg(long, value_name = "FILE", help = "Configuration file path")]
        config: Option<PathBuf>,
        
        #[arg(long, value_name = "LEVEL", help = "Log level (trace, debug, info, warn, error)")]
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
        Commands::Init { chain_id, home, genesis } => {
            init_command(chain_id, home, genesis).await
        }
        Commands::Start { home, config, log_level } => {
            start_command(home, config, log_level).await
        }
        Commands::Version => {
            version_command()
        }
        Commands::Genesis { command } => {
            genesis_command(command).await
        }
        Commands::Keys { command } => {
            keys_command(command).await
        }
        Commands::Config { command } => {
            config_command(command).await
        }
    }
}

async fn init_command(chain_id: String, home: Option<PathBuf>, genesis: Option<PathBuf>) -> Result<()> {
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

async fn start_command(home: Option<PathBuf>, config: Option<PathBuf>, log_level: Option<String>) -> Result<()> {
    setup_logging(log_level)?;
    
    let home_dir = get_home_dir(home)?;
    let config_path = config.unwrap_or_else(|| home_dir.join("config").join("app.toml"));
    
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
                recover_key(&name)?;
            } else {
                add_new_key(&name)?;
            }
        }
        KeysCommands::List => {
            list_keys()?;
        }
        KeysCommands::Show { name } => {
            show_key(&name)?;
        }
        KeysCommands::Delete { name } => {
            delete_key(&name)?;
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
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info"))
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

fn create_directories(home_dir: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(home_dir)?;
    std::fs::create_dir_all(home_dir.join("config"))?;
    std::fs::create_dir_all(home_dir.join("data"))?;
    std::fs::create_dir_all(home_dir.join("wasm_modules"))?;
    Ok(())
}

fn init_config_files(home_dir: &PathBuf, chain_id: &str) -> Result<()> {
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
    format!(r#"# Application Configuration
chain_id = "{}"

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
"#, chain_id)
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
"#.to_string()
}

fn init_node_key(home_dir: &PathBuf) -> Result<()> {
    let key_path = home_dir.join("config").join("node_key.json");
    if !key_path.exists() {
        // TODO: Generate actual node key using helium-crypto
        let placeholder_key = r#"{"priv_key": {"type": "secp256k1", "value": "placeholder"}}"#;
        std::fs::write(&key_path, placeholder_key)?;
        tracing::info!("Generated node key: {}", key_path.display());
    }
    Ok(())
}

fn init_genesis_file(home_dir: &PathBuf, genesis_path: &PathBuf) -> Result<()> {
    let dest_path = home_dir.join("config").join("genesis.json");
    
    // Validate genesis file
    validate_genesis_file(genesis_path)?;
    
    // Copy to config directory
    std::fs::copy(genesis_path, &dest_path)?;
    tracing::info!("Genesis file copied to: {}", dest_path.display());
    
    Ok(())
}

fn validate_genesis_file(path: &PathBuf) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let _: serde_json::Value = serde_json::from_str(&content)?;
    // TODO: Add proper genesis validation using helium types
    Ok(())
}

fn export_genesis(_home_dir: &PathBuf, _output_path: &PathBuf) -> Result<()> {
    // TODO: Implement genesis export
    anyhow::bail!("Genesis export not yet implemented")
}

fn load_config(path: &PathBuf) -> Result<AppConfig> {
    let content = std::fs::read_to_string(path)?;
    let config: AppConfig = toml::from_str(&content)?;
    Ok(config)
}

fn init_storage(_home_dir: &PathBuf, _config: &AppConfig) -> Result<()> {
    // TODO: Initialize storage backends using helium-store
    tracing::info!("Storage backends initialized");
    Ok(())
}

fn load_wasm_modules(_config: &AppConfig) -> Result<()> {
    // TODO: Load WASM modules using wasmtime
    tracing::info!("WASM modules loaded");
    Ok(())
}

async fn start_abci_server(_home_dir: &PathBuf, _config: &AppConfig) -> Result<()> {
    // TODO: Start ABCI server using helium-server
    tracing::info!("ABCI server started");
    
    // Keep the server running
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down...");
    
    Ok(())
}

// Key management functions

fn add_new_key(name: &str) -> Result<()> {
    // TODO: Implement using helium-keyring
    tracing::info!("Key '{}' added", name);
    Ok(())
}

fn recover_key(name: &str) -> Result<()> {
    // TODO: Implement key recovery from mnemonic
    tracing::info!("Key '{}' recovered", name);
    Ok(())
}

fn list_keys() -> Result<()> {
    // TODO: List keys from keyring
    println!("NAME\tTYPE\tADDRESS");
    Ok(())
}

fn show_key(name: &str) -> Result<()> {
    // TODO: Show key details
    println!("Key: {}", name);
    Ok(())
}

fn delete_key(name: &str) -> Result<()> {
    // TODO: Delete key from keyring
    tracing::info!("Key '{}' deleted", name);
    Ok(())
}

// Config functions

fn show_config(home_dir: &PathBuf) -> Result<()> {
    let app_config_path = home_dir.join("config").join("app.toml");
    let content = std::fs::read_to_string(app_config_path)?;
    println!("{}", content);
    Ok(())
}

fn validate_config(path: &PathBuf) -> Result<()> {
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

#[derive(Debug, serde::Deserialize)]
struct WasmSection {
    modules_dir: String,
    cache_size: u32,
    memory_limit: String,
}