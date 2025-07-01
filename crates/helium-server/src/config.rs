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
        }
    }
}