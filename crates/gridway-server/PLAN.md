# Gridway Server Architecture

This document details the architectural vision of the Gridway Server crate, which orchestrates the blockchain node's external interfaces—ABCI 2.0 for consensus integration, and dynamically generated REST/gRPC APIs for client interactions—while seamlessly bridging the WASI component architecture with the broader blockchain ecosystem.

## Dynamic Service Generation Philosophy

Unlike traditional blockchain frameworks that hard-code module services, Gridway embraces a revolutionary approach: all client-facing APIs are dynamically generated from WASI component interfaces. This design eliminates the maintenance burden of manually updating service definitions and ensures perfect consistency between component capabilities and exposed APIs.

### Component Interface Discovery

The server discovers available component interfaces at startup, introspecting each WASI component to extract its query and transaction capabilities:

```rust
// Conceptual service generation from component interfaces
pub struct DynamicServiceGenerator {
    component_registry: ComponentRegistry,
    service_builder: ServiceBuilder,
}

impl DynamicServiceGenerator {
    pub async fn generate_services(&self) -> Result<ServiceCollection, Error> {
        let mut services = ServiceCollection::new();
        
        // Discover all components and their interfaces
        for (name, component) in self.component_registry.iter() {
            let interface = component.inspect_interface()?;
            
            // Generate gRPC service from component query interface
            if let Some(query_interface) = interface.query_methods() {
                let grpc_service = self.service_builder
                    .build_grpc_service(name, query_interface)?;
                services.register_grpc(grpc_service);
            }
            
            // Generate REST endpoints from the same interface
            let rest_endpoints = self.service_builder
                .build_rest_endpoints(name, interface)?;
            services.register_rest(rest_endpoints);
        }
        
        Ok(services)
    }
}
```

This approach ensures that adding a new component automatically exposes its functionality through both gRPC and REST without any manual service definition updates.

### Standard Transaction Services

While module-specific services are dynamically generated, the server provides standard transaction broadcast services that work uniformly across all components:

```rust
// Universal transaction handling independent of component specifics
impl TransactionService {
    async fn broadcast_tx(&self, tx_bytes: Vec<u8>, mode: BroadcastMode) -> Result<TxResponse, Error> {
        match mode {
            BroadcastMode::Sync => self.check_tx(tx_bytes).await,
            BroadcastMode::Async => self.submit_tx(tx_bytes).await,
            BroadcastMode::Commit => self.deliver_tx(tx_bytes).await,
        }
    }
}

## gRPC and REST API Generation

The server automatically generates both gRPC services and REST endpoints from component interfaces, ensuring consistency and reducing maintenance overhead:

### gRPC Service Generation

Each component's WIT interface translates into a fully-featured gRPC service:

```wit
// Component's WIT interface
interface bank-queries {
    get-balance: func(address: string, denom: string) -> result<balance, error>;
    get-all-balances: func(address: string) -> result<list<balance>, error>;
    get-supply: func(denom: string) -> result<supply, error>;
}
```

This automatically generates:
- Protobuf definitions for request/response types
- gRPC service implementation that routes to the component
- Proper error handling and type conversions

### REST API Generation

The same WIT interfaces produce RESTful endpoints following Cosmos SDK conventions:

```
GET /cosmos/bank/v1beta1/balances/{address}
GET /cosmos/bank/v1beta1/balances/{address}/by_denom?denom={denom}
GET /cosmos/bank/v1beta1/supply/{denom}
```

The generation process:
1. Analyzes WIT function signatures to determine HTTP methods
2. Creates intuitive URL paths based on component and function names
3. Handles parameter binding from path and query parameters
4. Automatically generates OpenAPI/Swagger documentation

## ABCI 2.0 Integration with CometBFT

The server implements the ABCI 2.0 protocol for integration with CometBFT, the evolution of Tendermint Core. This interface enables the consensus engine to drive the application's state machine through an enhanced set of methods that provide greater control over block production:

```rust
// ABCI 2.0 implementation with enhanced consensus integration
pub struct AbciApplication {
    base_app: Arc<Mutex<BaseApp>>,
    component_host: Arc<ComponentHost>,
}

impl AbciApplication {
    // ABCI 2.0 methods for enhanced block production
    async fn prepare_proposal(&self, request: PrepareProposalRequest) -> PrepareProposalResponse {
        // Application can reorder, add, or remove transactions
        let optimized_txs = self.base_app.optimize_block_transactions(request.txs)?;
        PrepareProposalResponse { txs: optimized_txs }
    }
    
    async fn process_proposal(&self, request: ProcessProposalRequest) -> ProcessProposalResponse {
        // Validate the proposed block from another validator
        let is_valid = self.base_app.validate_block_proposal(request.txs)?;
        ProcessProposalResponse { 
            status: if is_valid { ProposalStatus::Accept } else { ProposalStatus::Reject }
        }
    }
    
