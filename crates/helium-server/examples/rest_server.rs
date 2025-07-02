//! Example REST server with integrated REST gateway

use axum::Router;
use helium_server::rest::{RestGateway, RestGatewayConfig};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create REST gateway configuration
    let rest_config = RestGatewayConfig {
        grpc_endpoint: "http://[::1]:9090".to_string(),
        enable_logging: true,
    };

    // Create REST gateway
    let rest_gateway = RestGateway::new(rest_config).await?;

    // Build combined router
    let app = Router::new()
        // Mount REST API under root
        .nest("/", rest_gateway.router())
        // Add CORS layer
        .layer(CorsLayer::permissive());

    // Start server
    let addr: SocketAddr = "127.0.0.1:1317".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("REST server listening on {}", addr);
    info!("Available endpoints:");
    info!("  GET  /cosmos/bank/v1beta1/balances/{{address}}");
    info!("  GET  /cosmos/bank/v1beta1/balances/{{address}}/{{denom}}");
    info!("  GET  /cosmos/auth/v1beta1/accounts/{{address}}");
    info!("  POST /cosmos/tx/v1beta1/txs");
    info!("  POST /cosmos/tx/v1beta1/simulate");

    axum::serve(listener, app).await?;

    Ok(())
}
