//! Example demonstrating Cosmos SDK compatible metrics endpoint
//!
//! This example shows how to start a metrics server that is compatible
//! with Cosmos SDK telemetry standards.

use gridway_telemetry::{
    http::MetricsServerConfig,
    metrics::{BLOCK_HEIGHT, MEMPOOL_SIZE, TOTAL_TRANSACTIONS, TX_FAILED},
    registry,
};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize telemetry
    gridway_telemetry::init()?;

    // Create a custom configuration matching Cosmos SDK defaults
    let config = MetricsServerConfig {
        bind_address: "127.0.0.1:1317".parse()?,
        metrics_path: "/metrics".to_string(),
        enable_health_check: true,
        global_labels: vec![
            ("chain_id".to_string(), "gridway-1".to_string()),
            ("node_id".to_string(), "node123".to_string()),
        ],
    };

    println!("Starting Cosmos SDK compatible metrics server on http://127.0.0.1:1317");
    println!("Endpoints:");
    println!("  - http://127.0.0.1:1317/metrics?format=prometheus - Prometheus format");
    println!("  - http://127.0.0.1:1317/metrics?format=text - Text/JSON format (default)");
    println!("  - http://127.0.0.1:1317/health - Health check endpoint");

    // Get the global registry
    let registry = registry();

    // Spawn the metrics server
    let server_handle = gridway_telemetry::http::spawn_metrics_server(registry, config);

    // Simulate blockchain activity
    let mut height = 1u64;
    let mut total_txs = 0u64;
    let mut failed_txs = 0u64;

    loop {
        // Update metrics
        BLOCK_HEIGHT.set(height as i64);

        // Simulate some transactions
        let tx_count = rand::random::<u8>() % 10;
        for _ in 0..tx_count {
            TOTAL_TRANSACTIONS.inc();
            total_txs += 1;

            // Randomly fail some transactions
            if rand::random::<u8>() % 10 == 0 {
                TX_FAILED.inc();
                failed_txs += 1;
            }
        }

        // Update mempool
        let mempool_count = rand::random::<u8>() % 50;
        MEMPOOL_SIZE.set(mempool_count as i64);

        println!("Block {height}: {total_txs} txs ({failed_txs} failed), mempool: {mempool_count}");

        height += 1;
        sleep(Duration::from_secs(5)).await;
    }
}
