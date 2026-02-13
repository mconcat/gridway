# Gridway Gap Analysis: Path to Feature-Complete Testnet

**Last Updated:** 2025-02-10  
**Goal:** All capabilities from `main` branch working on Commonware Simplex — multi-node testnet with TX signing, token transfers, balance queries, and API access.

---

## Executive Summary

The `experiment/commonware-migration` branch has completed its core WASM microkernel pipeline and Commonware consensus integration. The full execution path — TX validation (ed25519 in WASM), bank module dispatch (WASM), block hooks (WASM), and deterministic state roots (Patricia Merkle Trie) — works end-to-end with 102 passing tests. HTTP API endpoints, key generation tooling, and E2E test scripts exist.

The remaining work falls into two categories:
1. **Integration gaps** — multi-node networking has not been tested end-to-end
2. **Regressions from `main`** — state persistence, key management, Docker setup, and TX building ergonomics need to be rebuilt on the new stack

---

## Current State: What Works

| Component | Status | Evidence |
|-----------|--------|----------|
| WASM validator (ed25519 verify + sequence) | ✅ | `test_full_wasm_tx_pipeline` |
| WASM bank module (MsgSend) | ✅ | Balance changes verified in tests |
| WASM hooks (pre/post execute) | ✅ | `test_execute_block_with_hooks_via_wasm` |
| VFS → MerkleStore pipeline | ✅ | `test_vfs_jmt_end_to_end`, `test_vfs_namespace_isolation` |
| Deterministic state roots | ✅ | `test_deterministic_hash` |
| commit() returns real root hash | ✅ | `test_commit` |
| Snapshot export/import | ✅ | `test_export_import_snapshot` |
| GridwayApp (Application traits) | ✅ | `test_gridway_app_creation` |
| Simplex Engine wiring | ✅ | Compiles, follows Alto pattern |
| Block replay from archive | ✅ | `replay_blocks()` implemented |
| Module governance | ✅ | Store code, install, upgrade |
| HTTP API (tx, balance, account, status) | ✅ | `gridway-node` binary, `start_http_server()` |
| Key generation & TX signing | ✅ | `gridway-keygen` binary |
| E2E test scripts | ✅ | `test-e2e-transfer.sh`, `test-state-sync.sh` |

---

## Gaps

### Gap 1: Multi-Node Integration Test 🔴 Critical
**Effort:** 3-5 days | **Blocks:** Testnet launch

**What exists:** The consensus engine is fully wired (broadcast buffer, marshal actor, simplex engine, authenticated P2P). `gridway-setup` generates multi-node configs (Ed25519 keys, BLS shares, peer lists). `gridway-node` has CLI with config parsing. HTTP API for TX submission and balance queries works.

**What's missing:** No one has run multiple `gridway-node` instances and verified they reach consensus. The entire E2E networking path — P2P peer discovery, block proposal broadcasting, notarization, finalization, state root agreement — is untested.

**Required:**
- Run 3+ `gridway-node` instances with `gridway-setup`-generated configs
- Verify P2P peer discovery and authenticated connections
- Verify block proposal, notarization, and finalization
- Verify state root agreement across nodes (same blocks → same roots)
- Fix protocol/serialization/config issues discovered during testing

**Key risk:** Commonware P2P authenticated channels may have configuration subtleties not covered by following Alto's pattern.

### Gap 2: Genesis State Initialization 🔴 Critical
**Effort:** 1-2 days | **Blocks:** Testnet launch

**What exists:** `gridway-setup` generates validator configs. BaseApp has `set_balance()` and `set_account()`. Init scripts exist but reference the old CometBFT setup.

**What's missing:** When `gridway-node` starts with an empty archive, it needs to initialize genesis state (accounts, balances) before the first block.

**Required:**
- Define genesis config format (accounts + balances, can be in setup YAML or separate JSON)
- `gridway-node` detects first start (empty archive) → loads genesis state into BaseApp → commits initial state root
- `gridway-setup` generates genesis with initial accounts for testing

### Gap 3: Docker & Deployment Update 🟡 High
**Effort:** 1-2 days | **Blocks:** Easy testnet deployment

**Regression from `main`:** `Dockerfile` builds `gridway-server` (deleted crate). `docker-compose.yml` references CometBFT. Init scripts call CometBFT Docker image. All of this is stale.

**Required:**
- Update `Dockerfile` to build `gridway-node` instead of `gridway-server`
- Update `docker-compose.yml` to run `gridway-node` instances (no separate CometBFT)
- Update `docker-compose.multi.yml` for multi-node Commonware testnet
- Update init scripts to use `gridway-setup` instead of CometBFT init

### Gap 4: State Persistence 🟡 High
**Effort:** 3-5 days | **Blocks:** Production readiness

**Regression from `main`:** `main` had JMTStore with RocksDB for persistent state. Current branch uses `memory-db` (in-memory). State is rebuilt via block replay from Commonware archive on restart.

**Current mitigation:** Block replay works. For short-lived testnets this is acceptable.

**Required for production:**
- Implement a persistent `HashDB` backend (RocksDB or sled) for `trie-db`
- Swap `MemoryDB` for the persistent backend in MerkleStore
- Keep block replay as fallback for state corruption recovery

**Can be deferred:** Block replay is functional. For a development testnet, in-memory + replay is sufficient.

### Gap 5: TX Building Ergonomics 🟡 High
**Effort:** 2-3 days | **Blocks:** User experience

