//! Type definitions for the telemetry module.

use thiserror::Error;

/// Errors that can occur in the metrics subsystem
#[derive(Error, Debug)]
pub enum MetricError {
    /// Failed to register a metric
    #[error("failed to register metric: {0}")]
    RegistrationFailed(String),

    /// Failed to observe a metric value
    #[error("failed to observe metric: {0}")]
    ObservationFailed(String),

    /// Failed to encode metrics
    #[error("failed to encode metrics: {0}")]
    EncodingFailed(String),

    /// HTTP server error
    #[error("HTTP server error: {0}")]
    HttpServerError(String),

    /// Invalid metric configuration
    #[error("invalid metric configuration: {0}")]
    InvalidConfiguration(String),

    /// Prometheus error
    #[error("prometheus error: {0}")]
    PrometheusError(#[from] prometheus::Error),
}

/// Result type for metric operations
pub type MetricResult<T> = Result<T, MetricError>;
