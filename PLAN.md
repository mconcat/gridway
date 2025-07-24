# Gridway: A WASI Microkernel Architecture for Blockchain

## Project Context and Architectural Vision

This document provides a comprehensive technical assessment of the Gridway Rust Cosmos SDK implementation, evaluating its readiness for production and its fidelity to the project's architectural vision. The primary goal of this initiative is to re-architect the Cosmos SDK in Rust as a next-generation **microkernel**. This is not a line-by-line translation of the Go implementation but a fundamental re-evaluation of blockchain architecture, designed to be more performant, more secure, and vastly more flexible.

The architectural vision is centered on four core innovations:

1. **A WASI-based Microkernel:** All application logic is packaged as sandboxed WebAssembly (WASI) components. This decouples the core node software from application logic, enabling runtime module upgrades via on-chain governance and creating a true multi-language development ecosystem.

2. **Dynamic Component Loading:** WASI components are stored directly in the merkle tree at well-known paths (e.g., `/sbin/ante-handler`, `/bin/bank`) and loaded dynamically at runtime. This revolutionary approach enables governance to upgrade core system components without hard forks, as programs are determined by their stored file paths rather than static compilation.

3. **State as a Virtual Filesystem (VFS):** The traditional `MultiStore` is replaced by a single `GlobalAppStore`, with state access mediated through a VFS using Unix-like paths (`/home/{module}/`, `/sys/`, `/tmp/`). This provides a powerful, intuitive, and language-agnostic API for all state interactions.

4. **A Unified Capability Model:** Security is enforced through an Object-Capability (OCAP) model where modules are granted unforgeable handles (as WASI file descriptors) to the specific resources they are permitted to access, embodying the Principle of Least Privilege.

This assessment evaluates the current codebase against this vision. Our methodology combines deep code analysis, architectural review, and an evaluation of ecosystem compatibility. We examined all major components—including `BaseApp`, the WASI runtime, the VFS, and supporting modules—and cross-referenced the implementation against project documentation.

The scope of this review covers the implementation's fidelity to the architectural design, its alignment with critical ecosystem integration requirements (wallets, explorers, IBC), and its readiness for production deployment, considering the needs of validators, developers, and end-users.

## Assessment Scope, Methodology, and Criteria

This assessment evaluates the implementation across four key dimensions: **architectural integrity**, **implementation quality**, **ecosystem compatibility**, and **production readiness**. Our analysis concludes that the project has successfully transitioned from a proof-of-concept to a viable production candidate, validating the core architectural principles of determinism, isolation, and upgradeability. The successful execution of the ante handler as a WASI module, for instance, proves the technical feasibility of the microkernel approach with acceptable performance.

However, while the foundational architecture is sound, the implementation sits at a "validated proof-of-concept with production gaps" stage. Significant work remains to bridge these gaps, particularly in security hardening, performance optimization, and operational tooling.

This architectural document is focused exclusively on the **core SDK framework**—the foundational layers and libraries required to build a blockchain application, not the application-specific business logic itself.

**In Scope:**

- **Microkernel & Engine (`baseapp`):** The core engine managing state, the ABCI lifecycle, and the WASM runtime.
- **WASM Runtime Integration:** The integration of the `wasmtime` engine and the WASI environment, including the VFS and capability system.
- **State Store (`store`):** The Merkleized key-value store (JMT).
- **Core Types (`types`):** Fundamental data structures shared across the SDK.
- **Core Libraries:** Foundational data structures (`types`, `math`), cryptography (`crypto`, `keyring`), and diagnostics (`errors`, `log`).
- **Node Infrastructure:** The node daemon and CLI tooling (`server`, `client`).
- **Testing Framework (`simapp`):** The SDK for property-based and integration testing.

**Out of Scope:**

- **Standard Application Modules (`x/`):** Implementations of modules like `x/bank` or `x/staking` will be built *using* this framework but are not part of its core design.
- **Inter-Blockchain Communication (IBC):** While the SDK is designed to be IBC-compatible, the implementation of the `ibc-go` module's Rust equivalent is a separate undertaking.
- **CometBFT:** We integrate with CometBFT via its ABCI 2.0 interface; a rewrite of the consensus engine itself is out of scope.
- **WASM Runtime Implementation:** We integrate and build upon the existing `wasmtime` runtime; we are not creating a new WebAssembly runtime.

## Component Architecture Documentation

The detailed implementation architecture is documented in component-specific PLAN.md files:

### Core Components

- **[BaseApp Architecture](crates/gridway-baseapp/PLAN.md)**: WASI microkernel foundation, Virtual Filesystem (VFS), capability-based security model, and transaction processing engine
- **[Store Architecture](crates/gridway-store/PLAN.md)**: GlobalAppStore state management, JMT (Jellyfish Merkle Tree) implementation, and persistence layer
- **[Crypto Architecture](crates/gridway-crypto/PLAN.md)**: Cryptographic infrastructure, key management, and signature verification
- **[Server Architecture](crates/gridway-server/PLAN.md)**: Network layer, gRPC/REST APIs, and ABCI implementation
- **[Client Architecture](crates/gridway-client/PLAN.md)**: Wallet integration, transaction building, and developer tools
- **[Types Architecture](crates/gridway-types/PLAN.md)**: Core data structures and minimal protobuf utilities

### Architecture Decision Records

- **[ADR-001: The Determinism Challenge](docs/architecture/ADR-001-determinism-challenge.md)**: Balancing a Unix API with blockchain consensus requirements
- **[ADR-002: Observability Architecture](docs/architecture/ADR-002-observability-architecture.md)**: Telemetry and monitoring in a microkernel system
- **[ADR-003: Ecosystem Integration](docs/architecture/ADR-003-ecosystem-integration.md)**: Compatibility with existing blockchain infrastructure

