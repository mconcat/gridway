# Gridway Implementation Status Report

**Last Updated:** 2025-02-10  
**Branch:** `experiment/commonware-migration`  
**Test Status:** 102 tests passing (0 failures)  
**Codebase:** ~17.5K lines of Rust across 10 crates + 8 WASM module crates

---

## Migration Goal

The `experiment/commonware-migration` branch replaces the CometBFT/ABCI consensus layer with Commonware Simplex while preserving and rebuilding all user-facing capabilities that existed on `main`. The end state must support everything `main` could do — transaction signing, testnet execution, balance management, API queries — on top of Commonware consensus.

---

## Component Status

### ✅ Fully Working

#### WASM Execution Pipeline
- **ComponentHost** (wasmtime 34.0): Loads and executes WASM components with fuel metering
- **Validator component**: JSON decode → ed25519 signature verification → sequence check → message extraction. All in WASM, using `ed25519-dalek` compiled to wasm32-wasip1.
- **Bank module component**: `MsgSend` handling — balance check, debit sender, credit recipient, event emission. All state access via kvstore WIT → VFS → MerkleStore.
- **Hook component**: `pre_execute` and `post_execute` block lifecycle hooks.
- **Full pipeline test**: `test_full_wasm_tx_pipeline` — creates keypair, signs TX, validates via WASM validator, dispatches to WASM bank, verifies balance changes and sequence increment.

#### VFS → MerkleStore Integration
- VFS mounts `NamespacedStore` views into namespace paths (`/bank/`, `/auth/`, etc.)
- WASM modules access state through `kvstore` WIT interface → VFS → NamespacedStore → MerkleStore
- Namespace isolation verified: writing to `bank` namespace does not affect `auth` namespace
- Direct key access helpers (`write_key`, `read_key`, `range_keys`) available for host-side operations

#### State Store
- **MerkleStore**: Patricia Merkle Trie using `trie-db` with SHA-256 hasher. Deterministic state roots.
- **GlobalAppStore**: Single MerkleStore with namespace prefixing (bank:, auth:, staking:, gov:)
- **`commit()`**: Returns real Merkle root hash (not placeholder)
- **Deterministic**: Same writes produce same root hash across runs
- **Snapshot export/import**: `to_snapshot()` / `from_snapshot()` for state sync

#### Account Management
- Account model stored in `auth` namespace as JSON
- Sequence tracking with automatic increment after TX execution
- Public key → address derivation via SHA-256 truncation

#### Commonware Consensus Integration
- **`GridwayApp`**: Implements `Application`, `VerifyingApplication`, `Reporter` traits
- **`propose()`**: Drains pending TX pool → executes via BaseApp → returns GridwayBlock
- **`verify()`**: Re-executes block TXs → checks state root matches proposal
- **`report()`**: Commits state on finalization
- **Block replay**: Rebuilds BaseApp state from archived blocks on restart
- **`GridwayBlock`**: Implements `Block`, `Heightable`, `Digestible`, `Committable`, `Write`/`Read`
- **Engine**: Full Simplex engine wiring — broadcast buffer, marshal actor, consensus engine

#### HTTP API (in `gridway-node` binary)
- `POST /tx` — Submit signed transaction JSON to pending pool
- `GET /balance/{address}/{denom}` — Query account balance
- `GET /account/{address}` — Query account info (public key, sequence)
- `GET /status` — Node status and current state root
- `GET /snapshot` — Full state snapshot (JSON)
- `GET /health` — Health check

#### CLI Tooling
- `gridway-node` — Validator node binary (consensus + HTTP API)
- `gridway-setup` — Generate multi-node testnet configs (keys, peers, BLS shares)
- `gridway-keygen` — Generate ed25519 keypairs, sign transactions, derive addresses

#### E2E Test Scripts
- `scripts/test-e2e-transfer.sh` — Signed token transfer on multi-node testnet
- `scripts/test-state-sync.sh` — Block replay on restart + state snapshot verification

#### Module Governance
- `MsgStoreCode`: Store WASM bytecode with SHA-256 verification
- `MsgInstallModule`: Deploy stored code as named module
- `MsgUpgradeModule`: Replace module code while preserving state

#### Capability System
- Path-based read/write capabilities per namespace
- Capability delegation and revocation
- Access control enforced by VFS

### ⚠️ Implemented but Not Production-Ready

