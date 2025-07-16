//! Health check endpoints for monitoring

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Health check response
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub chain_id: String,
    pub block_height: u64,
    pub abci_connected: bool,
    pub syncing: bool,
}

/// Readiness check response
#[derive(Debug, Serialize, Deserialize)]
pub struct ReadyResponse {
    pub ready: bool,
    pub reason: Option<String>,
}

/// Health check state
#[derive(Clone)]
pub struct HealthState {
    pub chain_id: String,
    pub version: String,
    pub block_height: Arc<RwLock<u64>>,
    pub abci_connected: Arc<RwLock<bool>>,
    pub syncing: Arc<RwLock<bool>>,
}

impl HealthState {
    pub fn new(chain_id: String) -> Self {
        Self {
            chain_id,
            version: env!("CARGO_PKG_VERSION").to_string(),
            block_height: Arc::new(RwLock::new(0)),
            abci_connected: Arc::new(RwLock::new(false)),
            syncing: Arc::new(RwLock::new(true)),
        }
    }

    pub async fn update_height(&self, height: u64) {
        *self.block_height.write().await = height;
    }

    pub async fn set_connected(&self, connected: bool) {
        *self.abci_connected.write().await = connected;
    }

    pub async fn set_syncing(&self, syncing: bool) {
        *self.syncing.write().await = syncing;
    }
}

/// Health check handler
pub async fn health_handler(
    State(state): State<HealthState>,
) -> Result<Json<HealthResponse>, StatusCode> {
    let block_height = *state.block_height.read().await;
    let abci_connected = *state.abci_connected.read().await;
    let syncing = *state.syncing.read().await;

    Ok(Json(HealthResponse {
        status: "healthy".to_string(),
        version: state.version.clone(),
        chain_id: state.chain_id.clone(),
        block_height,
        abci_connected,
        syncing,
    }))
}

/// Readiness check handler
pub async fn ready_handler(
    State(state): State<HealthState>,
) -> Result<Json<ReadyResponse>, StatusCode> {
    let abci_connected = *state.abci_connected.read().await;
    let syncing = *state.syncing.read().await;

    if !abci_connected {
        return Ok(Json(ReadyResponse {
            ready: false,
            reason: Some("ABCI not connected".to_string()),
        }));
    }

    if syncing {
        return Ok(Json(ReadyResponse {
            ready: false,
            reason: Some("Node is syncing".to_string()),
        }));
    }

    Ok(Json(ReadyResponse {
        ready: true,
        reason: None,
    }))
}

/// Create health check router
pub fn health_router(state: HealthState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .with_state(state)
}
