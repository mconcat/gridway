# Helium BaseApp Architecture

This document details the implementation architecture of the Helium BaseApp crate, which provides the WASI microkernel foundation, virtual filesystem abstraction, capability-based security model, and transaction processing engine for the Helium blockchain.

## The WASI Component Model Foundation

The core of Helium's microkernel architecture embraces the WASI 0.2 Component Model, a significant evolution from traditional module-based approaches. Implemented in `helium-baseapp/src/component_host.rs`, the component architecture establishes a type-safe, composable foundation for all blockchain elements. This architectural choice represents a fundamental commitment to the future of WebAssembly standards and provides unprecedented safety guarantees through interface-driven design.

```rust
pub struct ComponentHost<T: ComponentContext> {
    engine: Engine,
    store: Store<T>,
    linker: Linker<T>,
    // Components are cached after loading from VFS, keyed by path
    component_cache: HashMap<String, Component>,
}

impl<T: ComponentContext> ComponentHost<T> {
    pub fn new(ctx: T) -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.consume_fuel(true);
        config.epoch_interruption(true);
        
        let engine = Engine::new(&config)?;
        let mut store = Store::new(&engine, ctx);
        
        // Set fuel for gas metering
        store.set_fuel(1000000)?;
        
        let linker = Linker::new(&engine);
        
        Ok(ComponentHost { 
            engine, 
            store, 
            linker,
            component_cache: HashMap::new()
        })
    }
}
```

This implementation crystallizes several architectural principles that distinguish component-based design from traditional module approaches:

1. **Component Model Enablement**: The `wasm_component_model(true)` configuration activates the full WASI 0.2 component model, enabling rich type systems, resource management, and interface-driven composition that transcends the limitations of simple function exports.

2. **Gas Metering**: Configuring `consume_fuel(true)` enables gas metering at the WASM instruction level, providing deterministic resource accounting essential for blockchain consensus.

3. **Execution Control**: The `epoch_interruption(true)` setting allows the host to interrupt long-running components, establishing hard bounds on execution time to prevent denial-of-service attacks.

The component loading and instantiation process leverages WebAssembly Interface Types (WIT) for type-safe interactions:

```rust
// NOTE: This load_component method is for development/testing
// In production, components are loaded from VFS paths
pub fn load_component_from_file(&mut self, name: &str, path: &Path) -> Result<(), ComponentError> {
    let component = Component::from_file(&self.engine, path)?;
    self.component_registry.insert(name.to_string(), component);
    Ok(())
}

pub fn instantiate_component(&mut self, path: &str) -> Result<ComponentInstance, ComponentError> {
    let component = self.component_registry.get(path)
        .ok_or(ComponentError::ComponentNotFound)?;
    
    // Components are instantiated based on their interface type
    // determined by their path (e.g., /sbin/ante-handler -> AnteHandler interface)
    let instance = match path {
        "/sbin/ante-handler" => self.instantiate_ante_handler(component)?,
        "/sbin/tx-decoder" => self.instantiate_tx_decoder(component)?,
        path if path.starts_with("/bin/") => self.instantiate_module(component)?,
        _ => return Err(ComponentError::UnknownComponentType),
    };
    
    Ok(instance)
}
```

### Dynamic Component Loading Architecture

A revolutionary aspect of Helium's architecture is the storage of WASI components directly in the merkle tree, enabling dynamic program updates through governance. Rather than hard-coding critical blockchain components, the system loads them from well-known paths in the state store:

```rust
// Core system components stored in merkle tree
const SYSTEM_PROGRAMS: &[(&str, &str)] = &[
    ("/sbin/ante-handler", "Core transaction validation"),
    ("/sbin/tx-decoder", "Transaction decoding logic"),  
    ("/sbin/begin-blocker", "Block initialization"),
    ("/sbin/end-blocker", "Block finalization"),
];

impl ComponentHost {
    pub async fn load_system_component(&mut self, path: &str) -> Result<Component, ComponentError> {
        // Load WASM bytecode from merkle storage
        let wasm_bytes = self.vfs.read_file(path).await?;
        
        // Compile and instantiate the component
        let component = Component::from_bytes(&self.engine, &wasm_bytes)?;
        
        // The component's path determines its identity and capabilities
        self.register_component(path, component)?;
        Ok(component)
    }
}
```

This design enables unprecedented flexibility—governance proposals can upgrade core blockchain behavior by storing new WASM programs at these paths. A chain can evolve its validation logic, add new transaction types, or modify block processing without coordinated software upgrades.

### Default System Programs

The `crates/wasi-modules/` directory contains reference implementations of core system programs—ante-handler, tx-decoder, begin-blocker, and end-blocker. These are **not** statically linked into the BaseApp. Instead, they serve as:

