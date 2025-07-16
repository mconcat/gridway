//! Core blockchain metrics definitions.
//!
//! This module defines the standard metrics collected by the Helium blockchain
//! for monitoring health and performance.

use lazy_static::lazy_static;
use prometheus::{Gauge, HistogramVec, IntCounter, IntGauge, Registry};

use crate::types::MetricResult;

lazy_static! {
    /// Current blockchain height
    pub static ref BLOCK_HEIGHT: IntGauge = IntGauge::new(
        "consensus_height",
        "Current height of the blockchain"
    ).expect("Failed to create consensus_height metric");

    /// Total number of transactions processed
    pub static ref TOTAL_TRANSACTIONS: IntCounter = IntCounter::new(
        "tx_count",
        "Total number of transactions processed"
    ).expect("Failed to create tx_count metric");

    /// Transaction processing time histogram
    pub static ref TRANSACTION_PROCESSING_TIME: HistogramVec = HistogramVec::new(
        prometheus::HistogramOpts::new(
            "tx_processing_time",
            "Time taken to process a transaction in seconds"
        ).buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
        &["tx_type"]
    ).expect("Failed to create tx_processing_time metric");

    /// Current mempool size
    pub static ref MEMPOOL_SIZE: IntGauge = IntGauge::new(
        "mempool_tx_count",
        "Current number of transactions in the mempool"
    ).expect("Failed to create mempool_tx_count metric");

    /// Total number of blocks processed
    pub static ref TOTAL_BLOCKS_PROCESSED: IntCounter = IntCounter::new(
        "total_blocks_processed",
        "Total number of blocks processed"
    ).expect("Failed to create total_blocks_processed metric");

    /// Block processing time histogram
    pub static ref BLOCK_PROCESSING_TIME: HistogramVec = HistogramVec::new(
        prometheus::HistogramOpts::new(
            "consensus_block_processing_time",
            "Time taken to process a block in seconds"
        ).buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0]),
        &["block_type"]
    ).expect("Failed to create block_processing_time metric");

    /// Node uptime in seconds
    pub static ref NODE_UPTIME: Gauge = Gauge::new(
        "node_uptime_seconds",
        "Time since the node started in seconds"
    ).expect("Failed to create node_uptime metric");

    /// Number of connected peers
    pub static ref CONNECTED_PEERS: IntGauge = IntGauge::new(
        "connected_peers",
        "Number of currently connected peers"
    ).expect("Failed to create connected_peers metric");

    /// Transaction size in bytes
    pub static ref TX_SIZE_BYTES: HistogramVec = HistogramVec::new(
        prometheus::HistogramOpts::new(
            "tx_size_bytes",
            "Size of transactions in bytes"
        ).buckets(vec![100.0, 500.0, 1000.0, 5000.0, 10000.0, 50000.0]),
        &["tx_type"]
    ).expect("Failed to create tx_size_bytes metric");

    /// Gas used per transaction
    pub static ref TX_GAS_USED: HistogramVec = HistogramVec::new(
        prometheus::HistogramOpts::new(
            "tx_gas_used",
            "Gas used per transaction"
        ).buckets(vec![10000.0, 50000.0, 100000.0, 200000.0, 500000.0, 1000000.0]),
        &["tx_type"]
    ).expect("Failed to create tx_gas_used metric");

    /// Number of failed transactions
    pub static ref TX_FAILED: IntCounter = IntCounter::new(
        "tx_failed",
        "Total number of failed transactions"
    ).expect("Failed to create tx_failed metric");
}

/// Register all core metrics with the provided registry
pub fn register_core_metrics(registry: &Registry) -> MetricResult<()> {
    // Register counters
    registry.register(Box::new(TOTAL_TRANSACTIONS.clone()))?;
    registry.register(Box::new(TOTAL_BLOCKS_PROCESSED.clone()))?;
    registry.register(Box::new(TX_FAILED.clone()))?;

    // Register gauges
    registry.register(Box::new(BLOCK_HEIGHT.clone()))?;
    registry.register(Box::new(MEMPOOL_SIZE.clone()))?;
    registry.register(Box::new(CONNECTED_PEERS.clone()))?;
    registry.register(Box::new(NODE_UPTIME.clone()))?;

    // Register histograms
    registry.register(Box::new(TRANSACTION_PROCESSING_TIME.clone()))?;
    registry.register(Box::new(BLOCK_PROCESSING_TIME.clone()))?;
    registry.register(Box::new(TX_SIZE_BYTES.clone()))?;
    registry.register(Box::new(TX_GAS_USED.clone()))?;

    Ok(())
}

/// Helper function to observe transaction processing time
pub fn observe_transaction_time(tx_type: &str, duration_secs: f64) {
    TRANSACTION_PROCESSING_TIME
        .with_label_values(&[tx_type])
        .observe(duration_secs);
}

/// Helper function to observe block processing time
pub fn observe_block_time(block_type: &str, duration_secs: f64) {
    BLOCK_PROCESSING_TIME
        .with_label_values(&[block_type])
        .observe(duration_secs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_creation() {
        // Test that metrics can be created and accessed without panic
        // Note: We don't test for specific values as metrics are global
        // and may have been modified by other tests
        let _ = BLOCK_HEIGHT.get();
        let _ = TOTAL_TRANSACTIONS.get();
        let _ = MEMPOOL_SIZE.get();
        let _ = TOTAL_BLOCKS_PROCESSED.get();
        let _ = CONNECTED_PEERS.get();
        
        // Test that we can update metrics
        BLOCK_HEIGHT.set(1);
        assert!(BLOCK_HEIGHT.get() >= 1);
    }

    #[test]
    fn test_metric_updates() {
        // Test counter increment
        let initial_txs = TOTAL_TRANSACTIONS.get();
        TOTAL_TRANSACTIONS.inc();
        assert_eq!(TOTAL_TRANSACTIONS.get(), initial_txs + 1);

        // Test gauge set
        BLOCK_HEIGHT.set(100);
        assert_eq!(BLOCK_HEIGHT.get(), 100);

        // Test histogram observation
        observe_transaction_time("transfer", 0.05);
        observe_block_time("normal", 1.5);
    }

    #[test]
    fn test_register_metrics() {
        let registry = Registry::new();
        assert!(register_core_metrics(&registry).is_ok());
    }
}
