# Gridway Roadmap: Commonware Migration Completion

**Last Updated:** 2026-02-10
**Branch:** `experiment/commonware-migration`
**Goal:** Production-grade multi-node testnet with WASM microkernel, zero mockups/workarounds

---

## Status Key
- ✅ Done
- 🔄 In Progress
- ⬜ Not Started

---

## Phase 0: Foundation (DONE)
- ✅ JMT → MerkleStore (Patricia Merkle Trie) migration
- ✅ VFS → MerkleStore integration
- ✅ Full WASM pipeline (validator, bank, hooks)
- ✅ Deterministic state root from commit()
- ✅ Commonware Application/VerifyingApplication traits
- ✅ Genesis config & initialization
- ✅ Hardcoded CHAIN_ID removed → config
- ✅ Hardcoded test keypairs removed → genesis-driven
- 🔄 HTTP API (raw TCP → axum migration in progress)

---

## Phase 1: Code Quality & Hardening
**Priority:** Must complete before multi-node
**Estimated:** 3-5 days

### 1.1 Error Handling Cleanup ⬜
- Replace all `unwrap()`/`expect()` in production code with proper error handling
- Consensus crate has ~76 unwrap calls
- BaseApp, store, types crates — audit and fix
- Test code can keep unwrap()

### 1.2 Mempool Hardening ⬜
- Current: bare `VecDeque<Vec<u8>>` with no limits
- Add max pending TX count (configurable)
- Add max TX size limit
- Add duplicate TX detection (hash-based)
- Basic fee/priority ordering (optional, can be simple)

### 1.3 WASM Resource Limits ⬜
- Memory limits per WASM module execution
- CPU/fuel limits already exist (wasmtime fuel metering) — verify they're enforced
- Max WASM binary size for module governance

### 1.4 Lock Contention Review ⬜
- `Arc<Mutex<BaseApp>>` is used everywhere — review for potential deadlocks
- Consider RwLock where reads dominate (balance queries)
- Document locking strategy

---

## Phase 2: Functionality Restoration
**Priority:** Required for usable testnet
**Estimated:** 4-6 days

### 2.1 CLI TX Tool ⬜
- Binary or script to: generate keypair, build SignedTx JSON, sign with ed25519, POST to node
- `gridway-tx send --from <key> --to <addr> --amount 100ugridway --node http://localhost:PORT`
- `gridway-tx keygen --seed <n>` or `--random`
- `gridway-tx balance --address <addr> --node http://...`

### 2.2 State Persistence ⬜
- Replace `memory-db` with persistent HashDB backend (RocksDB or sled) for trie-db
- State survives node restart without block replay
- Keep block replay as fallback/recovery mechanism
- Benchmark: persistence overhead vs replay time

### 2.3 Stale Documentation Cleanup ⬜
- Audit all code comments for references to deleted crates (gridway-server, gridway-client, gridway-proto, etc.)
- Remove or update stale ABCI/CometBFT references in code comments
- Ensure CLAUDE.md build instructions match reality

---

## Phase 3: Integration & Multi-Node
**Priority:** The actual goal
**Estimated:** 5-8 days

### 3.1 Single-Node Integration Test ⬜
- Start gridway-node programmatically in test
- Submit TX via HTTP API
- Verify balance changes
- Verify state root changes
- Verify block height advances

### 3.2 Multi-Node Consensus Test ⬜
- 3-node local testnet via gridway-setup
- Verify P2P peer discovery and connection
- Verify block proposal → notarization → finalization
- Verify state root agreement across nodes
- Verify TX propagation and execution on all nodes

### 3.3 Node Restart & Replay Test ⬜
- Stop one node, restart it
- Verify block replay rebuilds correct state
- Verify node catches up to network height
- Verify balances match other nodes

### 3.4 Docker Deployment ⬜
- Update docker-compose for Commonware nodes
- Dockerfile for gridway-node
- Setup script for N-node testnet in Docker

---

## Phase 4: Vision Completion (Post-MVP)
**Priority:** Architecture goals, not blocking testnet

### 4.1 Component Storage in Merkle Trie ⬜
- Store WASM bytecode in trie at well-known paths
- Load components from trie instead of filesystem
- Governance-driven module upgrades

### 4.2 File-Descriptor-Based Capabilities ⬜
- Replace path-based capabilities with unforgeable FD handles
- OCAP model as originally designed

### 4.3 Additional WASM Modules ⬜
- Staking module (WASM)
- Governance voting module (WASM)
- IBC module (WASM)

---

## Dependency Graph

```
Phase 1 (Quality) ──→ Phase 3.1 (Single-Node Test) ──→ Phase 3.2 (Multi-Node)
                  ──→ Phase 2.1 (CLI TX Tool)       ──→ Phase 3.2 (Multi-Node)
                  ──→ Phase 2.2 (State Persistence)  ──→ Phase 3.3 (Restart Test)
                                                      ──→ Phase 3.4 (Docker)
```

Phase 1 blocks everything. Phase 2.1 (CLI) blocks multi-node testing (need to submit TXs).
Phase 2.2 (persistence) blocks restart testing but not initial multi-node.

**Critical path:** Phase 1 → Phase 2.1 → Phase 3.1 → Phase 3.2