    async fn finalize_block(&self, request: FinalizeBlockRequest) -> FinalizeBlockResponse {
        // ABCI 2.0 combines BeginBlock, DeliverTx, and EndBlock
        let mut events = vec![];
        let mut tx_results = vec![];
        
        // Execute all transactions in the finalized block
        for tx in request.txs {
            let result = self.base_app.execute_transaction(&tx).await?;
            tx_results.push(result);
        }
        
        // Component-based block finalization
        let block_events = self.component_host.finalize_block(request.height).await?;
        events.extend(block_events);
        
        FinalizeBlockResponse {
            events,
            tx_results,
            validator_updates: vec![],
            consensus_param_updates: None,
        }
    }
}
```

### ABCI 2.0 Advantages

1. **PrepareProposal/ProcessProposal**: Enables application-driven block optimization and validation
2. **FinalizeBlock**: Atomic block processing replacing separate BeginBlock/DeliverTx/EndBlock
3. **Vote Extensions**: Support for additional validator-signed data (for oracle prices, etc.)
4. **Enhanced Performance**: Reduced round trips between consensus and application layers

## CLI and Node Management

The server crate provides a comprehensive CLI for node operators, following familiar patterns from Cosmos SDK while adapting to the WASI component architecture:

### Core Commands

```rust
// CLI structure for node operations
pub enum Commands {
    /// Initialize node configuration and genesis state
    Init {
        #[arg(long, default_value = "~/.gridway")]
        home: PathBuf,
        #[arg(long)]
        chain_id: String,
        #[arg(long)]
        moniker: Option<String>,
    },
    
    /// Start the node and all services
    Start {
        #[arg(long, default_value = "~/.gridway")]
        home: PathBuf,
        #[arg(long)]
        with_components: Vec<PathBuf>, // WASI component paths
        #[arg(long)]
        api: bool,
        #[arg(long)]
        grpc: bool,
    },
    
    /// Component management commands
    Component(ComponentCmd),
    
    /// Key management
    Keys(KeysCmd),
    
    /// Genesis file manipulation
    Genesis(GenesisCmd),
    
    /// Node status and debugging
    Status,
    Version,
}
```

### Component Management

Unique to Gridway, the CLI provides component management capabilities:

```rust
pub enum ComponentCmd {
    /// List installed components and their interfaces
    List,
    
    /// Install a new WASI component
    Install {
        path: PathBuf,
        #[arg(long)]
        verify: bool,
    },
    
    /// Inspect component interfaces and capabilities
    Inspect {
        name: String,
    },
    
    /// Generate API documentation from component interfaces
    GenDocs {
        #[arg(long)]
        output: PathBuf,
    },
}
```

### Configuration Management

The node configuration extends standard blockchain settings with component-specific options:

```toml
# config.toml
[components]
enabled = ["bank", "staking", "governance", "custom-dex"]
directory = "./components"
resource_limits = { memory_mb = 512, fuel = 10000000 }

[api]
enable = true
address = "tcp://0.0.0.0:1317"
swagger = true

[grpc]
enable = true
address = "0.0.0.0:9090"
max_recv_msg_size = "10MB"

[abci]
address = "tcp://127.0.0.1:26658"
```

## Service Orchestration

The server orchestrates multiple services that work together to provide a complete blockchain node:

### Service Lifecycle Management

```rust
pub struct NodeServer {
    abci_server: AbciServer,
    grpc_server: Option<GrpcServer>,
    rest_server: Option<RestServer>,
    metrics_server: Option<MetricsServer>,
    component_host: Arc<ComponentHost>,
}

impl NodeServer {
    pub async fn start(&mut self) -> Result<(), Error> {
        // Start ABCI server for CometBFT connection
        self.abci_server.listen("tcp://127.0.0.1:26658").await?;
        
        // Start API servers if enabled
        if let Some(grpc) = &mut self.grpc_server {
            grpc.serve("0.0.0.0:9090").await?;
        }
        
        if let Some(rest) = &mut self.rest_server {
            rest.serve("0.0.0.0:1317").await?;
        }
        
        // Health and metrics
        if let Some(metrics) = &mut self.metrics_server {
            metrics.serve("0.0.0.0:9091").await?;
        }
        
        Ok(())
    }
}
```

### Service Coordination

The services coordinate through shared state and event channels:

1. **ABCI Server**: Drives blockchain state transitions through CometBFT
2. **gRPC Server**: Serves dynamically generated Protobuf services
3. **REST Server**: Provides HTTP/JSON API with Swagger documentation
4. **Metrics Server**: Exposes Prometheus metrics for monitoring
5. **Health Endpoints**: Report node sync status and component health

## Architecture Benefits

The server architecture provides several key advantages:

### Dynamic Extensibility

Adding a new component automatically exposes its functionality through all API layers without code changes. The server introspects component interfaces at startup and generates appropriate service endpoints.

### Operational Simplicity

Node operators work with familiar CLI commands and configuration formats while benefiting from the advanced WASI component architecture underneath. The complexity is hidden behind intuitive interfaces.

### Performance Optimization

- **Lazy Component Loading**: Components load only when first accessed
- **Request Routing**: Direct routing to components without intermediate layers
- **Parallel Query Execution**: Independent queries execute concurrently
- **Resource Isolation**: Each component's resource usage is tracked and limited

### Future Evolution

The server architecture accommodates future enhancements:

1. **Multi-Version Components**: Support multiple versions of the same component
2. **Hot Reload**: Update components without restarting the node
3. **Remote Components**: Execute components on separate machines
4. **Cross-Chain Queries**: Unified API for querying multiple chains

## See Also

- [BaseApp Architecture](../gridway-baseapp/PLAN.md) - Transaction processing and routing
- [WASI Security Model](../gridway-baseapp/PLAN.md#security-and-capability-system-implementation) - Capability-based security
- [Project Overview](../../PLAN.md) - High-level architectural vision