**Regression from `main`:** `main` had `gridway-client` with a TX builder library and CLI commands (`gridway tx bank send ...`).

**What exists now:** `gridway-keygen` can generate keys and sign TX bodies. HTTP `POST /tx` accepts signed JSON. E2E test scripts build TXs manually.

**Required:**
- CLI wrapper: `gridway-node tx send --from <addr> --to <addr> --amount 100ugridway --key <key>`
- Or standalone tool: `gridway-tx send ...` that builds, signs, and POSTs to a node
- Can be a shell script wrapping `gridway-keygen` + `curl` as an interim solution

### Gap 6: Key Management 🟢 Low
**Effort:** 3-5 days | **Blocks:** Security hardening

**Regression from `main`:** `main` had `gridway-keyring` with OS keychain integration.

**Current state:** Keys stored as plaintext hex in YAML config files.

**Required for production:**
- Encrypted key storage (at minimum password-protected keyfile)
- Or re-integrate OS keyring support

**Can be deferred:** Plaintext keys are acceptable for development testnet.

### Gap 7: Component Storage in Merkle Trie 🟢 Low (future)
**Effort:** 5-7 days

**What exists:** WASM binaries loaded from filesystem (`modules/*.wasm`). ModuleGovernance stores code metadata.

**Required for vision:**
- Store WASM bytecode in Merkle trie at well-known paths
- Load components from trie state instead of filesystem
- Enable governance-driven module upgrades

**Can be deferred:** Filesystem loading is functional.

### Gap 8: Simulation Testing Framework 🟢 Low (future)
**Effort:** 3-5 days

**Regression from `main`:** `main` had `gridway-simapp` for property-based testing.

**Current state:** 102 unit/integration tests cover individual components. No fuzz testing or simulation framework.

**Can be deferred:** Unit tests provide adequate coverage for current development stage.

---

## Task Dependency Graph

```
Gap 2 (Genesis) ─────┐
                      ├── Gap 1 (Multi-Node) ──── Gap 5 (TX Ergonomics)
Gap 3 (Docker) ───────┘
                                                   
Gap 4 (Persistence) ── independent
Gap 6 (Key Mgmt) ──── independent
Gap 7 (Component Store) ── independent
Gap 8 (Simulation) ── independent
```

**Critical path:** Genesis + Docker Update → Multi-Node Test → TX Ergonomics

---

## Priority Matrix

### Must Have for Testnet Launch

| Gap | Description | Effort | Priority |
|-----|-------------|--------|----------|
| 1 | Multi-node integration test | 3-5d | 🔴 Critical |
| 2 | Genesis state initialization | 1-2d | 🔴 Critical |
| 3 | Docker & deployment update | 1-2d | 🟡 High |
| 5 | TX building ergonomics (at least a script) | 1d (script) | 🟡 High |

### Should Have for Usable Testnet

| Gap | Description | Effort | Priority |
|-----|-------------|--------|----------|
| 4 | State persistence (RocksDB) | 3-5d | 🟡 High |
| 5 | Full TX CLI tool | 2-3d | 🟡 High |

### Can Defer

| Gap | Description | Effort | Priority |
|-----|-------------|--------|----------|
| 6 | Key management (encrypted/keyring) | 3-5d | 🟢 Low |
| 7 | Component storage in Merkle trie | 5-7d | 🟢 Low |
| 8 | Simulation testing framework | 3-5d | 🟢 Low |

---

## Estimated Timeline

| Phase | Tasks | Estimated Time |
|-------|-------|---------------|
| Phase 1: Testnet Foundation | Gap 2 (Genesis) + Gap 3 (Docker) | 2-4 days |
| Phase 2: Multi-Node | Gap 1 (Integration Test) | 3-5 days |
| Phase 3: User Interaction | Gap 5 (TX CLI, at least script) | 1-2 days |
| **Total to working testnet** | | **6-11 days** |
| Phase 4: Durability | Gap 4 (Persistent Backend) | 3-5 days |
| Phase 5: Hardening | Gap 6 (Key Mgmt) | 3-5 days |

---

## Verification Plan

### Milestone 1: Two-Node Consensus
- Two `gridway-node` instances start from same genesis state
- P2P connection established via Commonware authenticated channels
- Blocks produced with matching state roots
- Height advances on both nodes
- Finalization certificates stored in archive

### Milestone 2: Token Transfer
- Genesis has accounts with ugridway balances
- Submit `SignedTx` with `MsgSend` via `POST /tx`
- Balance decreases for sender (verified via `GET /balance/{addr}/ugridway`)
- Balance increases for recipient
- Both nodes report same balances
- Sequence number incremented

### Milestone 3: Restart and Replay
- Execute several transfers across blocks
- Stop one node
- Restart it
- Block replay rebuilds state from archive
- Node catches up to network height
- Balances match other nodes

### Milestone 4: Full Parity with `main`
- TX signing and submission via CLI tool
- Docker-based multi-node deployment
- All capabilities from `main` functional on Commonware stack
- Persistent state (optional: block replay fallback acceptable)

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| P2P connection issues | Medium | High | Engine follows Alto pattern; test with 2 nodes first |
| State root divergence between nodes | Low | Critical | Trie is deterministic; add assertion logging |
| Block replay too slow for long chains | Medium | Medium | Implement persistent backend (Gap 4) |
| BLS threshold key distribution errors | Low | Medium | `gridway-setup` generates correct shares; verify with 3 nodes |
| Docker image build failures | Low | Low | Update Dockerfile incrementally, test locally |
