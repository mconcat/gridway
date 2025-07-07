//! ABCI Server Configuration

use serde::{Deserialize, Serialize};

/// ABCI server configuration options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbciConfig {
    /// Listen address for ABCI connections (e.g., "tcp://0.0.0.0:26658")
    pub listen_address: String,

    /// Optional gRPC address (e.g., "0.0.0.0:9090")
    pub grpc_address: Option<String>,

    /// Maximum number of concurrent connections
    pub max_connections: usize,

    /// Interval between flushes in milliseconds
    pub flush_interval: u64,

    /// Number of blocks between state persistence
    pub persist_interval: u64,

    /// Number of recent blocks to retain
    pub retain_blocks: u64,

    /// Chain ID for the network
    pub chain_id: String,

    /// Directory for storing snapshots (optional)
    pub snapshot_dir: Option<String>,

    /// Interval between snapshots (in blocks)
    pub snapshot_interval: u64,

    /// Maximum number of snapshots to keep
    pub max_snapshots: usize,

    /// Maximum number of validators
    pub max_validators: usize,
}

impl Default for AbciConfig {
    fn default() -> Self {
        Self {
            listen_address: "tcp://0.0.0.0:26658".to_string(),
            grpc_address: Some("0.0.0.0:9090".to_string()),
            max_connections: 10,
            flush_interval: 100,
            persist_interval: 1,
            retain_blocks: 0, // Keep all blocks by default
            chain_id: "helium-1".to_string(),
            snapshot_dir: Some("data/snapshots".to_string()),
            snapshot_interval: 10000, // Every 10k blocks
            max_snapshots: 3,
            max_validators: 100,
        }
    }
}
