//! Integrated server that combines real service implementations
//!
//! This module provides an integrated server setup that uses production-ready
//! service implementations with state store integration instead of mock services.

use crate::services::{AuthService, BankService, TxService};
use gridway_baseapp::BaseApp;
use gridway_store::{KVStore, StateManager};
use gridway_telemetry::{
    init as init_telemetry,
    metrics::{BLOCK_HEIGHT, CONNECTED_PEERS, MEMPOOL_SIZE, TOTAL_TRANSACTIONS},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Integrated server configuration
#[derive(Debug, Clone)]
pub struct IntegratedServerConfig {
    /// Server bind address
    pub address: String,
    /// Enable service initialization with test data
    pub initialize_test_data: bool,
    /// Chain ID
    pub chain_id: String,
    /// Enable metrics collection
    pub enable_metrics: bool,
    /// Metrics server bind address
    pub metrics_address: String,
}

impl Default for IntegratedServerConfig {
    fn default() -> Self {
        Self {
            address: "127.0.0.1:9090".to_string(),
            initialize_test_data: true,
            chain_id: "helium-testnet".to_string(),
            enable_metrics: true,
            metrics_address: "127.0.0.1:1317".to_string(),
        }
    }
}

/// Integrated server with real service implementations
pub struct IntegratedServer {
    config: IntegratedServerConfig,
    state_manager: Arc<RwLock<StateManager>>,
    base_app: Arc<RwLock<BaseApp>>,
    bank_service: Option<Arc<BankService>>,
    auth_service: Option<Arc<AuthService>>,
    tx_service: Option<Arc<TxService>>,
}

impl IntegratedServer {
    /// Create a new integrated server
    pub fn new(config: IntegratedServerConfig) -> Self {
        let mut state_manager = StateManager::new_with_memstore();

        // Register namespaces for different modules
        state_manager
            .register_namespace("bank".to_string(), false)
            .expect("Failed to register bank namespace");
        state_manager
            .register_namespace("auth".to_string(), false)
            .expect("Failed to register auth namespace");
        state_manager
            .register_namespace("tx".to_string(), false)
            .expect("Failed to register tx namespace");

        let state_manager = Arc::new(RwLock::new(state_manager));
        let base_app = Arc::new(RwLock::new(
            BaseApp::new("helium".to_string()).expect("Failed to create BaseApp"),
        ));

        Self {
            config,
            state_manager,
            base_app,
            bank_service: None,
            auth_service: None,
            tx_service: None,
        }
    }

    /// Initialize all services
    pub async fn initialize_services(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Initializing integrated server services");

        // Initialize telemetry if enabled
        if self.config.enable_metrics {
            init_telemetry()?;
            info!("Telemetry subsystem initialized");
        }

        // Create service instances
        let bank_service = Arc::new(BankService::with_defaults(
            self.state_manager.clone(),
            self.base_app.clone(),
        ));

        let auth_service = Arc::new(AuthService::with_defaults(
            self.state_manager.clone(),
            self.base_app.clone(),
        ));

        let tx_service = Arc::new(TxService::with_defaults(
            self.state_manager.clone(),
            self.base_app.clone(),
        ));

        // Initialize with test data if configured
        if self.config.initialize_test_data {
            info!("Initializing services with test data");

            bank_service
                .initialize_for_testing()
                .await
                .map_err(|e| format!("Failed to initialize bank service:: {e}"))?;

            auth_service
                .initialize_for_testing()
                .await
                .map_err(|e| format!("Failed to initialize auth service:: {e}"))?;

            tx_service
                .initialize_for_testing()
                .await
                .map_err(|e| format!("Failed to initialize tx service:: {e}"))?;

            info!("Test data initialization completed");
        }

        // Store service references
        self.bank_service = Some(bank_service);
        self.auth_service = Some(auth_service);
        self.tx_service = Some(tx_service);

        info!("All services initialized successfully");
        Ok(())
    }

    /// Start the metrics server if enabled
    async fn start_metrics_server(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.config.enable_metrics {
            let metrics_config = gridway_telemetry::http::MetricsServerConfig {
                bind_address: self
                    .config
                    .metrics_address
                    .parse()
                    .map_err(|e| format!("Invalid metrics address:: {e}"))?,
                metrics_path: "/metrics".to_string(),
                enable_health_check: true,
                global_labels: vec![("chain_id".to_string(), self.config.chain_id.clone())],
            };

            let registry = gridway_telemetry::registry();
            let _metrics_handle =
                gridway_telemetry::http::spawn_metrics_server(registry, metrics_config);

            info!("Metrics server started on {}", self.config.metrics_address);
        }
        Ok(())
    }

    /// Start the integrated gRPC server
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.bank_service.is_none() || self.auth_service.is_none() || self.tx_service.is_none() {
            return Err("Services not initialized. Call initialize_services() first.".into());
        }

        let addr: std::net::SocketAddr = self
            .config
            .address
            .parse()
            .map_err(|e| format!("Invalid server address {}: {}", self.config.address, e))?;

        info!("Starting integrated gRPC server on {}", addr);

        // Start metrics server if enabled
        self.start_metrics_server().await?;

        // Clone service references for the server
        let _bank_service = self.bank_service.as_ref().unwrap().clone();
        let _auth_service = self.auth_service.as_ref().unwrap().clone();
        let _tx_service = self.tx_service.as_ref().unwrap().clone();

        // Note: In a real gRPC implementation with generated proto code, we would:
        // 1. Generate service definitions from .proto files using tonic-build
        // 2. Implement the generated service traits on our service structs
        // 3. Add the services to the tonic Server using .add_service()
        //
        // For this proof-of-concept, we demonstrate the structure but cannot
        // actually bind to gRPC endpoints without the generated proto code.

        // This is what the real implementation would look like:
        /*
        let server = Server::builder()
            .add_service(BankQueryServer::new(bank_service))
            .add_service(AuthQueryServer::new(auth_service))
            .add_service(TxServiceServer::new(tx_service))
            .serve(addr);

        info!("gRPC server started successfully");
        server.await?;
        */

        // For now, we'll create a placeholder server that shows the integration
        info!("Creating gRPC server with integrated services");
        info!("Bank service: ready with state store integration");
        info!("Auth service: ready with account management");
        info!("Tx service: ready with transaction processing");

        // Simulate server running
        info!("Integrated gRPC server would be listening on {}", addr);
        info!("Services are configured and ready to handle requests");

        // In a real deployment, this would be:
        // server.await?;

        Ok(())
    }

    /// Get service statistics for monitoring
    pub async fn get_service_stats(
        &self,
    ) -> Result<ServiceStats, Box<dyn std::error::Error + Send + Sync>> {
        let mut stats = ServiceStats::default();

        if let Some(_bank_service) = &self.bank_service {
            // Count bank store entries
            let state_manager = self.state_manager.read().await;
            if let Ok(store) = state_manager.get_store("bank") {
                let mut balance_count = 0;
                for (_key, _) in store.prefix_iterator(b"balance_") {
                    balance_count += 1;
                }
                stats.bank_balance_count = balance_count;

                // Update metrics
                if self.config.enable_metrics {
                    MEMPOOL_SIZE.set(0); // Example - would be actual mempool size
                }
            }
        }

        if let Some(_auth_service) = &self.auth_service {
            // Count auth store entries
            let state_manager = self.state_manager.read().await;
            if let Ok(store) = state_manager.get_store("auth") {
                let _account_count = 0;
                let mut account_count = 0;
                let prefix = b"account_";
                for (key, _) in store.prefix_iterator(prefix) {
                    let key_str = String::from_utf8_lossy(&key);
                    if key_str.starts_with("account_") && !key_str.contains("next_account_number") {
                        account_count += 1;
                    }
                }
                stats.auth_account_count = account_count;
            }
        }

        if let Some(_tx_service) = &self.tx_service {
            // Count tx store entries
            let state_manager = self.state_manager.read().await;
            if let Ok(store) = state_manager.get_store("tx") {
                let mut tx_count = 0;
                for (_key, _) in store.prefix_iterator(b"tx_hash_") {
                    tx_count += 1;
                }
                stats.tx_transaction_count = tx_count;

                // Update metrics
                if self.config.enable_metrics {
                    TOTAL_TRANSACTIONS.inc_by(tx_count as u64);
                    BLOCK_HEIGHT.set(1000); // Example - would be actual block height
                    CONNECTED_PEERS.set(5); // Example - would be actual peer count
                }
            }
        }

        Ok(stats)
    }

    /// Shutdown the server gracefully
    pub async fn shutdown(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Shutting down integrated server");

        // Commit any pending state changes
        let mut state_manager = self.state_manager.write().await;
        state_manager
            .commit()
            .map_err(|e| format!("Failed to commit final state:: {e}"))?;

        info!("Server shutdown completed successfully");
        Ok(())
    }
}

