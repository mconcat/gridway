# Cosmos SDK Telemetry Compatibility

This document describes how the helium-telemetry crate implements Cosmos SDK compatible metrics and telemetry.

## Compatibility Features

### 1. Metrics Endpoint

- **Path**: `/metrics` (same as Cosmos SDK)
- **Port**: `1317` by default (matching Cosmos SDK API server port)
- **Format Support**: 
  - `?format=prometheus` - Returns Prometheus text format
  - `?format=text` - Returns JSON structure (default, matching Cosmos SDK)

### 2. Response Format

For `?format=text` (default), the response matches Cosmos SDK structure:
```json
{
  "metrics": "<prometheus_text_format_metrics>",
  "content_type": "text/plain"
}
```

### 3. Metric Naming Conventions

Following Cosmos SDK patterns:
- Transaction metrics: `tx_*` prefix
  - `tx_count` - Total transactions
  - `tx_failed` - Failed transactions
  - `tx_processing_time` - Processing duration
  - `tx_size_bytes` - Transaction size
  - `tx_gas_used` - Gas consumption
- Consensus metrics: `consensus_*` prefix
  - `consensus_height` - Current block height
  - `consensus_block_processing_time` - Block processing duration
- Mempool metrics: `mempool_*` prefix
  - `mempool_tx_count` - Transactions in mempool

### 4. Global Labels

Support for global labels as used in Cosmos SDK `app.toml`:
```rust
MetricsServerConfig {
    global_labels: vec![
        ("chain_id".to_string(), "helium-1".to_string()),
        ("node_id".to_string(), "node123".to_string()),
    ],
}
```

### 5. Configuration

The telemetry system can be configured similar to Cosmos SDK:
- Enable/disable metrics collection
- Set custom bind address
- Configure global labels
- Enable health check endpoint

## Usage Example

```rust
use gridway_telemetry::{
    http::{MetricsServerConfig, spawn_metrics_server},
    metrics::{BLOCK_HEIGHT, TOTAL_TRANSACTIONS},
    registry,
};

// Create Cosmos SDK compatible configuration
let config = MetricsServerConfig {
    bind_address: "127.0.0.1:1317".parse()?,
    metrics_path: "/metrics".to_string(),
    enable_health_check: true,
    global_labels: vec![
        ("chain_id".to_string(), "helium-1".to_string()),
    ],
};

// Start metrics server
let registry = registry();
let handle = spawn_metrics_server(registry, config);

// Update metrics
BLOCK_HEIGHT.set(100);
TOTAL_TRANSACTIONS.inc();
```

## Differences from Cosmos SDK

1. **Implementation Language**: Rust instead of Go
2. **Metrics Library**: Uses `prometheus` crate instead of go-metrics
3. **No StatsD/Datadog**: Currently only supports Prometheus sink
4. **Simplified Configuration**: No `app.toml` file; configuration via code

## Future Enhancements

1. Add StatsD sink support
2. Implement metric retention policies
3. Add more granular module-specific metrics
4. Support for custom metric sinks