1. **Default Options**: During chain initialization, these can be stored at their respective `/sbin` paths
2. **Development References**: Monorepo organization allows coordinated development and testing
3. **Upgrade Templates**: Governance proposals can use modified versions as starting points

```rust
// During chain genesis or development setup
impl ChainInitializer {
    pub async fn install_default_programs(&mut self) -> Result<(), Error> {
        // Load default WASM modules from the build artifacts
        let defaults = [
            ("ante-handler", "/sbin/ante-handler"),
            ("tx-decoder", "/sbin/tx-decoder"),
            ("begin-blocker", "/sbin/begin-blocker"),
            ("end-blocker", "/sbin/end-blocker"),
        ];
        
        for (module_name, install_path) in defaults {
            let wasm_bytes = include_bytes!(concat!("../../../modules/", module_name, ".wasm"));
            self.vfs.write_file(install_path, wasm_bytes).await?;
        }
        
        Ok(())
    }
}
```

This approach maintains the benefits of monolithic development—coordinated changes, integrated testing, type safety across boundaries—while preserving the runtime flexibility of dynamic loading. The reference implementations demonstrate best practices and provide a working system out of the box, but chains are free to replace them through governance.

### Component Interface Architecture

The WASI Component Model fundamentally transforms host-guest communication through WebAssembly Interface Types (WIT). Rather than manual memory management and ABI definitions, the component model provides a declarative, type-safe approach to defining component boundaries. The WIT definitions establish contracts that transcend traditional function signatures:

```wit
interface ante-handler {
    record ante-request {
        tx-bytes: list<u8>,
        tx-index: u32,
        block-height: u64,
        block-time: u64,
        is-check-tx: bool,
        is-recheck-tx: bool,
    }
    
    record ante-response {
        gas-wanted: u64,
        gas-used: u64,
        result-code: u32,
        result-log: string,
        events: list<event>,
    }
    
    process-ante: func(request: ante-request) -> result<ante-response, string>;
}
```

This interface-first approach eliminates entire classes of bugs that plague manual ABI implementations. The component model's type system enforces correctness at the boundary, making invalid states unrepresentable. Memory management, serialization, and error propagation become implementation details handled by the component runtime rather than sources of security vulnerabilities.


### Architectural Milestones Achieved:
- WASI 0.2 Component Model adoption with full type safety guarantees
- WebAssembly Interface Types (WIT) as the definitive interface language
- Dynamic component loading from merkle storage
- Path-based component identity and capability model
- Fuel-based gas metering at the WebAssembly instruction level
- Type-safe bindings generation eliminating manual serialization

### Critical Architecture Work Ahead:
- Virtual Filesystem implementation with POSIX-compatible semantics
- File descriptor-based capability handle system
- Guest-side executable wrappers for fine-grained access control
- CPU time limits and execution timeouts beyond fuel metering
- Memory consumption limits with per-component quotas
- Call stack depth restrictions preventing recursion attacks
- Comprehensive observability integration for distributed tracing
- Governance-controlled program deployment mechanisms

## Virtual Filesystem Design Philosophy

The Virtual Filesystem represents the philosophical heart of Helium's capability system, embracing the profound simplicity of "everything is a file"—a principle that has guided Unix system design for decades and now finds new expression in blockchain architecture.

The VFS is not a traditional filesystem backed by disk storage—it is an abstraction layer over the blockchain's merkle tree state store. Each "file" in the VFS corresponds to a key-value pair in the underlying merkle store, with paths mapping to merkle tree keys and file contents to merkle tree values. This design provides a familiar interface while maintaining the cryptographic integrity and verification properties essential for blockchain consensus.

### Filesystem Hierarchy for Programs

The VFS establishes a Unix-inspired hierarchy for organizing blockchain components:

```
/
├── sbin/           # System binaries (governance-controlled)
│   ├── ante-handler      # Core transaction validation
│   ├── tx-decoder        # Transaction decoding
│   ├── begin-blocker     # Block initialization  
│   └── end-blocker       # Block finalization
├── bin/            # Application modules
│   ├── bank             # Token transfers
│   ├── staking          # Validator staking
│   ├── governance       # Proposal handling
│   └── custom-module    # Chain-specific modules
├── lib/            # Shared libraries and contracts
│   ├── token-v1         # Reusable token implementation
│   └── multisig-v2      # Multisig wallet library
└── home/           # Module state storage
    ├── bank/            # Bank module state
    └── staking/         # Staking module state
```

This hierarchy serves multiple purposes:

1. **Clear Separation**: System components (`/sbin`) require governance approval to modify
2. **Discoverability**: The BaseApp can scan `/bin` to find available modules
3. **Reusability**: Libraries in `/lib` can be shared across modules
4. **State Organization**: Module state lives under `/home/{module}/`

The planned VFS implementation will transform blockchain state interaction into familiar POSIX file operations, where each file descriptor serves as an unforgeable capability handle. This design marries the elegance of Unix philosophy with the security requirements of blockchain systems, creating an interface that is both powerful and comprehensible.

### Architectural Vision

The Virtual Filesystem will provide a POSIX-compatible interface that transforms how components interact with blockchain state:

```rust
// VFS implementation architecture
pub struct VirtualFilesystem {
    state_backend: Arc<dyn StateBackend>,
    capability_manager: Arc<CapabilityManager>,
    fd_table: FileDescriptorTable,
    mount_points: HashMap<PathBuf, MountPoint>,
}

pub struct FileDescriptor {
    handle: u32,
    path: PathBuf,
    mode: OpenMode,
    capability: CapabilityHandle,
    position: u64,
}

impl VirtualFilesystem {
    pub fn open(&mut self, path: &Path, flags: OpenFlags) -> Result<FileDescriptor, VfsError> {
        // Path resolution translates filesystem paths to KVStore operations
        // Each successful open returns a file descriptor that embodies
        // the capability to access that resource
        let capability = self.capability_manager.request_capability(path, flags)?;
        let fd = self.fd_table.allocate(path, capability)?;
        Ok(fd)
    }
}
```

The path structure `/state/{module}/{key}` will provide intuitive namespace isolation. More importantly, the file descriptor abstraction enables fine-grained capability tracking—possession of a file descriptor proves the right to access the underlying resource.

### Capability-Based Security Through File Descriptors

In this model, file descriptors become capability handles in the purest sense. The security properties emerge naturally from the abstraction:

1. **Unforgeability**: File descriptors cannot be manufactured; they must be obtained through proper channels
2. **Delegation**: File descriptors can be passed between components, enabling controlled sharing
3. **Revocation**: Closing a file descriptor immediately revokes access to the resource
4. **Least Privilege**: Components receive only the file descriptors they need, nothing more

A crucial architectural benefit emerges from requiring upfront declaration of all file handles: **deterministic parallelism**. This design pattern, proven successful in Solana's architecture, transforms the traditional sequential blockchain execution model. When a transaction must declare all file descriptors it will access before execution begins, the system gains perfect knowledge of the transaction's data dependencies. This enables safe parallel execution of non-conflicting transactions—those accessing disjoint sets of files can run simultaneously without coordination.

The three-phase purse pattern takes this concept further by temporally separating high-contention operations from parallelizable computation. Consider a typical DeFi scenario where thousands of users interact with the same token contract:

1. **Sequential Bottlenecks**: All withdraw operations must serialize through the token contract
2. **Parallel Paradise**: The actual DEX swaps, liquidity calculations, and order matching run in parallel
3. **Sequential Convergence**: Final deposits serialize again through the token contract

This pattern transforms what would be fully sequential execution into a parallel computation sandwich. The file descriptors serve triple duty—they are capability handles, dependency declarations, and temporal coordination tokens. By making state dependencies explicit through purse files, the system can automatically identify and exploit parallelism opportunities.

Future optimizations could employ Aptos-style accumulators to parallelize even the withdraw/deposit phases for commutative operations, but the current design already enables order-of-magnitude improvements in throughput for complex DeFi operations. This elegant convergence of security, correctness, and performance represents a fundamental advance in blockchain architecture.

The enforcement happens not in the BaseApp but within the guest environment itself. Executable wrappers will mediate access to file descriptors, implementing fine-grained policies that the BaseApp need not understand:

```rust
// Guest-side enforcement concept
pub struct SecureExecutor {
    allowed_paths: Vec<PathPattern>,
    forbidden_operations: Vec<Operation>,
}

impl SecureExecutor {
    pub fn wrap_component(&self, component: Component) -> SecureComponent {
        // The wrapper intercepts filesystem operations,
        // enforcing policies before delegating to the actual component
        SecureComponent::new(component, self.allowed_paths.clone())
    }
}
```

### Interface Files and Capability Transfer

Beyond state access, the VFS will support special "interface files" that provide structured communication channels:

- `/dev/ibc/channel-0` - Inter-blockchain communication channels
- `/proc/self/gas` - Current gas consumption metrics
- `/sys/block/height` - Current block information
- `/tmp/events` - Event emission interface

Most importantly, the VFS enables secure resource transfer through temporary "purse files"—a pattern that exemplifies the object capability principle in action. These purse files are not mere data representations but actual authority tokens created by the resource's origin contract.

