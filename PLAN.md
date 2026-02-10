# Gridway: A WASI Microkernel Blockchain

**Last Updated:** 2025-02-10

## Overview

Gridway is a blockchain built as a WASM microkernel. All application logic — transaction validation, bank transfers, block lifecycle hooks — runs inside sandboxed WebAssembly (WASI) components. The host (BaseApp) provides state access through a Virtual Filesystem (VFS) backed by a Patricia Merkle Trie, and orchestrates execution via the ComponentHost (wasmtime).

Consensus is provided by [Commonware Library](https://github.com/commonwarexyz/monorepo) v0.0.65 — specifically the Simplex BFT engine with BLS12-381 threshold signatures and Ed25519 validator identity.

## Architectural Vision

The architecture centers on four principles:

1. **WASI Microkernel:** All application logic runs as sandboxed WASM components. The host loads, executes, and manages these components. There is no Rust-native fallback for core logic — WASM modules must be present.

2. **State as a Virtual Filesystem (VFS):** A single `GlobalAppStore` backed by a Patricia Merkle Trie replaces the traditional MultiStore. State access is mediated through a VFS with namespace isolation (`/bank/`, `/auth/`, `/staking/`, `/gov/`). WASM modules interact with state through a `kvstore` WIT interface that routes through the VFS to the Merkle trie.

3. **Dynamic Component Loading:** WASM components are loaded from well-known paths and can be replaced at runtime. The `ModuleGovernance` system supports `MsgStoreCode`, `MsgInstallModule`, and `MsgUpgradeModule` for on-chain module management. Components are currently loaded from the filesystem; storing them in the Merkle trie is a future goal.

4. **Capability-Based Security:** Modules receive scoped access to state namespaces. The VFS enforces read/write capabilities per namespace. The `CapabilityManager` tracks permissions with delegation and revocation support.

## Execution Pipeline

The full block execution pipeline is WASM-native:

```
execute_block(height, timestamp, chain_id, txs)
  │
  ├── 1. hook.pre_execute(block_ctx)              → WASM hook component
  │
  ├── 2. For each TX:
  │   ├── a. validator.validate(tx_ctx, raw_bytes) → WASM validator component
  │   │      • JSON decode
  │   │      • Ed25519 signature verification
  │   │      • Sequence number check (via kvstore → VFS → auth namespace)
  │   │      • Message extraction
  │   │
  │   ├── b. For each message:
  │   │      module.handle(ctx, msg)               → WASM domain module (bank, etc.)
  │   │      • State reads/writes via kvstore WIT → VFS → MerkleStore
  │   │
  │   └── c. Increment sender sequence
  │
  ├── 3. hook.post_execute(block_ctx, stats)       → WASM hook component
  │
  └── 4. Compute state root from MerkleStore
```

## Consensus Layer: Commonware Simplex

Gridway uses the Commonware Library's Simplex BFT consensus engine:

- **`GridwayApp`** implements `Application`, `VerifyingApplication`, and `Reporter` traits
- **`propose()`** drains pending TXs → executes via BaseApp → returns `GridwayBlock` with state root
- **`verify()`** re-executes the proposed block's TXs → checks state root matches
- **`report()`** on finalization → commits state to MerkleStore

Supporting infrastructure:
- **BLS12-381 threshold signing** for consensus certificates (notarization, finalization)
- **Ed25519** for validator identity and P2P authentication
- **`commonware-p2p` (authenticated)** for networking
- **`commonware-broadcast` (buffered)** for message dissemination
- **`commonware-storage` (archive)** for persisting finalized blocks and certificates
- **Block replay** on node restart from the archive to rebuild BaseApp state

### Block Type

`GridwayBlock` implements all required Commonware traits:
- `commonware_consensus::Block` (parent digest reference)
- `Heightable` (block height)
- `Digestible` / `Committable` (SHA-256 content hash)
- `Write` / `Read` (codec serialization)

Fields: `parent` digest, `height`, `timestamp`, `state_root`, `transactions`.

### Node Binary

`gridway-node` is the validator binary. It wires together:
- Commonware tokio runtime
- Authenticated P2P networking
- Buffered broadcast engine
- Simplex consensus engine with random leader election
- GridwayApp (BaseApp + WASM microkernel)

Configuration is YAML-based (`NodeConfig`) with Ed25519 private key, BLS threshold share, P2P peers, and storage directory.

## Crate Architecture

```
gridway (root)
├── crates/
│   ├── gridway-consensus/     — Commonware integration (Application, Engine, node binary)
│   ├── gridway-baseapp/       — WASM microkernel host (ComponentHost, VFS, capabilities)
│   ├── gridway-store/         — Patricia Merkle Trie + GlobalAppStore + namespaced views
│   ├── gridway-types/         — GridwayBlock, transaction types, events
│   ├── gridway-crypto/        — Ed25519 signing/verification, SHA-256, address derivation
│   ├── gridway-errors/        — Error types
│   ├── gridway-log/           — Logging/tracing utilities
│   ├── gridway-math/          — Numeric types (Dec, Int)
│   ├── gridway-telemetry/     — Metrics instrumentation
│   └── wasi-modules/          — WASM component source code
│       ├── bank/              — Bank module (MsgSend, balance queries)
│       ├── validator/         — TX validation (decode, ed25519 verify, sequence check)
│       ├── hook/              — Block lifecycle hooks (pre/post execute)
│       ├── ante-handler/      — Legacy ante handler (superseded by validator)
│       ├── begin-blocker/     — Legacy begin blocker (superseded by hook)
│       ├── end-blocker/       — Legacy end blocker (superseded by hook)
│       ├── tx-decoder/        — Legacy TX decoder (superseded by validator)
│       └── test-minimal/      — Minimal test component
├── wit/                       — WIT interface definitions
│   ├── module.wit             — Domain module interface (handle, query)
│   ├── kvstore.wit            — KVStore resource interface (get, set, delete, range)
│   ├── validator.wit          — TX validation interface (validate → validated-tx)
│   └── hook.wit               — Block hook interface (pre-execute, post-execute)
└── modules/                   — Compiled .wasm binaries
    ├── bank_component.wasm
    ├── hook_component.wasm
    └── validator_component.wasm
```

### Dependency Graph

```
gridway-consensus
  ├── gridway-baseapp
  │   ├── gridway-store (MerkleStore, GlobalAppStore, KVStore trait)
  │   ├── gridway-types
  │   ├── gridway-crypto
  │   └── gridway-telemetry
  ├── gridway-types (GridwayBlock)
  ├── gridway-crypto
  └── commonware-* (consensus, p2p, broadcast, storage, runtime, ...)
```

## WIT Interfaces

WASM modules communicate with the host through four WIT interfaces:

- **`kvstore`** — Namespace-scoped key-value store backed by VFS → MerkleStore. Resource-based: `open-store(name) → store`, then `store.get/set/delete/has/range`.
- **`module`** — Domain module interface: `handle(context, message) → module-response`. Used by bank and future modules.
- **`validator`** — TX validation: `validate(tx-context, raw-tx) → validation-result`. Combines decoding, signature verification, and message extraction.
- **`hook`** — Block lifecycle: `pre-execute(block-context) → hook-result`, `post-execute(block-context, tx-count, total-gas) → hook-result`.

## State Store

- **MerkleStore**: Patricia Merkle Trie using Parity's `trie-db` with SHA-256. In-memory backend (`memory-db`). Provides deterministic state root hashes for consensus.
- **GlobalAppStore**: Wraps a single MerkleStore with namespace isolation. Each namespace (bank, auth, staking, gov) gets a prefixed view.
- **NamespacedStore**: Implements `KVStore` trait with automatic key prefixing.
- **VFS**: Mounts NamespacedStores, enforces capabilities, provides file-like operations and direct key access.

The pipeline: WASM module → kvstore WIT → ComponentHost → VFS → NamespacedStore → MerkleStore → trie-db.

State is in-memory (MemoryDB backend). For persistence across restarts, the consensus layer replays finalized blocks from the Commonware archive. A persistent trie backend (RocksDB/sled) is a future option.

## Current Implementation Status

**Working (102 tests passing):**
- Full WASM execution pipeline: validate_tx → WASM validator (ed25519 verify + sequence check), bank.MsgSend → WASM bank module, hooks → WASM hook module
- VFS → MerkleStore integration with namespace isolation
- Deterministic state root hashes from Patricia Merkle Trie
- `commit()` returns real Merkle root hash
- Account management (auth namespace) with sequence tracking
- State snapshot export/import for state sync
- Commonware Application/VerifyingApplication/Reporter trait implementations
- Block proposal, verification, and finalization flow
- Block replay from archive on restart
- Module governance (store code, install, upgrade)
- Capability-based access control with delegation

**Not yet working:**
- Multi-node testnet (networking not tested end-to-end)
- Persistent trie backend (in-memory only, relies on block replay)
- Component storage in Merkle trie (loaded from filesystem)
- File-descriptor-based capability model (uses path-based capabilities)
- IBC module
- Staking/governance modules as WASM components

## Module Governance

The `ModuleGovernance` system enables on-chain module management:
- `MsgStoreCode` — Store WASM bytecode with metadata (SHA-256 verified)
- `MsgInstallModule` — Deploy a stored code as a named module with config (message routes, capabilities, gas limits)
- `MsgUpgradeModule` — Replace a module's code reference while preserving its state

This is a built-in handler, not a WASM module itself. It bridges the gap toward fully governance-controlled component upgrades.

## Design Decisions

1. **WASM-only execution**: No Rust-native fallback for module logic. If a WASM module fails to load, the operation errors. This enforces the microkernel boundary.

2. **Commonware Simplex consensus**: Replaced CometBFT/ABCI with Commonware Library. Provides BFT consensus with BLS threshold signatures, integrated P2P, and archive-based persistence. Cosmos SDK compatibility is not a goal.

3. **Patricia Merkle Trie**: Uses `trie-db` (Parity) instead of JMT or IAVL. SHA-256 based, deterministic, with in-memory backend.

4. **Ed25519 + BLS12-381**: Ed25519 for validator identity and TX signatures. BLS12-381 threshold signatures for consensus certificates.

5. **JSON TX format**: Transactions are JSON-encoded `SignedTx` with ed25519 signatures over canonical body bytes. Simpler than Protobuf/Amino but sufficient for the current stage.

6. **In-memory state with block replay**: State is not persisted directly. On restart, finalized blocks are replayed from the Commonware archive to rebuild state. Snapshot export/import provides an alternative path.
