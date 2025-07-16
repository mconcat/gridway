# ADR-001: The Determinism Challenge - Balancing a Unix API with Consensus

## Status

Accepted

## Context

One of the most critical architectural challenges in the Helium implementation is the tension between providing developers with a familiar, Unix-like API surface via WASI and the absolute need for deterministic execution required for blockchain consensus.

## Decision

The solution involves a multi-layered approach of filtering, overriding, and restricting the standard WASI interface to create a safe, deterministic subset.

### Approach 1: Strict Function Allowlist

The first line of defense is a strict allowlist of WASI functions. The host environment only exposes functions that are inherently deterministic or can be made so.

```rust
// WASI function allowlist implementation
pub struct WasiFunctionFilter {
    allowed_functions: HashSet<String>,
    deterministic_overrides: HashMap<String, Box<dyn HostFunction>>,
}

impl WasiFunctionFilter {
    pub fn new_blockchain_safe() -> Self {
        // ... (populate allowed_functions with safe VFS and memory operations)
        
        // EXPLICITLY FORBIDDEN: clock_time_get, random_get, sock_*, poll_oneoff
        
        let mut deterministic_overrides = HashMap::new();
        
        // Override clock functions with deterministic alternatives
        deterministic_overrides.insert(
            "clock_time_get".to_string(),
            Box::new(DeterministicClockFunction::new())
        );
        
        // ... (other overrides)
    }
}
```

### Approach 2: Deterministic Overrides

For functions that are essential but inherently non-deterministic, such as time and randomness, the system provides deterministic overrides.

#### Time Handling

Instead of accessing system time, a call to `clock_time_get` is intercepted and served by a `BlockchainTimeSource` that returns a value derived from the block context:

```rust
// Deterministic time source
pub struct BlockchainTimeSource {
    block_time: SystemTime,
    block_height: u64,
    tx_index: u32,
}

impl BlockchainTimeSource {
    pub fn get_deterministic_time(&self) -> SystemTime {
        // Return block time, ensuring all validators get the same value
        self.block_time
    }
}
```

#### Randomness

Calls for randomness are handled by a `BlockchainPrng` that generates pseudo-random numbers from deterministic block data:

```rust
// Deterministic pseudo-randomness source
pub struct BlockchainPrng {
    block_hash: [u8; 32],
    transaction_hash: [u8; 32],
    call_counter: Arc<Mutex<u64>>,
}

impl BlockchainPrng {
    pub fn get_deterministic_random(&self, buffer: &mut [u8]) -> Result<(), RandomError> {
        let mut counter = self.call_counter.lock().unwrap();
        
        // Create deterministic seed from block context and a call counter
        let mut hasher = sha2::Sha256::new();
        hasher.update(&self.block_hash);
        hasher.update(&self.transaction_hash);
        hasher.update(&counter.to_le_bytes());
        let seed = hasher.finalize();
        
        // Use a cryptographic PRNG for high-quality deterministic output
        let mut rng = ChaCha20Rng::from_seed(seed.into());
        rng.fill_bytes(buffer);
        *counter += 1;
        Ok(())
    }
}
```

### Approach 3: I/O Restrictions

Network and filesystem access are handled with strict restrictions:

1. **Network Access**: Completely prohibited; all socket-related functions return errors
2. **Filesystem Access**: Funneled through the Virtual Filesystem (VFS) boundary

```rust
// Filesystem virtualization and access validation
pub struct VirtualFilesystemBoundary { /* ... */ }

impl VirtualFilesystemBoundary {
    pub fn validate_path_access(/* ... */) -> Result<(), VfsError> {
        // ... (checks that path is within the module's allowed namespace)
    }
    
    pub fn canonicalize_path(&self, path: &str) -> Result<String, VfsError> {
        let mut components = Vec::new();
        for component in path.split('/') {
            match component {
                "" | "." => continue,
                ".." => {
                    if components.pop().is_none() {
                        return Err(VfsError::PathTraversal { /* ... */ });
                    }
                },
                normal => components.push(normal),
            }
        }
        Ok(format!("/{}", components.join("/")))
    }
}
```

### Approach 4: Cross-Platform Compatibility

To guarantee determinism across different validator machines:

```rust
// WASM runtime configuration for determinism
pub struct DeterministicWasmConfig {
    // ...
    pub compilation_settings: CompilationSettings {
        // Disable optimizations that could vary between platforms
        optimization_level: OptLevel::None,
        deterministic_compilation: true,
        target_features: TargetFeatures::minimal_safe(),
    },
    pub execution_settings: ExecutionSettings {
        // Strict execution limits
        fuel_consumption: true,
        max_stack_depth: 1024,
        trap_on_grow_failure: true,
    },
}
```

## Consequences

### Positive

- Provides a useful subset of WASI without compromising consensus
- Maintains developer familiarity with Unix-like APIs
- Enables deterministic execution across all validators
- Prevents common sources of non-determinism

### Negative

- Restricts functionality compared to full WASI
- Requires careful documentation of what's available vs. forbidden
- May require developers to adapt existing code patterns
- Adds complexity to the host implementation

### Neutral

- Creates a new "blockchain-safe WASI" subset that could become a standard
- Requires ongoing maintenance as WASI evolves

## References

- [WASI Specification](https://wasi.dev/)
- [WebAssembly Determinism](https://webassembly.org/docs/nondeterminism/)
- [Cosmos SDK Determinism Requirements](https://docs.cosmos.network/main/build/building-modules/invariants)