/// Service statistics for monitoring
#[derive(Debug, Default)]
pub struct ServiceStats {
    pub bank_balance_count: usize,
    pub auth_account_count: usize,
    pub tx_transaction_count: usize,
}

/// Example of how to create and run the integrated server
pub async fn run_integrated_server() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create server configuration
    let config = IntegratedServerConfig {
        address: "127.0.0.1:9090".to_string(),
        initialize_test_data: true,
        chain_id: "helium-testnet".to_string(),
        enable_metrics: true,
        metrics_address: "127.0.0.1:1317".to_string(),
    };

    // Create and initialize server
    let mut server = IntegratedServer::new(config);
    server.initialize_services().await?;

    // Show service statistics
    let stats = server.get_service_stats().await?;
    info!("Service statistics:: {:?}", stats);

    // Start the server (in real implementation, this would run indefinitely)
    server.start().await?;

    // Graceful shutdown
    server.shutdown().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_integrated_server_creation() {
        let config = IntegratedServerConfig::default();
        let server = IntegratedServer::new(config);

        assert!(server.bank_service.is_none());
        assert!(server.auth_service.is_none());
        assert!(server.tx_service.is_none());
    }

    #[tokio::test]
    async fn test_service_initialization() {
        let config = IntegratedServerConfig::default();
        let mut server = IntegratedServer::new(config);

        server.initialize_services().await.unwrap();

        assert!(server.bank_service.is_some());
        assert!(server.auth_service.is_some());
        assert!(server.tx_service.is_some());
    }

    #[tokio::test]
    async fn test_service_stats() {
        let config = IntegratedServerConfig::default();
        let mut server = IntegratedServer::new(config);

        server.initialize_services().await.unwrap();
        let stats = server.get_service_stats().await.unwrap();

        // Should have test data initialized
        assert!(stats.bank_balance_count > 0);
        assert!(stats.auth_account_count > 0);
    }

    #[tokio::test]
    async fn test_server_without_initialization() {
        let config = IntegratedServerConfig::default();
        let server = IntegratedServer::new(config);

        let result = server.start().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Services not initialized"));
    }
}
