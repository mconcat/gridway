# Helium Store Architecture

This document details the architectural vision of the Helium Store crate, which provides the unified state management foundation for the Helium blockchain through a single global merkle store accessed via Virtual Filesystem semantics.

## Unified State Store Philosophy

Helium makes a fundamental departure from the traditional Cosmos SDK MultiStore architecture by adopting a single, unified merkle store.

### The Single Store Principle

Instead of each module maintaining its own isolated merkle tree, Helium employs a single global merkle store where all state resides. This unified approach eliminates the artificial boundaries between module states while maintaining logical isolation through the Virtual Filesystem abstraction:

```rust
// Conceptual view of state organization
/home/bank/balances/cosmos1abc...           -> 1000000uatom
/home/bank/supply/total                     -> 21000000000000uatom
/home/staking/validators/cosmosvaloper1xyz  -> {validator_data}
/home/dex/orderbook/ETH-USD/asks/1.2345    -> {order_data}
/lib/standard/token-interface               -> {interface_definition}
/tmp/tx-123/purse-456                      -> {temporary_purse_data}
```

This filesystem-inspired organization provides intuitive namespace isolation while maintaining the efficiency of a single merkle tree. Each component operates within its designated directory, unable to access other components' state without explicitly granted capabilities.

### Architectural Advantages

The single store architecture delivers profound benefits:

1. **Unified State Commitment**: A single merkle root represents the entire blockchain state, simplifying consensus and verification
2. **Natural Composability**: Components can seamlessly interact through the filesystem abstraction without artificial barriers
3. **Optimal Proof Generation**: Merkle proofs span the entire state space, enabling efficient cross-component verification
4. **Simplified State Sync**: New nodes synchronize a single merkle tree rather than multiple independent trees

## Transaction Isolation Through Snapshots

Each transaction in Helium executes against its own isolated snapshot of the global merkle store. This design provides perfect isolation without the complexity of traditional locking mechanisms:

```rust
// Conceptual transaction execution model
pub struct TransactionSnapshot {
    base_state: MerkleRoot,
    height: BlockHeight,
    modifications: StateBuffer,
}

// Each transaction sees a consistent view of the world
impl Transaction {
    pub fn execute(&self, snapshot: TransactionSnapshot) -> Result<StateChanges, TxError> {
        // All reads see the snapshot state
        // All writes are buffered locally
        // Other transactions cannot observe these changes
    }
}
```

From the component's perspective, it has exclusive access to the entire filesystem during transaction execution. Multiple transactions can read the same files simultaneously from their snapshots, while writes are buffered until commit. This model enables safe parallelism—transactions touching disjoint state regions can execute concurrently without coordination.

## Filesystem Hierarchy and State Organization

The store maps Virtual Filesystem paths to merkle tree keys, creating an intuitive hierarchy that mirrors traditional Unix filesystems:

### Directory Structure(example)

```
/home/          - Component home directories (read-write)
  bank/         - Bank module state
  staking/      - Staking module state  
  dex/          - DEX application state
  my-contract/  - Custom contract state

/lib/           - Shared libraries (read-only)
  standard/     - Standard interfaces and implementations
  governance/   - Governance-approved shared code

/tmp/           - Transaction-scoped temporary storage
  purse-*       - Temporary purse files
  scratch/      - Working storage

/sys/           - System information (read-only)
  block/        - Current block data
  chain/        - Chain configuration
```

Each component receives access to specific directories through capability-based file descriptors. The `/home/{component}` directory provides complete read-write access for component-specific state, while `/lib` offers read-only access to shared resources. The `/tmp` directory enables transaction-scoped temporary storage that exists only for the transaction lifetime.

### Path-to-Key Mapping

The Virtual Filesystem transparently maps filesystem paths to merkle tree keys:

```rust
// VFS path to merkle key transformation
/home/bank/balances/cosmos1abc -> merkle_key("home:bank:balances:cosmos1abc")
/lib/standard/token -> merkle_key("lib:standard:token")
/tmp/tx-123/purse-456 -> merkle_key("tmp:tx-123:purse-456")
```

This mapping preserves the hierarchical nature while enabling efficient merkle tree operations. The store layer handles this transformation transparently, allowing components to work with familiar filesystem semantics.

## Merkle Tree Abstraction Strategy

### The Merkle Backend Trait

The store layer defines a minimal trait that captures the essential operations needed for blockchain state management:

```rust
// Core merkle tree operations abstraction
trait MerkleBackend {
    type Proof;
    type Root;
    
    // Essential operations - no iteration required
    fn get(&self, key: &[u8], version: u64) -> Result<Option<Vec<u8>>, Error>;
    fn put_batch(&mut self, changes: Vec<(Vec<u8>, Option<Vec<u8>>)>) -> Result<Self::Root, Error>;
    fn get_with_proof(&self, key: &[u8], version: u64) -> Result<(Option<Vec<u8>>, Self::Proof), Error>;
    fn commit(&mut self) -> Result<(Self::Root, u64), Error>;
}
```

This minimal interface deliberately excludes iteration support, as JMT does not provide efficient range queries, and the virtual filesystem can work independently from the existence of iteration feature.

### Implementation Candidates

Two primary implementations are under consideration:

**Jellyfish Merkle Tree (JMT)**
- Proven in production environments (Diem/Aptos)
- Excellent batch operation performance
- Well-understood security properties

**Merk (from Nomic/Turbofish)**
- Designed specifically for high throughput and parallelism
- Innovative approach to merkle tree construction
- Potential for superior performance in parallel execution scenarios

### Empirical Evaluation

The abstraction enables running the same blockchain with different backends, facilitating:

1. **Performance Benchmarking**: Direct comparison under realistic workloads
2. **Parallel Execution Testing**: Evaluation of how each backend handles concurrent access
3. **State Sync Efficiency**: Measuring synchronization performance
4. **Proof Generation Costs**: Comparing the overhead of merkle proof creation

This data-driven approach ensures that the final choice optimizes for Helium's specific requirements rather than relying on generic assumptions.

### Future Evolution

The store architecture accommodates future enhancements without breaking changes:

1. **Adaptive Pruning**: Intelligent strategies that preserve frequently accessed historical states while pruning rarely-used data
2. **Incremental State Sync**: New nodes can begin participating before full sync completes
3. **Cross-Chain Proof Compatibility**: Adapters for IBC and other cross-chain protocols
4. **Advanced Indexing**: Secondary indices for efficient range queries without compromising the primary merkle structure

These enhancements will be developed based on production experience and ecosystem requirements.

## Future Architectural Considerations

The unified store design leaves room for several architectural enhancements:

1. **Performance Optimization**: As usage patterns emerge, specialized indexing and caching strategies can be implemented without changing the fundamental architecture
2. **State Migration**: Tools for migrating from traditional MultiStore architectures to the unified model
3. **Advanced Pruning**: Intelligent strategies that adapt to actual usage patterns
4. **Cross-Chain Integration**: Proof generation and verification for interoperability with other blockchain systems

These enhancements will be guided by production experience and ecosystem needs.

## See Also

- [Virtual Filesystem Architecture](../helium-baseapp/PLAN.md#virtual-filesystem-vfs-and-state-access) - How VFS interacts with the store
- [BaseApp Transaction Processing](../helium-baseapp/PLAN.md#baseapp-and-transaction-processing) - Transaction context and atomicity
- [Project Overview](../../PLAN.md) - High-level architectural vision