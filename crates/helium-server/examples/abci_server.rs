//! Example ABCI++ server

use helium_baseapp::BaseApp;
use helium_server::abci_server::AbciServerBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create a base application
    let app = BaseApp::new("helium-example".to_string());

    // Build and start the ABCI++ server
    AbciServerBuilder::new()
        .with_app(app)
        .with_chain_id("helium-testnet".to_string())
        .with_address("127.0.0.1:26658".to_string())
        .build_and_start()
        .await?;

    Ok(())
}
