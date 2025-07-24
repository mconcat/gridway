//! Telemetry and metrics infrastructure for Gridway blockchain.
//!
//! This crate provides a comprehensive metrics collection and exposition
//! framework using Prometheus for monitoring blockchain health and performance.

pub mod http;
pub mod metrics;
pub mod registry;
pub mod types;

pub use metrics::{
    BLOCK_HEIGHT, BLOCK_PROCESSING_TIME, MEMPOOL_SIZE, TOTAL_BLOCKS_PROCESSED, TOTAL_TRANSACTIONS,
    TRANSACTION_PROCESSING_TIME, TX_FAILED, TX_GAS_USED, TX_SIZE_BYTES,
};
pub use registry::MetricsRegistry;
pub use types::{MetricError, MetricResult};

use lazy_static::lazy_static;
use std::sync::Arc;

lazy_static! {
    /// Global metrics registry instance
    pub static ref METRICS_REGISTRY: Arc<MetricsRegistry> = {
        match MetricsRegistry::new() {
            Ok(registry) => Arc::new(registry),
            Err(e) => {
                tracing::error!(error = %e, "Failed to initialize metrics registry");
                panic!("Critical error: Failed to initialize metrics registry:: {e}");
            }
        }
    };
}

/// Initialize the telemetry subsystem
pub fn init() -> MetricResult<()> {
    // Force lazy static initialization
    let _ = &*METRICS_REGISTRY;

    tracing::info!("Telemetry subsystem initialized");
    Ok(())
}

/// Get a reference to the global metrics registry
pub fn registry() -> Arc<MetricsRegistry> {
    METRICS_REGISTRY.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        assert!(init().is_ok());
    }

    #[test]
    fn test_registry_access() {
        let registry = registry();
        assert!(Arc::strong_count(&registry) >= 1);
    }
}
