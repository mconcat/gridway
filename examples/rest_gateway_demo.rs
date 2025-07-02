//! REST Gateway Demo
//!
//! This example demonstrates the REST gateway implementation that converts
//! Cosmos SDK REST API calls to gRPC service calls.

use helium_server::rest::{RestGateway, RestGatewayConfig};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting Helium REST Gateway Demo");

    // Configure REST gateway
    let config = RestGatewayConfig {
        grpc_endpoint: "http://[::1]:9090".to_string(),
        enable_logging: true,
    };

    // Create REST gateway with real gRPC services
    let gateway = RestGateway::new(config).await?;
    let app = gateway.router();

    // Start server
    let addr: SocketAddr = "127.0.0.1:1317".parse()?;
    let listener = TcpListener::bind(&addr).await?;

    info!("🚀 REST Gateway listening on http://{}", addr);
    info!("📋 Available endpoints:");
    info!("   GET  /health                                      - Health check");
    info!("   GET  /status                                      - Status information");
    info!("   GET  /cosmos/bank/v1beta1/balances/cosmos1test    - Get all balances");
    info!("   GET  /cosmos/bank/v1beta1/balances/cosmos1test/stake - Get stake balance");
    info!("   GET  /cosmos/auth/v1beta1/accounts/cosmos1test    - Get account info");
    info!("   POST /cosmos/tx/v1beta1/simulate                  - Simulate transaction");
    info!("   POST /cosmos/tx/v1beta1/txs                       - Broadcast transaction");
    info!("");
    info!("💡 Example curl commands:");
    info!("   curl http://127.0.0.1:1317/health");
    info!("   curl http://127.0.0.1:1317/cosmos/bank/v1beta1/balances/cosmos1test");
    info!("   curl http://127.0.0.1:1317/cosmos/auth/v1beta1/accounts/cosmos1test");

    // Serve the application
    // Note: This example requires axum to be added to the root Cargo.toml dependencies
    // axum::serve(listener, app).await?;

    // For now, just indicate that the server would start here
    info!("REST Gateway configured and ready to serve (axum dependency required to actually run)");

    Ok(())
}