## Component Types and Execution Model

Gridway supports three distinct component types, enabling diverse development patterns:

1. **SDK-Style Modules:** Traditional blockchain modules implementing specific interfaces (e.g., `bank`, `staking`)
2. **Chain-in-Chain Modules:** Full ABCI 2.0 applications running as nested chains within transactions
3. **Shell-Like Executables:** Simple string-based I/O programs for utilities and scripting

Components are loaded from the merkle tree filesystem hierarchy:
- `/sbin/`: Core system components (ante-handler, begin-blocker, end-blocker)
- `/bin/`: Application modules
- `/lib/`: Shared libraries and utilities
- `/home/{module}/`: Module-specific persistent state
- `/tmp/`: Transaction-scoped temporary storage
- `/sys/`: System information and runtime state

**Important:** The WASI modules included in the repository (e.g., `crates/wasi-modules/`) are reference implementations, not statically linked components. They serve as default options for development and can be completely replaced by governance-uploaded alternatives stored in the merkle tree.

## Current State Summary

The Gridway SDK has validated core concepts but remains a proof-of-concept. While WASI execution works and individual components exist, they are not integrated into the revolutionary architecture described in this document. The implementation demonstrates feasibility but lacks the persistence, security, and upgradeability required for production use.

The implementation status of key components:

| Component | Status | Reality |
|-----------|---------|----------|
| WASI Microkernel | ⚠️ Partial | Executes WASI modules but not as designed - uses old module approach, not components |
| Virtual Filesystem | ⚠️ Disconnected | VFS exists but isn't connected to WASI - modules cannot use file operations |
| Capability Security | ❌ Wrong Model | In-memory only, uses traditional capabilities instead of file descriptors |
| JMT Storage | ❌ Not Connected | JMT exists but BaseApp uses MemStore - no state persistence |
| Dynamic Loading | ❌ Not Implemented | Components loaded from filesystem, not merkle storage |
| API Compatibility | ✅ Maintained | gRPC/REST endpoints match Cosmos SDK standards |

Critical architectural gaps that must be addressed:

| Gap | Priority | Impact |
|-----|----------|---------|
| State Persistence | 🔴 Critical | No JMT integration - entire system is in-memory only |
| Component Storage | 🔴 Critical | Components not stored in merkle tree as designed |
| VFS-WASI Bridge | 🔴 Critical | Modules cannot access state through file operations |
| Capability Model | 🔴 Critical | Wrong implementation - not file descriptor based |
| Resource Limits | 🟡 High | No CPU/memory limits could enable DoS attacks |
| Developer Tooling | 🟡 High | Limited CLI and SDK support |

The assessment places the project at **Technology Readiness Level 3-4**: Proof of concept with component validation. The core concepts are validated but the revolutionary architecture remains unimplemented. Specifically:

- The system cannot persist state across restarts
- Components cannot be loaded from or stored in the blockchain
- The elegant file-based state access model is not functional
- The capability system doesn't follow the ocap design

## Future Evolution Pathways

The WASI microkernel architecture is not just an endpoint but a foundation for significant future evolution, positioning the Gridway SDK to pioneer capabilities beyond those of traditional blockchain platforms.

### Evolving the Core Protocol

The architecture is designed for future enhancements to its core security and scalability:

- **Capability Delegation:** Allowing modules to temporarily and securely grant a subset of their rights to other modules
- **Temporal Capabilities:** Granting permissions that automatically expire after a certain time or number of uses
- **Hierarchical Permissions:** Creating complex, nested permission structures for advanced applications

Beyond security, the architecture provides a clear path toward a **distributed execution model**. The isolation of modules makes it feasible to explore execution parallelism, where different modules could run concurrently when they have no intersecting state modification.

### Deepening Interoperability

While already IBC-compatible, the modular design enables more advanced **cross-chain communication patterns**. Specialized bridge modules could be developed to create high-performance, trust-minimized connections to other ecosystems, and multi-chain applications could be orchestrated with greater ease than on monolithic platforms.

## Summary of Critical Architectural Decisions

These are the architectural decisions that define Gridway, though most remain unimplemented:

1. **WASI Component Model Architecture:** The foundational choice to implement blockchain logic as dynamically loaded, sandboxed WebAssembly components using WASI 0.2, enabling modularity and multi-language support. *(Partially implemented - uses old WASI module approach)*

2. **Dynamic Component Loading from Merkle Storage:** Components are stored in the merkle tree itself and loaded from well-known paths, enabling governance-controlled upgrades of even core system components without hard forks. *(Not implemented - loads from filesystem)*

3. **Unified GlobalAppStore:** A single merkle tree (using JMT or Merk) replaces the traditional MultiStore, with logical isolation achieved through VFS paths rather than separate stores. *(Not implemented - uses MemStore)*

4. **File Descriptor-Based Capabilities:** Security through unforgeable file descriptors as capability handles, where possession of a descriptor proves access rights. *(Not implemented - uses wrong capability model)*

5. **VFS-Mediated State Access:** All state access through POSIX file operations, making blockchain development intuitive and language-agnostic. *(Not implemented - VFS not connected to WASI)*

6. **CometBFT and ABCI 2.0:** Full integration with modern consensus features. *(Partially implemented)*

7. **Ecosystem API Compatibility:** Strict adherence to existing Cosmos SDK API contracts. *(Implemented)*