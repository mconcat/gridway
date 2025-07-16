# Helium Examples

This directory contains examples demonstrating how to build applications on Helium's WASI microkernel architecture.

## Counter Module Example

The counter examples demonstrate a complete application development workflow:

### 1. `counter_module.rs` - Module Development
Shows how to write a blockchain module that:
- Manages state through the Virtual Filesystem (VFS)
- Handles different message types (increment, decrement, reset, query)
- Emits events for blockchain explorers and indexers
- Follows Helium's security model with capability-based access

**Key concepts demonstrated:**
- State stored at VFS paths like `/home/counter/state`
- Message routing based on type field (e.g., `counter/Increment`)
- Event emission for auditability
- Error handling and state protection (overflow/underflow)

### 2. `counter_client.rs` - Client Development
Shows how to build client applications that:
- Construct properly formatted transactions
- Manage nonces and gas limits
- Query module state
- Handle batch operations

**Key concepts demonstrated:**
- Transaction structure with module paths
- Client-side message building
- Query vs state-changing operations
- Module composition patterns

## Architecture Overview

In Helium's architecture:

1. **Modules are WASI Components**: Compiled to WebAssembly and stored in the blockchain's merkle tree
2. **Dynamic Loading**: Modules are loaded from paths like `/bin/counter` or `/home/myapp/bin/counter`
3. **VFS-Based State**: All state access goes through the Virtual Filesystem with capability enforcement
4. **Message Routing**: Messages are routed to modules based on their type field
5. **Governance Upgradeable**: Modules can be upgraded through governance without hard forks

## Running the Examples

These examples are primarily for demonstration and may not compile against the current codebase as the APIs are still evolving. They show the intended developer experience and architectural patterns.

To explore the examples:
```bash
# View the module implementation
cat counter_module.rs

# View the client implementation
cat counter_client.rs

# Run the tests to see example outputs
cargo test -p helium --examples -- --nocapture
```

## Next Steps

For developers interested in building on Helium:

1. **Study the Architecture**: Read `/PLAN.md` for the complete architectural vision
2. **Understand Components**: Learn about the three component types (SDK-style, chain-in-chain, shell-like)
3. **VFS Patterns**: Familiarize yourself with the Virtual Filesystem access patterns
4. **WASI Development**: Learn about WebAssembly Component Model and WASI

## Future Examples

As the platform matures, we plan to add examples for:
- Token/banking modules
- Governance modules
- Inter-module communication
- Chain-in-chain applications
- Shell-like utility programs