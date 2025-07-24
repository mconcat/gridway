# ADR-003: Integration with Ecosystem Services

## Status

Accepted

## Context

The Gridway SDK must integrate seamlessly with the array of services that support a modern blockchain ecosystem. By maintaining standard API interfaces, it ensures compatibility with existing tools while the microkernel architecture offers new, richer data streams for enhanced functionality.

## Decision

Maintain strict API compatibility while extending interfaces to expose the unique capabilities of the WASI microkernel architecture.

### Block Explorer Compatibility

Standard REST and gRPC API layers provide familiar endpoints:

```rust
// Block explorer endpoint compatibility in gridway-server/src/rest.rs
pub async fn get_block_by_height(
    State(state): State<RestState>,
    Path(height): Path<u64>,
) -> Result<Json<BlockResponse>, StatusCode> {
    // ... (query BaseApp for block data)
}

pub async fn get_transaction_by_hash(
    State(state): State<RestState>,
    Path(tx_hash): Path<String>,
) -> Result<Json<TxResponse>, StatusCode> {
    // ... (query BaseApp for transaction data)
}
```

### Enhanced Analytics Events

The microkernel architecture provides deeper insights through structured events:

```rust
// Enhanced event structure for analytics platforms
#[derive(Debug, Clone, Serialize)]
pub struct AnalyticsEvent {
    pub event_type: String,
    pub module_source: String,
    // ... (block height, transaction index)
    pub attributes: HashMap<String, serde_json::Value>,
    pub execution_context: ExecutionContext, // Includes WASI module, function name, etc.
    pub performance_metrics: PerformanceMetrics,
}
```

This enables:
- Module-level performance tracking
- Detailed execution traces
- Reproducible analytics

### Validator Configuration

Validators require new configuration parameters for the WASI runtime:

```rust
// Validator configuration for WASI microkernel
#[derive(Debug, Clone, Deserialize)]
pub struct ValidatorConfig {
    pub validator_key: ValidatorKey,
    pub consensus_config: ConsensusConfig,
    pub wasi_config: WasiConfig, // New WASI-specific settings
    pub performance_config: PerformanceConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WasiConfig {
    pub module_cache_size_mb: u64,
    pub max_module_memory_mb: u64,
    pub module_execution_timeout_ms: u64,
}
```

### Module Governance and Hot-Swapping

On-chain governance can upgrade modules without chain restarts:

```rust
// Module upgrade coordination
pub struct ModuleUpgradeCoordinator {
    module_cache: Arc<RwLock<ModuleCache>>,
    governance_tracker: GovernanceTracker,
}

impl ModuleUpgradeCoordinator {
    pub async fn apply_approved_upgrade(&self, 
                                       proposal: ModuleUpgradeProposal,
                                       activation_height: u64) -> Result<(), UpgradeError> {
        // Validate new module bytecode
        let validated_module = self.validate_module_bytecode(&proposal.wasm_bytecode)?;
        
        // Stage the upgrade
        self.module_cache.write().unwrap().stage_upgrade(
            proposal.module_id.clone(),
            validated_module,
            activation_height
        )?;
        
        // The actual swap happens automatically at the specified height
        Ok(())
    }
}
```

### Distributed Tracing Integration

Native support for distributed tracing tools:

```rust
use tracing::{instrument, Span};

impl BaseApp {
    #[instrument(skip(self, tx_bytes))]
    pub async fn process_transaction_with_tracing(&mut self, tx_bytes: &[u8]) -> Result<TxResponse, TxError> {
        let span = Span::current();
        
        // Create child spans for each WASI module execution
        let ante_span = tracing::info_span!("ante_handler_execution");
        // ... (execute ante handler with tracing)
        
        let execution_span = tracing::info_span!("wasi_module_execution");
        // ... (execute transaction messages with tracing)
        
        // Attach metrics to the span
        span.record("gas_used", &tx_result.gas_used);
    }
}
```

### Prometheus Metrics

Custom metrics for WASI-specific monitoring:

```rust
// WASI-specific Prometheus metrics
lazy_static! {
    static ref MODULE_EXECUTION_TIME: Histogram = register_histogram!(
        "wasi_module_execution_duration_seconds",
        "Time spent executing WASI modules",
        vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]
    ).unwrap();
    
    static ref MODULE_CACHE_HITS: Counter = register_counter!(
        "wasi_module_cache_hits_total",
        "Number of module cache hits"
    ).unwrap();
    
    static ref HOST_FUNCTION_CALLS: CounterVec = register_counter_vec!(
        "wasi_host_function_calls_total",
        "Number of host function calls by function",
        &["function_name"]
    ).unwrap();
}
```

## Consequences

### Positive

- Full compatibility with existing ecosystem tools
- Enhanced analytics capabilities
- Better debugging and monitoring
- Seamless module upgrades via governance
- Rich performance insights

### Negative

- Additional API surface to maintain
- More complex validator operations
- Higher data volume for analytics
- Learning curve for new monitoring tools

### Neutral

- Sets new standards for blockchain observability
- May influence future Cosmos SDK development
- Creates opportunities for specialized tooling

## Migration Strategy

1. **Phase 1**: Ensure basic API compatibility
2. **Phase 2**: Add enhanced analytics endpoints
3. **Phase 3**: Integrate distributed tracing
4. **Phase 4**: Deploy advanced monitoring dashboards

## References

- [Cosmos SDK REST API](https://docs.cosmos.network/api)
- [OpenTelemetry for Distributed Systems](https://opentelemetry.io/docs/)
- [Prometheus Best Practices](https://prometheus.io/docs/practices/)