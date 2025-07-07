//! Server components and utilities for the helium blockchain.
//!
//! This crate provides HTTP/RPC server implementations and utilities
//! for helium blockchain applications.

pub mod abci_server;
pub mod config;
pub mod consensus;
pub mod grpc;
pub mod rest;
pub mod services;
pub mod snapshot;
pub mod validators;

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::info;
// NOTE: REST gateway integration disabled due to compilation dependencies
// Use the rest module directly when baseapp conflicts are resolved

/// Server error types
#[derive(Error, Debug)]
pub enum ServerError {
    /// Bind error
    #[error("failed to bind to address: {0}")]
    Bind(String),

    /// Serve error
    #[error("server error: {0}")]
    Serve(String),

    /// Invalid request
    #[error("invalid request: {0}")]
    InvalidRequest(String),
}

/// Result type for server operations
pub type Result<T> = std::result::Result<T, ServerError>;

/// Server configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Server address
    pub address: SocketAddr,
    /// Enable CORS
    pub cors: bool,
    /// Maximum request size
    pub max_request_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            address: "127.0.0.1:26657".parse().unwrap(),
            cors: true,
            max_request_size: 1024 * 1024, // 1MB
        }
    }
}

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    /// Application name
    pub name: String,
    /// Application version
    pub version: String,
}

/// Health check response
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub name: String,
    pub version: String,
}

/// Status response
#[derive(Serialize)]
pub struct StatusResponse {
    pub node_info: NodeInfo,
    pub sync_info: SyncInfo,
}

/// Node information
#[derive(Serialize)]
pub struct NodeInfo {
    pub id: String,
    pub moniker: String,
    pub network: String,
    pub version: String,
}

/// Sync information
#[derive(Serialize)]
pub struct SyncInfo {
    pub latest_block_height: u64,
    pub latest_block_time: String,
    pub catching_up: bool,
}

/// RPC request wrapper
#[derive(Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

/// RPC response wrapper
#[derive(Serialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: String,
    pub result: serde_json::Value,
}

/// RPC error response
#[derive(Serialize)]
pub struct RpcErrorResponse {
    pub jsonrpc: String,
    pub id: String,
    pub error: RpcError,
}

/// RPC error details
#[derive(Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

/// Health check handler
async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        name: state.name.clone(),
        version: state.version.clone(),
    })
}

/// Status handler
async fn status_handler(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    Json(StatusResponse {
        node_info: NodeInfo {
            id: "node123".to_string(),
            moniker: "helium-node".to_string(),
            network: "helium-testnet".to_string(),
            version: state.version.clone(),
        },
        sync_info: SyncInfo {
            latest_block_height: 1000,
            latest_block_time: "2024-01-01T12:00:00Z".to_string(),
            catching_up: false,
        },
    })
}

/// RPC handler
async fn rpc_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RpcRequest>,
) -> std::result::Result<Json<RpcResponse>, (StatusCode, Json<RpcErrorResponse>)> {
    match req.method.as_str() {
        "abci_info" => {
            let result = serde_json::json!({
                "response": {
                    "data": state.name,
                    "version": state.version,
                    "app_version": "1",
                    "last_block_height": "1000",
                    "last_block_app_hash": ""
                }
            });

            Ok(Json(RpcResponse {
                jsonrpc: req.jsonrpc,
                id: req.id,
                result,
            }))
        }
        _ => {
            let error_response = RpcErrorResponse {
                jsonrpc: req.jsonrpc,
                id: req.id,
                error: RpcError {
                    code: -32601,
                    message: format!("Method not found: {}", req.method),
                },
            };
            Err((StatusCode::NOT_FOUND, Json(error_response)))
        }
    }
}

/// HTTP server for helium applications
pub struct Server {
    config: Config,
    state: Arc<AppState>,
}

impl Server {
    /// Create a new server
    pub fn new(config: Config, name: String, version: String) -> Self {
        let state = Arc::new(AppState { name, version });
        Self { config, state }
    }

    /// Build the router
    fn build_router(&self) -> Router {
        let mut router = Router::new()
            .route("/health", get(health_handler))
            .route("/status", get(status_handler))
            .route("/", post(rpc_handler))
            .with_state(self.state.clone());

        if self.config.cors {
            router = router.layer(CorsLayer::permissive());
        }

        router
    }

    /// Start the server
    pub async fn start(&self) -> Result<()> {
        let app = self.build_router();

        let listener = TcpListener::bind(&self.config.address)
            .await
            .map_err(|e| ServerError::Bind(e.to_string()))?;

        info!(address = %self.config.address, "Server listening");

        axum::serve(listener, app)
            .await
            .map_err(|e| ServerError::Serve(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Import only if needed for actual tests

    #[tokio::test]
    async fn test_server_creation() {
        let config = Config::default();
        let server = Server::new(config, "test-app".to_string(), "1.0.0".to_string());
        assert_eq!(server.state.name, "test-app");
        assert_eq!(server.state.version, "1.0.0");
    }

    #[tokio::test]
    async fn test_health_handler() {
        let state = Arc::new(AppState {
            name: "test".to_string(),
            version: "1.0".to_string(),
        });

        let response = health_handler(State(state)).await;
        assert_eq!(response.status, "ok");
        assert_eq!(response.name, "test");
        assert_eq!(response.version, "1.0");
    }
}