Consider the three-phase pattern that enables maximal parallelism in complex transactions:

```rust
// Message 1: Withdraw Phase - Must touch token contract state
let withdraw_msg = Message {
    to: token_contract,
    action: "create_purse",
    params: PurseRequest { amount: 1000, recipient: dex_contract }
};
// Returns: purse_fd (file descriptor to temporary purse file)

// Message 2: Execution Phase - Operates solely within DEX contract
let execute_msg = Message {
    to: dex_contract,
    action: "swap",
    params: SwapRequest { 
        input_purse: purse_fd,  // From previous message
        output_denom: "uosmo"
    }
};
// Returns: output_purse_fd

// Message 3: Finalize Phase - Returns to token contract state
let finalize_msg = Message {
    to: token_contract,
    action: "deposit_purse",
    params: DepositRequest {
        purse: output_purse_fd,
        recipient: user_address
    }
};
```

This pattern achieves remarkable parallelism properties. The withdraw and finalize phases must be executed sequentially as they modify the token contract's balance state. However, the execution phase—often the most computationally intensive—can run in parallel across thousands of transactions. Each transaction operates on its own purse files, touching only contract-local state.

The purse file contains not just data but actual spendable authority recognized by the issuing contract. This prevents double-spending while enabling parallel execution—a traditionally difficult problem in blockchain systems. The filesystem abstraction thus becomes a universal medium for capability-based resource management, where possession of a file descriptor constitutes proof of resource ownership.

TODO: design how these ownership of file descriptors can be expressed in rust's ownership system, with references passed between programs.

### Transaction Isolation Through State Snapshots

The VFS operates on a fundamental principle: each transaction executes against its own isolated snapshot of the blockchain state. This design eliminates entire classes of concurrency bugs while enabling safe parallelism:

```rust
impl VirtualFilesystem {
    pub fn create_transaction_view(&self, base_state: StateRoot) -> TransactionView {
        // Each transaction sees a consistent view of the state
        // Modifications are buffered until commit
        TransactionView {
            base: base_state.clone(),
            modifications: HashMap::new(),
            accessed_files: HashSet::new(),
        }
    }
}
```

From the component's perspective, it has exclusive access to the entire filesystem. Parallel transactions reading the same files see identical values, while writes are buffered in transaction-local storage. Only upon successful completion does a transaction's modifications become visible to others. This model provides perfect isolation without the complexity of traditional database locking mechanisms.

### Performance Engineering

While the VFS abstraction introduces overhead compared to direct KVStore access, careful engineering can minimize the impact. The key insight is that the VFS layer can perform intelligent caching and batching:

```rust
// Optimized VFS operations
impl VirtualFilesystem {
    pub fn read_cached(&mut self, fd: FileDescriptor, buffer: &mut [u8]) -> Result<usize, VfsError> {
        // Check read-ahead cache first
        if let Some(cached_data) = self.cache.get(fd.path, fd.position) {
            return Ok(copy_to_buffer(cached_data, buffer));
        }
        
        // Perform read with intelligent prefetching
        let data = self.read_with_prefetch(fd)?;
        Ok(copy_to_buffer(data, buffer))
    }
}
```

The abstraction also enables sophisticated optimizations like memory-mapped files for frequently accessed state, reducing the overhead for hot paths while maintaining the security properties of the capability system.

### Security Architecture

The VFS security model addresses the critical vulnerabilities identified in earlier designs through comprehensive path canonicalization:

```rust
impl VirtualFilesystem {
    fn canonicalize_path(&self, path: &Path) -> Result<PathBuf, VfsError> {
        // Resolve symbolic links
        // Normalize Unicode representations  
        // Eliminate directory traversal attempts
        // Validate against mount points
        // Apply security policies
        canonical_path_resolver::resolve(path, &self.mount_points)
    }
}
```

This canonicalization happens once at file open time, establishing a secure binding between the file descriptor and the authorized resource. Subsequent operations need only validate the file descriptor, not the path, improving both security and performance.

## Component Types and Their Philosophies

The WASI Component Model enables Helium to define distinct component types, each serving a specific role in the blockchain's operation. These components interact through well-defined interfaces, creating a system that is both modular and comprehensible.

### Ante Handler Components

The ante handler represents the gateway to the blockchain, validating transactions before they consume state-changing resources. The WIT interface captures this responsibility elegantly:

```wit
interface ante-handler {
    process-ante: func(request: ante-request) -> result<ante-response, string>;
}
```

Every transaction must pass through the ante handler's scrutiny. This component validates signatures, ensures sufficient fees, checks nonces, and performs any custom validation logic the chain requires. The beauty of the component model is that ante handlers can be composed—multiple ante handler components can form a pipeline, each checking different aspects of transaction validity.

