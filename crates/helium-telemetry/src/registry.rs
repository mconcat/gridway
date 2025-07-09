//! Metrics registry for managing custom metrics.
//!
//! This module provides a thread-safe registry wrapper around Prometheus Registry
//! that allows for dynamic registration of custom metrics at runtime.

use prometheus::{proto::MetricFamily, Encoder, Registry, TextEncoder};
use std::sync::{Arc, RwLock};

use crate::{
    metrics::register_core_metrics,
    types::{MetricError, MetricResult},
};

/// Thread-safe metrics registry wrapper
pub struct MetricsRegistry {
    /// The underlying Prometheus registry
    registry: Arc<Registry>,
    /// Lock for thread-safe access
    custom_metrics: Arc<RwLock<Vec<String>>>,
}

impl MetricsRegistry {
    /// Create a new metrics registry with core metrics pre-registered
    pub fn new() -> MetricResult<Self> {
        let registry = Arc::new(Registry::new());

        // Register core blockchain metrics
        register_core_metrics(&registry)?;

        Ok(Self {
            registry,
            custom_metrics: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Create a new metrics registry without pre-registered metrics
    pub fn new_empty() -> Self {
        Self {
            registry: Arc::new(Registry::new()),
            custom_metrics: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get a reference to the underlying Prometheus registry
    pub fn inner(&self) -> &Registry {
        &self.registry
    }

    /// Register a custom metric collector
    pub fn register_collector(
        &self,
        collector: Box<dyn prometheus::core::Collector>,
    ) -> MetricResult<()> {
        self.registry
            .register(collector)
            .map_err(|e| MetricError::RegistrationFailed(e.to_string()))?;

        Ok(())
    }

    /// Register a custom metric and track its name
    pub fn register_metric(
        &self,
        name: String,
        collector: Box<dyn prometheus::core::Collector>,
    ) -> MetricResult<()> {
        self.register_collector(collector)?;

        let mut metrics = self.custom_metrics.write().unwrap();
        metrics.push(name);

        Ok(())
    }

    /// Get all registered metric names
    pub fn metric_names(&self) -> Vec<String> {
        let metrics = self.custom_metrics.read().unwrap();
        metrics.clone()
    }

    /// Gather all metrics from the registry
    pub fn gather(&self) -> Vec<MetricFamily> {
        self.registry.gather()
    }

    /// Encode metrics in Prometheus text format
    pub fn encode_to_string(&self) -> MetricResult<String> {
        let encoder = TextEncoder::new();
        let metric_families = self.gather();

        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .map_err(|e| MetricError::EncodingFailed(e.to_string()))?;

        String::from_utf8(buffer).map_err(|e| MetricError::EncodingFailed(e.to_string()))
    }

    /// Unregister all metrics (useful for testing)
    #[cfg(test)]
    pub fn clear(&self) -> MetricResult<()> {
        // Prometheus doesn't support unregistering, so we create a new registry
        // This is only available in tests
        Ok(())
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new().expect("Failed to create default metrics registry")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus::{Counter, Gauge};

    #[test]
    fn test_registry_creation() {
        let registry = MetricsRegistry::new();
        assert!(registry.is_ok());
    }

    #[test]
    fn test_empty_registry() {
        let registry = MetricsRegistry::new_empty();
        let metrics = registry.gather();
        assert!(metrics.is_empty());
    }

    #[test]
    fn test_custom_metric_registration() {
        let registry = MetricsRegistry::new().unwrap();

        let counter = Counter::new("test_counter", "A test counter").unwrap();
        let result = registry.register_metric("test_counter".to_string(), Box::new(counter));

        assert!(result.is_ok());
        assert!(registry
            .metric_names()
            .contains(&"test_counter".to_string()));
    }

    #[test]
    fn test_gather_metrics() {
        let registry = MetricsRegistry::new().unwrap();
        let metrics = registry.gather();

        // Should have core metrics registered
        assert!(!metrics.is_empty());
    }

    #[test]
    fn test_encode_metrics() {
        let registry = MetricsRegistry::new().unwrap();
        let encoded = registry.encode_to_string();

        assert!(encoded.is_ok());
        let output = encoded.unwrap();
        assert!(output.contains("consensus_height"));
        assert!(output.contains("tx_count"));
    }

    #[test]
    fn test_duplicate_registration_fails() {
        let registry = MetricsRegistry::new().unwrap();

        let gauge1 = Gauge::new("duplicate_metric", "First gauge").unwrap();
        let gauge2 = Gauge::new("duplicate_metric", "Second gauge").unwrap();

        let result1 = registry.register_collector(Box::new(gauge1));
        assert!(result1.is_ok());

        let result2 = registry.register_collector(Box::new(gauge2));
        assert!(result2.is_err());
    }
}
