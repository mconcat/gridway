# ADR-002: Observability Architecture in a Microkernel

## Status

Accepted

## Context

The WASI microkernel architecture presents unique opportunities for telemetry and observability, offering a level of introspection into module execution that is impossible in monolithic systems. However, collecting this data requires careful design to avoid compromising performance or determinism.

## Decision

Implement a comprehensive observability system using a **host-mediated** approach for consensus-critical paths, with optional direct module metrics for development environments.

### Host-Mediated Metrics Collection

For all consensus-critical execution paths, metrics are collected from outside the WASI sandbox:

```rust
// Host-mediated metrics collection
pub struct HostMetricsCollector { /* ... */ }

impl HostMetricsCollector {
    pub fn record_module_execution(&mut self,
                                 module_id: &str,
                                 execution_context: &ExecutionContext) -> Result<(), MetricsError> {
        let start_time = Instant::now();
        // ... (capture initial state)
        
        // --- WASI module executes externally ---
        
        // Capture final state and calculate deltas
        let execution_time = start_time.elapsed();
        let fuel_consumed = initial_fuel - execution_context.remaining_fuel();
        
        // ... (store metrics)
        Ok(())
    }
}
```

This ensures that observation cannot affect deterministic execution.

### Performance Monitoring System

The architecture enables granular performance monitoring and resource tracking:

```rust
// Comprehensive performance monitoring system
pub struct WasiPerformanceMonitor {
    execution_profiler: ExecutionProfiler,
    memory_tracker: MemoryTracker,
    fuel_analyzer: FuelAnalyzer,
}

impl WasiPerformanceMonitor {
    pub fn complete_execution_monitoring(&mut self, 
                                       session: ExecutionSession) -> Result<ExecutionReport, MonitoringError> {
        // ... (calculate total execution time, memory usage, and fuel consumed)
        
        let report = ExecutionReport {
            // ...
            fuel_consumption: FuelConsumptionReport {
                total_consumed: fuel_consumed,
                instruction_breakdown: self.fuel_analyzer.get_instruction_breakdown(session_id),
                host_function_costs: self.fuel_analyzer.get_host_function_costs(session_id),
            },
            performance_characteristics: self.analyze_performance_characteristics(&session),
        };
        Ok(report)
    }
}
```

### Security Audit Trail

The capability system enables a powerful security audit trail:

```rust
// Security event correlation system
pub struct SecurityAuditSystem {
    event_collector: SecurityEventCollector,
    pattern_analyzer: SecurityPatternAnalyzer,
    anomaly_detector: AnomalyDetector,
}

impl SecurityAuditSystem {
    pub fn record_capability_exercise(&mut self, /* ... */) -> Result<(), AuditError> {
        // ... (create SecurityEvent)
        
        // Real-time anomaly detection
        if let Some(anomaly) = self.anomaly_detector.analyze_event(&event)? {
            self.handle_security_anomaly(anomaly)?;
        }
        
        Ok(())
    }
    
    pub fn analyze_module_behavior_drift(&self, /* ... */) -> BehaviorDriftReport {
        // Compare current behavior profile against a historical baseline
        // to detect significant changes that may indicate a compromise.
    }
}
```

### Distributed Tracing

The structured execution flow enables comprehensive distributed tracing:

```rust
// Distributed tracing system for WASI modules
pub struct WasiDistributedTracer { /* ... */ }

impl WasiDistributedTracer {
    pub fn start_transaction_trace(&mut self, tx_hash: &str) -> TraceContext { /* ... */ }
    
    pub fn start_module_execution_span(&mut self,
                                     context: &TraceContext,
                                     module_id: &str,
                                     function_name: &str) -> Result<Span, TracingError> {
        // Create a child span for the module execution
    }
    
    pub fn complete_trace(&mut self, trace_id: TraceId) -> Result<CompleteTrace, TracingError> {
        // Reconstruct the full execution flow graph from all collected spans
        let execution_flow = self.reconstruct_execution_flow(&spans)?;
        // ... (build the complete trace object for visualization)
    }
}
```

### Direct Module Metrics (Development Only)

For non-deterministic contexts like development and debugging:

```rust
// Optional direct module metrics interface
pub trait ModuleMetrics {
    fn emit_metric(&self, metric: CustomMetric) -> Result<(), MetricsError>;
    fn start_timer(&self, name: &str) -> TimerHandle;
    fn record_gauge(&self, name: &str, value: f64) -> Result<(), MetricsError>;
}

// In production, these calls write to a buffer processed after execution
impl ModuleMetrics for ProductionMetricsHandler {
    fn emit_metric(&self, metric: CustomMetric) -> Result<(), MetricsError> {
        // Buffer the metric for post-execution processing
        self.metric_buffer.lock().unwrap().push(metric);
        Ok(())
    }
}
```

## Consequences

### Positive

- Unprecedented visibility into module execution
- Real-time security anomaly detection
- Detailed performance profiling for optimization
- Complete transaction flow visualization
- No impact on deterministic execution

### Negative

- Additional memory overhead for metrics collection
- Complexity in correlating distributed traces
- Storage requirements for detailed audit trails
- Performance impact of comprehensive monitoring

### Neutral

- Creates new operational requirements for metrics infrastructure
- Enables new categories of blockchain analytics
- May become a differentiating feature for the platform

## Implementation Strategy

1. **Phase 1**: Basic host-mediated metrics
2. **Phase 2**: Security audit trail
3. **Phase 3**: Distributed tracing
4. **Phase 4**: Advanced analytics and anomaly detection

## References

- [OpenTelemetry Specification](https://opentelemetry.io/docs/specs/)
- [Distributed Tracing Best Practices](https://www.w3.org/TR/trace-context/)
- [Security Information and Event Management (SIEM)](https://www.nist.gov/publications/guide-computer-security-log-management)