The philosophical importance of separating ante handling into its own component cannot be overstated. It establishes a clear security boundary where all transactions are validated before entering the system, and it allows chains to customize their validation logic without modifying core infrastructure.

### Transaction Decoder Components

Transaction decoding might seem like a simple technical necessity, but it represents a profound architectural decision. By isolating decoding logic into its own component, Helium enables:

```wit
interface tx-decoder {
    decode-tx: func(tx-bytes: list<u8>) -> result<decoded-tx, string>;
}
```

This separation allows chains to support multiple transaction formats simultaneously. A chain might accept both Cosmos SDK transactions and Ethereum-compatible transactions, with different decoder components handling each format. The component model ensures type safety at these boundaries, preventing malformed transactions from propagating into the system.

### Block Processing Components

Begin and end blocker components embody the blockchain's heartbeat, executing logic at the boundaries of each block:

```wit
interface begin-blocker {
    begin-block: func(request: begin-block-request) -> result<begin-block-response, string>;
}

interface end-blocker {
    end-block: func(request: end-block-request) -> result<end-block-response, string>;
}
```

These components handle critical tasks like validator set updates, reward distribution, and governance proposals. By componentizing these operations, chains can compose complex block processing logic from simpler, auditable pieces. A DeFi chain might include an end blocker for liquidations, while a gaming chain might process match results in its begin blocker.

### The Three Worlds of Module Components

The module component architecture embraces a spectrum of complexity, recognizing that not all blockchain logic requires the same level of sophistication. This three-tier design enables developers to choose the appropriate abstraction level for their use case, optimizing for simplicity, power, or composability as needed.

#### SDK-Style Modules

The first tier encompasses traditional blockchain modules, offering a familiar programming model for developers coming from Cosmos SDK or similar frameworks:

```wit
interface sdk-module {
    execute-tx: func(request: tx-request) -> result<tx-response, string>;
    query: func(request: query-request) -> result<query-response, string>;
    
    // Optional hooks for module lifecycle
    init-genesis: func(state: genesis-state) -> result<init-response, string>;
    export-genesis: func() -> result<genesis-state, string>;
}
```

These modules handle discrete business logic—token transfers, governance proposals, staking operations. They operate within well-defined boundaries, accessing only their designated state namespace and interacting with other modules through structured messages. This tier serves as the workhorse of most blockchain applications.

#### Chain-in-Chain Modules

The second tier introduces a revolutionary concept: full blockchain emulation within a module. These components expose the complete ABCI 2.0 interface, effectively running as sovereign chains within the parent chain:

```wit
interface chain-module {
    // Full ABCI 2.0 method set
    init-chain: func(request: init-chain-request) -> result<init-chain-response, string>;
    prepare-proposal: func(request: prepare-proposal-request) -> result<prepare-proposal-response, string>;
    process-proposal: func(request: process-proposal-request) -> result<process-proposal-response, string>;
    finalize-block: func(request: finalize-block-request) -> result<finalize-block-response, string>;
    commit: func() -> result<commit-response, string>;
    
    // Chain-specific query interface
    query: func(request: query-request) -> result<query-response, string>;
    
    // State synchronization hooks
    export-state: func() -> result<chain-state, string>;
    import-state: func(state: chain-state) -> result<(), string>;
}
```

Imagine deploying an entire DeFi ecosystem as a single module, complete with its own block height, validator set simulation, and finalization logic. This enables unprecedented experimentation—developers can prototype new consensus mechanisms, test economic models, or run specialized application chains, all within the security envelope of the parent chain. The parent chain provides the ultimate settlement layer while the chain-in-chain module maintains its own state machine and processing logic.

#### Shell-Like Executables

The third tier embraces radical simplicity, providing a command-line-inspired interface for lightweight operations:

```wit
interface shell-executable {
    // Simple string-based interface
    execute: func(args: list<string>) -> result<string, string>;
    
    // Optional: structured input/output for better composability
    execute-structured: func(input: value) -> result<value, string>;
}
```

These components operate like Unix utilities—small, focused, composable. A multisig executable might parse commands like `add-signer alice` or `submit-tx 0x123...`. A text processing utility could transform on-chain data formats. A capability checker might validate permissions with simple string patterns.

The beauty lies in accessibility—developers can write blockchain logic without understanding complex state machines or transaction formats. These executables can be composed into sophisticated workflows through scripting, making blockchain development accessible to a broader audience.

#### Unified Component Model

Despite their different abstraction levels, all three tiers share the same component model foundation. They run in the same sandboxed environment, access state through the same VFS interface, and interact through the same capability system. This unified model enables seamless composition:

