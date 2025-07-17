//! HTTP metrics endpoint for Prometheus scraping.
//!
//! This module provides an HTTP server that exposes metrics in Prometheus format
//! for collection by Prometheus servers or other monitoring tools.

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

use crate::{
    registry::MetricsRegistry,
    types::{MetricError, MetricResult},
};

/// Configuration for the metrics HTTP server
#[derive(Debug, Clone)]
pub struct MetricsServerConfig {
    /// Address to bind the metrics server to
    pub bind_address: SocketAddr,
    /// Path to expose metrics on (default: /metrics)
    pub metrics_path: String,
    /// Whether to include a health check endpoint
    pub enable_health_check: bool,
    /// Global labels to apply to all metrics (Cosmos SDK compatibility)
    pub global_labels: Vec<(String, String)>,
}

impl Default for MetricsServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:1317".parse().unwrap(),
            metrics_path: "/metrics".to_string(),
            enable_health_check: true,
            global_labels: vec![],
        }
    }
}

/// Metrics HTTP server
pub struct MetricsServer {
    config: MetricsServerConfig,
    registry: Arc<MetricsRegistry>,
}

impl MetricsServer {
    /// Create a new metrics server with the given configuration
    pub fn new(config: MetricsServerConfig, registry: Arc<MetricsRegistry>) -> Self {
        Self { config, registry }
    }

    /// Build the router for the metrics server
    fn build_router(&self) -> Router {
        let mut router = Router::new()
            .route(&self.config.metrics_path, get(metrics_handler))
            .with_state(self.registry.clone());

        if self.config.enable_health_check {
            router = router.route("/health", get(health_handler));
        }

        router.layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .into_inner(),
        )
    }

    /// Start the metrics HTTP server
    pub async fn start(self) -> MetricResult<()> {
        let app = self.build_router();

        let listener = TcpListener::bind(&self.config.bind_address)
            .await
            .map_err(|e| MetricError::HttpServerError(format!("Failed to bind: {e}")))?;

        tracing::info!(
            address = %self.config.bind_address,
            path = %self.config.metrics_path,
            "Metrics server listening"
        );

        axum::serve(listener, app)
            .await
            .map_err(|e| MetricError::HttpServerError(e.to_string()))?;

        Ok(())
    }

    /// Start the metrics server in the background
    pub fn spawn(self) -> tokio::task::JoinHandle<MetricResult<()>> {
        tokio::spawn(async move { self.start().await })
    }
}

/// Query parameters for metrics endpoint
#[derive(Debug, Deserialize)]
struct MetricsQuery {
    format: Option<String>,
}

/// Response structure for Cosmos SDK compatibility
#[derive(Debug, Serialize)]
struct MetricsResponse {
    metrics: String,
    content_type: String,
}

/// Handler for the metrics endpoint
async fn metrics_handler(
    State(registry): State<Arc<MetricsRegistry>>,
    Query(params): Query<MetricsQuery>,
) -> Result<Response, StatusCode> {
    let format = params.format.as_deref().unwrap_or("text");

    match registry.encode_to_string() {
        Ok(metrics) => {
            match format {
                "prometheus" => Ok((
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                    metrics,
                )
                    .into_response()),
                _ => {
                    // Default text format returns JSON structure for Cosmos SDK compatibility
                    let response = MetricsResponse {
                        metrics,
                        content_type: "text/plain".to_string(),
                    };
                    Ok(Json(response).into_response())
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to encode metrics: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Handler for the health check endpoint
async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// Convenience function to start a metrics server with default configuration
pub async fn serve_metrics(registry: Arc<MetricsRegistry>) -> MetricResult<()> {
    let config = MetricsServerConfig::default();
    let server = MetricsServer::new(config, registry);
    server.start().await
}

/// Convenience function to spawn a metrics server in the background
pub fn spawn_metrics_server(
    registry: Arc<MetricsRegistry>,
    config: MetricsServerConfig,
) -> tokio::task::JoinHandle<MetricResult<()>> {
    let server = MetricsServer::new(config, registry);
    server.spawn()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init;

    #[tokio::test]
    async fn test_metrics_server_creation() {
        let _ = init();
        let registry = Arc::new(MetricsRegistry::new().unwrap());
        let config = MetricsServerConfig::default();
        let server = MetricsServer::new(config, registry);

        let router = server.build_router();
        // Just verify we can build the router without panicking
        let _ = router;
    }

    #[tokio::test]
    async fn test_metrics_handler() {
        let registry = Arc::new(MetricsRegistry::new().unwrap());
        let query = MetricsQuery {
            format: Some("prometheus".to_string()),
        };
        let response = metrics_handler(State(registry), Query(query)).await;

        assert!(response.is_ok());
        let response = response.unwrap();
        let (parts, _body) = response.into_parts();

        assert_eq!(parts.status, StatusCode::OK);
        assert!(parts.headers.get(header::CONTENT_TYPE).is_some());
    }

    #[tokio::test]
    async fn test_health_handler() {
        let response = health_handler().await;
        let response = response.into_response();
        let (parts, _) = response.into_parts();

        assert_eq!(parts.status, StatusCode::OK);
    }

    #[test]
    fn test_default_config() {
        let config = MetricsServerConfig::default();
        assert_eq!(config.metrics_path, "/metrics");
        assert!(config.enable_health_check);
        assert_eq!(config.bind_address.port(), 1317);
        assert!(config.global_labels.is_empty());
    }

    #[tokio::test]
    async fn test_metrics_handler_text_format() {
        let registry = Arc::new(MetricsRegistry::new().unwrap());
        let query = MetricsQuery {
            format: Some("text".to_string()),
        };
        let response = metrics_handler(State(registry), Query(query)).await;

        assert!(response.is_ok());
        // Text format returns JSON response
        let response = response.unwrap();
        let (parts, _) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
    }
}
