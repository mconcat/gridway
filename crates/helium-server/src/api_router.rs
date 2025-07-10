//! Main API router combining all REST endpoints

use axum::Router;
use crate::health::{HealthState, health_router};
use crate::swagger::swagger_router;

/// Create the main REST API router
pub fn create_api_router(health_state: HealthState) -> Router {
    Router::new()
        .merge(health_router(health_state))
        .merge(swagger_router())
        // TODO: Add other REST endpoints here (bank, auth, tx, etc.)
}