#### In-Memory State Backend
- MerkleStore uses `memory-db` (in-memory MemoryDB from Parity)
- State is lost on process restart
- Mitigation: block replay from Commonware archive rebuilds state
- For production: needs persistent `HashDB` backend (RocksDB/sled)

#### Multi-Node Consensus
- Engine wiring is complete (broadcast, marshal, simplex, P2P)
- Setup tooling generates correct configs
- **Not tested end-to-end** with multiple nodes on a network

### ❌ Not Yet Implemented

| Feature | Notes |
|---------|-------|
| Component storage in Merkle trie | WASM binaries loaded from filesystem, not from chain state |
| File-descriptor-based capabilities | Uses path-based model, not unforgeable FD handles |
| Persistent trie backend | In-memory only; relies on block replay for durability |
| IBC module | Not started |
| Staking module | Not started (WASM) |
| Governance voting module | Not started (WASM) |
| State sync P2P protocol | Snapshot export/import exists but no P2P sync protocol |

---

## Regression Tracking: `main` → `experiment/commonware-migration`

The `main` branch had a CometBFT/ABCI-based architecture with ~15 crates. The migration removed five crates and replaced the consensus/networking layer. This section tracks what was lost and what has been rebuilt.

### Removed Crates from `main`

| Crate | What It Provided | Current Status |
|-------|-----------------|----------------|
| `gridway-server` | gRPC (tonic) ABCI server, REST gateway, BankService, health endpoints | **Replaced.** `gridway-node` has HTTP API with equivalent endpoints (POST /tx, GET /balance, GET /status, GET /health). gRPC is gone — Commonware uses native P2P, not ABCI gRPC. |
| `gridway-client` | TX builder, broadcast via CometBFT RPC (`broadcast_tx_commit`), CLI interface | **Partially replaced.** `gridway-keygen` handles key generation and TX signing. TX submission via HTTP `POST /tx`. No full CLI wrapper yet (e.g., `gridway tx bank send ...`). |
| `gridway-proto` | Protobuf definitions (CometBFT ABCI v1 types, Cosmos bank/auth protos) | **Removed, not needed.** Gridway uses JSON TX format and WIT interfaces instead of Protobuf. Commonware has its own codec (`commonware-codec`). |
| `gridway-keyring` | OS keychain integration for key storage | **Not replaced.** Keys are stored in YAML config files (plaintext hex). Secure key storage is deferred. |
| `gridway-simapp` | Simulation/property-based testing framework | **Not replaced.** Unit tests cover individual components. No fuzz/property-based testing framework. |

### Feature-by-Feature Comparison

| Feature | `main` Branch | `experiment/commonware-migration` | Gap |
|---------|--------------|-----------------------------------|-----|
| **Consensus** | CometBFT 0.38 (external process) | Commonware Simplex (integrated) | ✅ Upgraded |
| **Block execution** | ABCI `FinalizeBlock` → BaseApp | `GridwayApp.propose/verify/report` → BaseApp | ✅ Upgraded |
| **TX validation** | Rust-native ante handler | WASM validator (ed25519 in WASM) | ✅ Upgraded |
| **Bank module** | BankService in gridway-server (Rust-native) | WASM bank component | ✅ Upgraded |
| **State store** | JMTStore + RocksDB (persistent) | Patricia Merkle Trie + MemoryDB (in-memory) | ⚠️ Regression: no persistence |
| **State persistence** | RocksDB on disk | Block replay from archive | ⚠️ Functional but slower |
| **gRPC API** | tonic gRPC server (Cosmos-compatible endpoints) | HTTP API (custom endpoints) | ⚠️ Different protocol, equivalent function |
| **REST API** | REST gateway over gRPC | HTTP API directly | ⚠️ Different format, equivalent function |
| **TX signing** | gridway-client TX builder + keyring | gridway-keygen + manual JSON | ⚠️ Less ergonomic |
| **TX broadcast** | `broadcast_tx_commit` via CometBFT RPC | `POST /tx` via HTTP | ✅ Equivalent |
| **Balance query** | gRPC BankService.GetBalance | `GET /balance/{addr}/{denom}` | ✅ Equivalent |
| **Key management** | OS keyring (gridway-keyring) | Plaintext hex in YAML config | ❌ Regression |
| **Protobuf compat** | Full Cosmos proto definitions | JSON-only TX format | ❌ Removed by design |
| **Docker testnet** | docker-compose with CometBFT | Stale (references gridway-server) | ❌ Needs update |
| **Multi-node testnet** | Docker 4-node with CometBFT | gridway-setup generates configs, not yet tested | ⚠️ In progress |
| **E2E test scripts** | Not present | `test-e2e-transfer.sh`, `test-state-sync.sh` | ✅ New |
| **Simulation testing** | gridway-simapp framework | Unit tests only | ❌ Regression |
| **Cosmos SDK compat** | gRPC + Proto + REST matching Cosmos SDK | Not a goal | ❌ Removed by design |