```rust
// A chain-in-chain module can invoke shell executables
let multisig_result = shell_executor.execute(vec!["check-approval", "tx-123"])?;

// An SDK module can delegate to a chain-in-chain for complex operations  
let sub_chain_result = chain_module.deliver_tx(wrapped_tx)?;

// Shell executables can query SDK modules
let balance = sdk_module.query(QueryBalance { address: "alice" })?;
```

This architectural flexibility transforms blockchain development from a specialized discipline into a continuum of options, each appropriate for different use cases and developer skill levels.

### Inter-Component Communication Through Object Capabilities

The component ecosystem operates on pure object capability principles—components can only interact with resources they explicitly possess handles for. This isn't merely a security measure; it's the fundamental organizing principle that enables safe, composable systems.

When components need to communicate, they pass capabilities as arguments:

```rust
// Component A wants Component B to process some data
let data_file = vfs.create_temp("/tmp/data/request-{uuid}")?;
vfs.write(data_file, &processing_request)?;

// Pass both the data AND the component handle
let result = component_b.process(ProcessRequest {
    data: data_file,
    callback: component_c_handle,  // B can call C if needed
    permission: permission_token,   // Proof of authorization
})?;
```

Component handles themselves are capabilities—possessing a handle to another component grants the ability to invoke it. The system makes no distinction between file descriptors, component handles, and other capabilities; they're all unforgeable tokens that confer specific powers.

This design eliminates ambient authority—components cannot discover or access other components through global namespaces. Every interaction requires explicit capability passing, creating clear audit trails and preventing confused deputy attacks. The beauty lies in the simplicity: if you have a handle, you can use it; if you don't, you can't.

### Component Identity and Access Control

While components cannot freely discover each other, the system provides controlled mechanisms for capability distribution:

```rust
// Components are identified by unique IDs, but IDs alone grant no power
pub struct ComponentId(String);

// Component handles combine identity with access capability
pub struct ComponentHandle {
    id: ComponentId,
    capabilities: CapabilitySet,
    interface: InterfaceType,
}

// The transaction must provide handles for all components it wants to use
pub struct Transaction {
    entry_point: ComponentId,
    provided_handles: HashMap<String, ComponentHandle>,
    provided_files: HashMap<String, FileDescriptor>,
}
```

This approach maintains the principle of least privilege while enabling practical blockchain development. Users construct transactions by assembling the required capabilities—component handles, file descriptors, special tokens—and the system ensures these capabilities flow correctly through the execution graph.

## Security Through Composition

The security model in the component architecture operates at multiple levels, each reinforcing the others. At the foundation, the WASI component model provides memory isolation and type safety. Components cannot access memory outside their sandbox, cannot forge capabilities, and cannot bypass the type system.

Above this foundation, the Virtual Filesystem ensures state separation through path-based isolation. Each component operates within its designated namespace in the filesystem hierarchy, making it impossible to accidentally or maliciously access another component's state. This isn't just a security measure—it's a correctness guarantee that makes reasoning about system behavior tractable.

The capability system will layer atop these primitives, but crucially, fine-grained capability enforcement happens within the guest environment. The BaseApp provides the mechanism (isolated components with controlled resources), while policy enforcement happens through executable wrappers that understand the specific security requirements of each chain.

This separation of mechanism and policy represents a fundamental design principle. The BaseApp doesn't need to understand whether a component should access specific paths or operations—it simply provides the tools for guest-side code to enforce these policies. This approach maintains flexibility while ensuring security.

### Future Capability Architecture

The planned capability system will integrate naturally with the component model:

```rust
// Future capability integration concept
impl ComponentHost {
    pub fn instantiate_with_capabilities<T: Component>(
        &mut self,
        component: &Component,
        capabilities: CapabilitySet,
    ) -> Result<T::Instance, Error> {
        // Components receive exactly the resources their capabilities permit
        let resources = self.resolve_capabilities_to_resources(capabilities)?;
        
        // Instantiation fails if required capabilities are missing
        component.instantiate_with_resources(&mut self.store, resources)
    }
}
```

Capabilities become first-class citizens in the component ecosystem, with each component declaring its required capabilities in its interface. This declarative approach enables static analysis of security properties and prevents capability confusion at runtime.

## BaseApp and Transaction Processing

The `BaseApp` implementation fundamentally reimagines transaction processing through the lens of dynamic component composition. Unlike traditional monolithic approaches, Helium's `BaseApp` orchestrates components loaded from the merkle tree, enabling runtime evolution of blockchain behavior.

