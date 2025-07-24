//! Example of starting a gRPC server with gridway services

use gridway_baseapp::BaseApp;
use gridway_server::grpc::services::*;
use gridway_store::StateManager;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create state manager and baseapp
    let state_manager = StateManager::new_with_memstore();
    let _state_manager = Arc::new(RwLock::new(state_manager));
    let base_app = Arc::new(RwLock::new(BaseApp::new("example-app".to_string())?));

    // Create service implementations
    let bank_service = BankQueryService::new();
    let auth_service = AuthQueryService::new();
    let _tx_service = TxService::with_baseapp(base_app);

    // Initialize test data
    bank_service
        .set_balance("cosmos1example", "uatom", 1000000)
        .await;

    auth_service
        .set_account(gridway_server::grpc::BaseAccount {
            address: "cosmos1example".to_string(),
            pub_key: None,
            account_number: 1,
            sequence: 0,
        })
        .await;

    info!("Example gRPC services initialized successfully");
    info!("Services include: Bank, Auth, and Transaction services");
    info!("In a real deployment, these would be served via tonic gRPC server");

    Ok(())
}