### Summary of Regressions Requiring Action

1. **State persistence** — `main` had RocksDB via JMTStore. Current branch uses in-memory trie with block replay. Functional but not production-grade.
2. **Key management** — `main` had OS keyring integration. Current branch stores keys as plaintext hex in YAML files.
3. **TX building ergonomics** — `main` had a TX builder library. Current branch requires manual JSON construction + `gridway-keygen` for signing.
4. **Docker/testnet setup** — Docker files (`Dockerfile`, `docker-compose.yml`) still reference the deleted `gridway-server` binary. Need updating for `gridway-node`.
5. **Simulation testing** — `main` had a property-based testing framework (`gridway-simapp`). Not replaced.

### Regressions Intentionally Not Restored

- **Protobuf compatibility** — Gridway uses JSON TX format and WIT interfaces. Cosmos proto compatibility is not a goal.
- **gRPC API** — Replaced by HTTP API. Cosmos-compatible gRPC endpoints are not a goal.
- **CometBFT dependency** — Replaced by integrated Commonware Simplex. No external consensus process.

---

## Crate Inventory

| Crate | Lines | Description | Status |
|-------|-------|-------------|--------|
| `gridway-baseapp` | ~5,800 | WASM microkernel host, VFS, ComponentHost, capabilities | ✅ Working |
| `gridway-consensus` | ~1,800 | Commonware integration, Application traits, Engine, node binary | ✅ Working |
| `gridway-store` | ~900 | MerkleStore (trie-db), GlobalAppStore, NamespacedStore | ✅ Working |
| `gridway-types` | ~400 | GridwayBlock, SignedTx, TxResponse, Event | ✅ Working |
| `gridway-crypto` | ~200 | Ed25519 signing, SHA-256, address derivation | ✅ Working |
| `gridway-errors` | ~60 | Error types | ✅ Working |
| `gridway-log` | ~50 | Tracing macros | ✅ Working |
| `gridway-math` | ~200 | Dec, Int numeric types | ✅ Working |
| `gridway-telemetry` | ~50 | Metrics instrumentation | ✅ Working |
| `wasi-modules/bank` | ~230 | WASM bank module (MsgSend) | ✅ Working |
| `wasi-modules/validator` | ~230 | WASM TX validator (ed25519, sequence) | ✅ Working |
| `wasi-modules/hook` | ~270 | WASM block hooks | ✅ Working |

---

## Technology Readiness Assessment

**TRL 5: Component validation in relevant environment**

- Core WASM microkernel pipeline is validated with real execution (ed25519 crypto, state mutations, event emission)
- Consensus integration is code-complete but not network-tested
- Individual components are well-tested (102 tests, 0 failures)
- The system can execute blocks, validate TXs, process bank sends, and produce deterministic state roots
- HTTP API, key generation, and E2E test scripts exist
- Missing: multi-node integration testing, persistent storage, Docker updates

---

## Test Coverage

| Area | Tests | Notes |
|------|-------|-------|
| BaseApp (full pipeline) | 14 | Including `test_full_wasm_tx_pipeline` end-to-end |
| VFS | 10 | File ops, namespace isolation, capabilities, range queries |
| ComponentHost | 6 | KVStore binding, VFS integration |
| Capabilities | 6 | Delegation, implication, access control |
| ModuleRouter | 6 | Registration, dependency resolution, IPC |
| ModuleGovernance | 3 | Store code, validation, authorization |
| WasiHost | 3 | Module lifecycle, state management |
| GridwayApp | 2 | Creation, pending TX management |
| Store | 3 | MemStore, CacheStore, prefix iterator |
| MerkleStore | 15 | Trie operations, commit, snapshot, determinism |
| GlobalAppStore | 8 | Namespace isolation, concurrent access |
| Types (Block) | 4 | Genesis, roundtrip, digest determinism |
| Crypto | 3 | SHA-256, signing, address derivation |
| Other | ~19 | Errors, telemetry, math, etc. |
| **Total** | **102** | |
