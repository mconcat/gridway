//! Helium Server Binary
//!
//! This is the main entry point for the Helium blockchain node.

use clap::{Parser, Subcommand};
use helium_baseapp::BaseApp;
use helium_server::{
    abci_server::AbciServer, 
    api_router::create_api_router,
    config::AbciConfig, 
    health::HealthState
};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};
use axum;

#[derive(Parser)]
#[command(name = "helium")]
#[command(about = "Helium blockchain node", long_about = None)]
struct Cli {
    /// Set the home directory
    #[arg(long, default_value = ".helium")]
    home: PathBuf,

    /// Subcommand to execute
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize the node configuration
    Init {
        /// Chain ID for the network
        #[arg(long, default_value = "helium-testnet")]
        chain_id: String,
    },
    /// Start the node
    Start {
        /// ABCI listen address
        #[arg(long, default_value = "tcp://0.0.0.0:26658")]
        abci_address: String,
        
        /// gRPC listen address
        #[arg(long, default_value = "0.0.0.0:9090")]
        grpc_address: String,
        
        /// Chain ID
        #[arg(long)]
        chain_id: Option<String>,
    },
    /// Show version information
    Version,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("helium=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init { chain_id } => {
            info!("Initializing Helium node with chain ID: {}", chain_id);
            
            // Create home directory
            std::fs::create_dir_all(&cli.home)?;
            
            // Create config directory
            let config_dir = cli.home.join("config");
            std::fs::create_dir_all(&config_dir)?;
            
            // Create data directory
            let data_dir = cli.home.join("data");
            std::fs::create_dir_all(&data_dir)?;
            
            // Write configuration file
            let config = AbciConfig {
                listen_address: "tcp://0.0.0.0:26658".to_string(),
                grpc_address: Some("0.0.0.0:9090".to_string()),
                max_connections: 10,
                flush_interval: 100,
                persist_interval: 1,
                retain_blocks: 0,
                chain_id: chain_id.clone(),
            };
            
            let config_path = config_dir.join("config.toml");
            let config_toml = toml::to_string_pretty(&config)?;
            std::fs::write(config_path, config_toml)?;
            
            info!("Node initialized successfully at {}", cli.home.display());
        }
        Commands::Start { abci_address, grpc_address, chain_id } => {
            info!("Starting Helium node");
            
            // Load configuration
            let config_path = cli.home.join("config/config.toml");
            let config = if config_path.exists() {
                let config_str = std::fs::read_to_string(&config_path)?;
                toml::from_str::<AbciConfig>(&config_str)?
            } else {
                AbciConfig {
                    listen_address: abci_address.clone(),
                    grpc_address: Some(grpc_address),
                    max_connections: 10,
                    flush_interval: 100,
                    persist_interval: 1,
                    retain_blocks: 0,
                    chain_id: chain_id.unwrap_or_else(|| "helium-testnet".to_string()),
                }
            };
            
            info!("Chain ID: {}", config.chain_id);
            info!("ABCI address: {}", config.listen_address);
            if let Some(ref grpc) = config.grpc_address {
                info!("gRPC address: {}", grpc);
            }
            
            // Create BaseApp
            let app = BaseApp::new(config.chain_id.clone())?;
            
            // Create and start ABCI server
            let server = AbciServer::with_config(app, config.chain_id.clone(), config.clone());
            
            // Create health state
            let health_state = HealthState::new(config.chain_id.clone());
            health_state.set_connected(true).await;
            
            // Create shutdown channel
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
            let (shutdown_tx2, mut shutdown_rx2) = tokio::sync::oneshot::channel();
            
            // Start REST API server on standard Cosmos SDK port
            let rest_addr = "0.0.0.0:1317";
            let health_state_clone = health_state.clone();
            tokio::spawn(async move {
                info!("Starting REST API server on {}", rest_addr);
                let app = create_api_router(health_state_clone);
                let listener = tokio::net::TcpListener::bind(rest_addr)
                    .await
                    .expect("Failed to bind REST API endpoint");
                
                axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        let _ = shutdown_rx2.await;
                    })
                    .await
                    .expect("REST API server failed");
            });
            
            // Handle shutdown signals
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.expect("Failed to listen for ctrl+c");
                info!("Received shutdown signal");
                let _ = shutdown_tx.send(());
                let _ = shutdown_tx2.send(());
            });
            
            // Start ABCI server
            if let Err(e) = AbciServer::start_abci_server(
                server.app.clone(),
                &config,
                shutdown_rx,
            ).await {
                error!("ABCI server error: {}", e);
                return Err(e.into());
            }
        }
        Commands::Version => {
            println!("Helium v{}", env!("CARGO_PKG_VERSION"));
            println!("Rust Cosmos SDK implementation");
        }
    }

    Ok(())
}