```rust
pub struct BaseApp<T: ComponentContext> {
    component_host: ComponentHost<T>,
    vfs: VirtualFilesystem,
    // Components are loaded dynamically from storage
    system_components: HashMap<String, ComponentInstance>,
}

impl<T: ComponentContext> BaseApp<T> {
    pub async fn initialize(&mut self) -> Result<(), BaseAppError> {
        // Load system components from well-known paths
        self.load_component_if_exists("/sbin/ante-handler").await?;
        self.load_component_if_exists("/sbin/tx-decoder").await?;
        self.load_component_if_exists("/sbin/begin-blocker").await?;
        self.load_component_if_exists("/sbin/end-blocker").await?;
        
        // Load module components from /bin directory
        for entry in self.vfs.list_directory("/bin").await? {
            if entry.is_wasm_component() {
                self.load_component_if_exists(&entry.path).await?;
            }
        }
        
        Ok(())
    }
    
    pub async fn execute_transaction(&mut self, tx_bytes: &[u8]) -> Result<TxResponse, BaseAppError> {
        // 1. Decode transaction through dynamically loaded decoder
        let decoder = self.system_components.get("/sbin/tx-decoder")
            .ok_or(BaseAppError::NoDecoderConfigured)?;
        let decoded_tx = decoder.decode_tx(tx_bytes).await?;
        
        // 2. Validate through ante handler if present
        if let Some(ante_handler) = self.system_components.get("/sbin/ante-handler") {
            let ante_request = AnteRequest {
                tx_bytes: tx_bytes.to_vec(),
                tx_index: self.current_tx_index(),
                block_height: self.current_height(),
                block_time: self.current_time(),
                is_check_tx: false,
                is_recheck_tx: false,
            };
            
            let ante_response = ante_handler.process_ante(ante_request).await?;
            
            if ante_response.result_code != 0 {
                return Err(BaseAppError::AnteHandlerRejection(ante_response.result_log));
            }
        }
        
        // 3. Route messages based on their target paths
        let mut responses = Vec::new();
        for message in decoded_tx.messages {
            let component_path = self.resolve_message_handler(&message)?;
            let handler = self.get_or_load_component(&component_path).await?;
            let response = handler.execute_message(message).await?;
            responses.push(response);
        }
        
        Ok(TxResponse::from_responses(responses))
    }
}
```

### Component Orchestration Philosophy

The beauty of the component model lies not in individual components but in their dynamic composition. The `BaseApp` serves as a conductor, orchestrating components loaded from storage according to the blockchain's evolving requirements. This orchestration follows clear principles:

1. **Separation of Concerns**: Each component type handles exactly one aspect of blockchain operation
2. **Dynamic Evolution**: Components can be upgraded through governance without hard forks
3. **Path-Based Identity**: Component location in the filesystem determines its role and capabilities
4. **Fail-Safe Design**: Component failures are isolated and handled gracefully
5. **Deterministic Execution**: The same inputs always produce the same outputs across all nodes

Message routing now follows a path-based resolution strategy:

```rust
impl<T: ComponentContext> BaseApp<T> {
    pub fn resolve_message_handler(&self, message: &Message) -> Result<String, RouterError> {
        // Messages specify their target module path
        // e.g., {"to": "/bin/bank", "action": "send", ...}
        let module_path = message.to.as_ref()
            .ok_or(RouterError::NoTargetSpecified)?;
        
        // Validate the path points to an executable component
        if !self.vfs.exists(module_path).await? {
            return Err(RouterError::ComponentNotFound(module_path.to_string()));
        }
        
        Ok(module_path.to_string())
    }
    
    pub async fn get_or_load_component(&mut self, path: &str) -> Result<&ComponentInstance, ComponentError> {
        // Check if component is already loaded
        if !self.system_components.contains_key(path) {
            // Load from storage on demand
            self.load_component_if_exists(path).await?;
        }
        
        self.system_components.get(path)
            .ok_or(ComponentError::ComponentNotFound)
    }
}
```

This approach eliminates the need for a separate ModuleRouter, as the filesystem itself becomes the routing table. Components are discovered and loaded dynamically based on their paths, enabling runtime extensibility.

### Block Lifecycle Management

Block processing in the component architecture transforms from monolithic functions into a carefully choreographed sequence of component interactions. Each block's lifecycle follows a predictable pattern that ensures consistency across the network:

```rust
impl<T: ComponentContext> BaseApp<T> {
    pub async fn begin_block(&mut self, header: BlockHeader) -> Result<Vec<Event>, BlockError> {
        let mut all_events = Vec::new();
        
        // Load and execute begin blocker if present
        if let Some(begin_blocker) = self.system_components.get("/sbin/begin-blocker") {
            let request = BeginBlockRequest {
                header: header.clone(),
                last_commit_info: self.get_last_commit_info(),
                byzantine_validators: self.get_byzantine_validators(),
            };
            
            let response = begin_blocker.begin_block(request).await?;
            all_events.extend(response.events);
        }
        
        Ok(all_events)
    }
    
    pub async fn end_block(&mut self, height: u64) -> Result<EndBlockResponse, BlockError> {
        let mut validator_updates = Vec::new();
        let mut consensus_param_updates = None;
        let mut all_events = Vec::new();
        
        // Load and execute end blocker if present
        // In practice, the beginblocker/endblocker should be prefetched before running the ProcessBlock.
        // This is because any changes to the core programs should take effect from the next height.
        if let Some(end_blocker) = self.system_components.get("/sbin/end-blocker") {
            let request = EndBlockRequest { height };
            let response = end_blocker.end_block(request).await?;
            
            validator_updates.extend(response.validator_updates);
            if response.consensus_param_updates.is_some() {
                consensus_param_updates = response.consensus_param_updates;
            }
            all_events.extend(response.events);
        }
        
        Ok(EndBlockResponse {
            validator_updates,
            consensus_param_updates,
            events: all_events,
        })
    }
}
```

The component model ensures that block processing remains deterministic even with multiple components. Each component operates in isolation, unable to affect others except through well-defined return values. This isolation prevents the subtle non-determinism bugs that plague monolithic implementations.

### Error Handling Philosophy

The component model provides natural isolation boundaries that prevent component failures from corrupting the overall system state. When a component fails, the failure is contained within that component's sandbox:

```rust
impl<T: ComponentContext> BaseApp<T> {
    async fn execute_component_safely<R, F>(&mut self, 
                                           component_name: &str,
                                           operation: F) -> Result<R, ComponentError>
    where
        F: Future<Output = Result<R, ComponentError>>
    {
        // Set up component execution environment with resource limits
        let execution_guard = self.component_host.create_execution_guard(component_name)?;
        
        // Execute with timeout and resource monitoring
        match timeout(self.component_timeout, operation).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(e)) => {
                // Component returned an error - this is normal flow
                self.record_component_error(component_name, &e);
                Err(e)
            }
            Err(_) => {
                // Component exceeded timeout - potential DoS
                self.record_component_timeout(component_name);
                Err(ComponentError::Timeout)
            }
        }
    }
}
```

### Resilience Patterns

The architecture incorporates several resilience patterns that emerge naturally from component isolation:

1. **Bulkheading**: Each component runs in its own sandbox, preventing failures from spreading
2. **Timeouts**: Every component operation has a maximum execution time
3. **Resource Limits**: Components cannot exhaust system resources due to fuel and memory limits
4. **Graceful Degradation**: Optional components can fail without stopping block processing
5. **Hot Reloading**: Components can be updated without stopping the blockchain

### Governance-Controlled Evolution

The dynamic loading architecture enables sophisticated governance mechanisms:

```rust
// Governance proposal to upgrade ante handler
pub struct UpgradeComponentProposal {
    pub title: String,
    pub description: String,
    pub component_path: String,  // e.g., "/sbin/ante-handler"
    pub new_wasm_code: Vec<u8>,
    pub activation_height: u64,
}

impl GovernanceHandler for ComponentUpgradeHandler {
    async fn execute_proposal(&mut self, proposal: Proposal) -> Result<(), ProposalError> {
        if let Proposal::UpgradeComponent(upgrade) = proposal {
            // Store the new component at the specified path
            self.vfs.write_file(&upgrade.component_path, &upgrade.new_wasm_code).await?;
            
            // The BaseApp will automatically load the new version
            // on the next block after activation_height
            self.schedule_reload(&upgrade.component_path, upgrade.activation_height)?;
        }
        Ok(())
    }
}
```

This mechanism allows chains to evolve their core behavior through democratic governance rather than coordinated upgrades, representing a fundamental advance in blockchain upgradeability.

### Security Considerations for Dynamic Loading

While dynamic component loading provides unprecedented flexibility, it requires careful security considerations:

1. **Code Validation**: All uploaded WASM code must pass validation before storage
2. **Permission Boundaries**: Only governance can modify `/sbin` system components
3. **Rollback Capability**: Previous versions are retained for emergency rollback
4. **Gradual Rollout**: New components can be tested on a subset of validators first

## See Also

- [Virtual Filesystem Architecture](../helium-store/PLAN.md) - Details on the VFS and state storage
- [Project Overview and Vision](../../PLAN.md) - High-level architectural vision
- [Server Integration](../helium-server/PLAN.md) - How components integrate with ABCI 2.0