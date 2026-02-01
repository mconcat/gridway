# Gridway Gap Analysis: Path to Two-Node Token Transfer

**Date:** 2025-07-12
**Goal:** Two nodes running blockchain network with real token transfers (bank module)

---

## Executive Summary

The Gridway codebase is at ~35K lines of Rust across 15 crates. Individual components exist in reasonable shape—JMT store with RocksDB, VFS, WASI component host, ABCI server, bank service, genesis types—but they are **not connected to each other**. The BaseApp, which should orchestrate everything, uses MemStore instead of JMT, returns placeholder app hashes, and doesn't route transactions to any actual module execution.

**Bottom line:** The pieces exist but the wiring is missing. Approximately 5-7 key integration tasks stand between the current state and a working two-node chain with token transfers.

---

## Current State: What Actually Works

### ✅ Working Components

| Component | Location | Status |
|-----------|----------|--------|
| JMTStore + RocksDB | `gridway-store/src/jmt.rs` | Persistent KV store with commit/version/root hash |
| GlobalAppStore | `gridway-store/src/global.rs` | Namespaced views over single JMT |
| StateManager | `gridway-store/src/state.rs` | Coordinates GlobalAppStore |
| ABCI gRPC Server | `gridway-server/src/abci_server.rs` | All ABCI 2.0 methods via tonic |
| BankService | `gridway-server/src/services/bank.rs` | get_balance, set_balance, transfer |
| Genesis Types | `gridway-types/src/genesis.rs` | AppGenesis, BankGenesis, validation |
| MsgSend Types | `gridway-types/src/msgs/bank.rs` | Protobuf encode/decode, validation |
| WASI Component Host | `gridway-baseapp/src/component_host.rs` | Loads and executes WASM components |
| VFS | `gridway-baseapp/src/vfs.rs` | File operations, mount points, capability checks |
| Server Binary | `gridway-server/src/bin/gridway-server.rs` | CLI with init/start/version |
| Docker Compose | `docker-compose.multi.yml` | 4-node setup with CometBFT 0.38 |
| Proto Definitions | `gridway-proto/` | CometBFT ABCI v1 types |

### ❌ What's Broken / Not Connected

| Issue | Location | Impact |
|-------|----------|--------|
| BaseApp uses MemStore | `baseapp/src/lib.rs:180` | No persistent state |
| commit() returns `[0u8; 32]` | `baseapp/src/lib.rs:773` | Nodes can't reach consensus |
| deliver_tx doesn't execute messages | `baseapp/src/lib.rs:713-730` | Only simulates gas, no state changes |
| init_chain is a no-op | `baseapp/src/lib.rs:812-816` | Genesis balances never loaded |
| get_balance/set_balance are no-ops | `baseapp/src/lib.rs:824-829` | Bank operations don't work |
| ABCI TCP handler is placeholder | `abci_server.rs:953-960` | Raw TCP connections not processed |
| No tx signing/broadcast path | Multiple | Can't submit real transactions |

---

## Critical Path: Tasks in Dependency Order

### Phase 1: Core State Pipeline (Must have — blocks everything else)

#### Task 1: Connect BaseApp to JMT/GlobalAppStore
**Priority:** 🔴 CRITICAL | **Effort:** Medium (2-3 days) | **Blocks:** Everything

**Current:** `BaseApp::new()` creates `MemStore::new()` and `VFS` mounts individual MemStores.

**Required:**
- Replace MemStore with JMTStore backed by RocksDB in BaseApp
- Initialize GlobalAppStore with JMT backend
- Pass `data_dir` path from config to JMTStore
- Mount namespaced views ("bank", "auth") into VFS via GlobalAppStore

**Key changes:**
```
baseapp/src/lib.rs:
  - BaseApp::new() → accept data_dir path
  - Create JMTStore::new("state", data_dir.join("state.db"))
  - Create GlobalAppStore::new(jmt_store)
  - Register namespaces: bank, auth, staking, gov
  - Use NamespacedStore views instead of individual MemStores
```

#### Task 2: Implement Proper commit()
**Priority:** 🔴 CRITICAL | **Effort:** Small (1 day) | **Blocks:** Consensus between nodes

**Current:** Returns `vec![0u8; 32]` placeholder.

**Required:**
- Call `jmt_store.commit()` to flush pending changes
- Return the actual JMT root hash
- Track block height and app hash for restart recovery
- Persist `(height, app_hash)` mapping

**Note:** JMTStore.commit() already returns `Hash` ([u8; 32]) — just needs to be called and returned.

#### Task 3: Implement Bank Module Message Execution
**Priority:** 🔴 CRITICAL | **Effort:** Medium (2-3 days) | **Blocks:** Token transfers

**Current:** `deliver_tx()` and `execute_transaction()` decode messages but only simulates gas for bank messages. The `BankService` in the server crate has real transfer logic but it's disconnected from tx execution.

**Required:**
- In `execute_transaction()`, when type_url is `/cosmos.bank.v1beta1.MsgSend`:
  - Parse MsgSend from message value
  - Check sender balance from the bank namespace in GlobalAppStore
  - Debit sender, credit recipient
  - Emit transfer events
- Can reuse logic from `BankService::transfer()` but operating on the BaseApp's store

**Design choice:** Implement bank logic directly in BaseApp for now (native module), not via WASM. WASM bank module is a Phase 3 goal.

#### Task 4: Implement Genesis Initialization
**Priority:** 🔴 CRITICAL | **Effort:** Small-Medium (1-2 days) | **Blocks:** Initial state

**Current:** `init_chain()` is empty (`Ok(())`).

**Required:**
- Parse genesis JSON (AppGenesis structure already exists)
- Load account balances into bank store namespace
- Load account info into auth store namespace
- Set initial block height
- Return initial app hash from first commit
- Generate and save genesis file during `gridway init`

**The genesis types are complete** — just need to write the values into the store.

### Phase 2: ABCI Integration (Required for multi-node)

#### Task 5: Fix ABCI Server Connection
**Priority:** 🟡 HIGH | **Effort:** Medium (2-3 days) | **Blocks:** CometBFT communication

**Current:** Two ABCI server modes exist:
1. `AbciServer::start()` — tonic gRPC server (works for gRPC ABCI)
2. `AbciServer::start_abci_server()` — raw TCP accept with placeholder handler

**CometBFT 0.38 supports gRPC ABCI** via `--proxy_app=grpc://host:port`. The tonic implementation should work.

**Required:**
- Verify that `AbciServer::start()` (tonic gRPC) actually works with CometBFT
- Update `start_abci_server()` to use tonic gRPC instead of raw TCP
- OR update the docker-compose to use `grpc://` protocol
- Ensure FinalizeBlock correctly calls begin_block, execute each tx, end_block, and returns proper app_hash

**Key verification:** Test with CometBFT locally: `cometbft node --proxy_app=grpc://127.0.0.1:26658`

#### Task 6: Testnet Setup Script
**Priority:** 🟡 HIGH | **Effort:** Medium (2-3 days) | **Blocks:** Running two nodes

**Required:**
- Script to generate genesis with validators for N nodes
- Generate CometBFT config (persistent_peers, seeds)
- Generate validator keys (ed25519)
- Set up genesis with initial balances
- Update docker-compose.multi.yml accordingly

**Partially exists:** `setup.sh` and `docker-compose.multi.yml` exist but need updating.

### Phase 3: Transaction Submission (Required for token transfers)

#### Task 7: Transaction Building & Broadcasting
**Priority:** 🟡 HIGH | **Effort:** Medium (2-3 days) | **Blocks:** User-initiated transfers

**Required:**
- CLI command: `gridway tx bank send <from> <to> <amount> --chain-id <id>`
- Build Tx with MsgSend, proper auth_info, fee
- Sign transaction with private key
- Broadcast via CometBFT RPC (`broadcast_tx_commit` or `broadcast_tx_sync`)
- Wait for inclusion and return result

**Existing:** `gridway-client/src/tx_builder.rs` has scaffolding. `gridway-types/src/tx.rs` has Tx types.

**Simplification:** For MVP, can bypass signing and just accept unsigned JSON transactions through a faucet-like endpoint.

---

## Task Dependency Graph

```
Task 1 (JMT Integration)
  ├── Task 2 (commit)
  ├── Task 3 (Bank Execution)
  │     └── Task 7 (TX Building)
  └── Task 4 (Genesis Init)
        └── Task 6 (Testnet Setup)
              └── Task 5 (ABCI Fix)
```

**Critical path:** 1 → 2 → 3 → 4 → 5 → 6 → 7

---

## Detailed Code Changes Required

### Task 1: JMT Integration — Specific Changes

**File: `crates/gridway-baseapp/src/lib.rs`**

```rust
// BEFORE (line ~178):
let store = Arc::new(std::sync::Mutex::new(MemStore::new()));

// AFTER:
pub fn new(name: String, data_dir: Option<PathBuf>) -> Result<Self> {
    let data_dir = data_dir.unwrap_or_else(|| PathBuf::from(".gridway/data"));
    std::fs::create_dir_all(&data_dir).map_err(|e| ...)?;
    
    let jmt_store = JMTStore::new("state".to_string(), data_dir.join("state.db"))
        .map_err(|e| ...)?;
    let global_store = GlobalAppStore::new(jmt_store);
    global_store.register_namespace("bank", false)?;
    global_store.register_namespace("auth", false)?;
    // ...
}
```

**File: `crates/gridway-baseapp/Cargo.toml`** — ensure `gridway-store` dependency includes JMT features.

### Task 2: commit() — Specific Changes

**File: `crates/gridway-baseapp/src/lib.rs`**

```rust
// BEFORE (line ~773):
pub fn commit(&mut self) -> Result<Vec<u8>> {
    Ok(vec![0u8; 32])
}

// AFTER:
pub fn commit(&mut self) -> Result<Vec<u8>> {
    let root_hash = self.global_store.get_store().lock()
        .map_err(|e| BaseAppError::Store(e.to_string()))?
        .commit()
        .map_err(|e| BaseAppError::Store(e.to_string()))?;
    Ok(root_hash.to_vec())
}
```

### Task 3: Bank Execution — Specific Changes

**File: `crates/gridway-baseapp/src/lib.rs` in `execute_transaction()`**

The wildcard match arm (line ~760) currently returns "unhandled message type". Need to add:

```rust
"/cosmos.bank.v1beta1.MsgSend" => {
    // Parse MsgSend
    let msg: MsgSend = parse_msg_send(msg_value)?;
    msg.validate_basic().map_err(|e| BaseAppError::InvalidTx(e.to_string()))?;
    
    // Execute transfer in bank namespace
    let bank_store = self.global_store.get_namespace("bank")?;
    execute_bank_send(&mut bank_store, &msg)?;
    
    events.push(transfer_event(&msg));
    total_gas_used += 65000;
}
```

### Task 4: Genesis Init — Specific Changes

**File: `crates/gridway-baseapp/src/lib.rs` in `init_chain()`**

```rust
pub fn init_chain(&mut self, chain_id: String, genesis_bytes: &[u8]) -> Result<()> {
    let genesis: AppGenesis = serde_json::from_slice(genesis_bytes)
        .map_err(|e| BaseAppError::InitChainFailed(e.to_string()))?;
    
    // Load bank balances
    if let Some(bank_genesis) = &genesis.app_state.bank {
        let mut bank_store = self.global_store.get_namespace("bank")?;
        for balance in &bank_genesis.balances {
            for coin in &balance.coins {
                let key = format!("balance_{}_{}", balance.address, coin.denom);
                bank_store.set(key.as_bytes(), coin.amount.as_bytes())?;
            }
        }
    }
    
    self.chain_id = chain_id;
    Ok(())
}
```

---

## What Can Be Deferred (Not needed for MVP)

| Feature | Why It Can Wait |
|---------|----------------|
| WASM VFS integration | Native bank module works for MVP |
| File descriptor capabilities | Security refinement, not functional requirement |
| Dynamic component loading from merkle tree | Use filesystem loading for now |
| Signature verification | Can be added after basic flow works |
| gRPC query services | REST or direct state queries sufficient |
| State sync / snapshots | Not needed for 2-node testnet |
| Vote extensions | CometBFT works without them |
| Governance module | Not needed for transfers |
| Module governance (store/install/upgrade) | Not needed for transfers |

---

## Estimated Timeline

| Phase | Tasks | Estimated Time |
|-------|-------|---------------|
| Phase 1 | Tasks 1-4 (State pipeline) | 5-8 days |
| Phase 2 | Tasks 5-6 (ABCI + Testnet) | 3-5 days |
| Phase 3 | Task 7 (TX submission) | 2-3 days |
| **Total** | | **10-16 days** |

**Parallel opportunities:** Tasks 5 (ABCI) and 7 (TX building) can be worked in parallel with Phase 1, once the store interface is defined.

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| CometBFT ABCI protocol mismatch | Medium | High | Test early with real CometBFT instance |
| JMT root hash non-determinism | Low | Critical | JMT is well-tested; use canonical key ordering |
| Transaction encoding incompatibility | Medium | Medium | Test with Cosmos SDK-compatible encodings |
| Docker networking issues | Low | Low | docker-compose already configured |
| WASI module loading failures | Low | Low | Can bypass WASI modules entirely for MVP |

---

## Verification Plan

### Milestone 1: Single Node State Persistence
- `gridway init --chain-id test` creates genesis
- `gridway start` runs ABCI server
- CometBFT connects and produces blocks
- App hash changes when state changes
- Restart preserves state

### Milestone 2: Two-Node Consensus
- Both nodes start from same genesis
- CometBFT peers connect
- Blocks produced with matching app hashes
- Height advances on both nodes

### Milestone 3: Token Transfer
- Genesis has accounts with balances
- Submit MsgSend transaction
- Balance decreases for sender
- Balance increases for recipient
- Query balances via API

---

## Files to Modify (Summary)

| File | Changes Needed |
|------|---------------|
| `crates/gridway-baseapp/src/lib.rs` | JMT integration, commit(), bank execution, genesis init |
| `crates/gridway-baseapp/Cargo.toml` | May need new deps |
| `crates/gridway-server/src/bin/gridway-server.rs` | Pass data_dir to BaseApp |
| `crates/gridway-server/src/abci_server.rs` | Verify gRPC ABCI works with CometBFT |
| `crates/gridway-server/src/integrated_server.rs` | Update to use GlobalAppStore |
| `scripts/setup-testnet.sh` | New or updated testnet setup script |
| `docker-compose.multi.yml` | Potentially update ABCI protocol |

---

## Conclusion

The project has solid foundations with well-designed individual components. The main gap is integration — connecting the JMT store to BaseApp, implementing actual transaction execution, and wiring up genesis initialization. These are straightforward engineering tasks, not architectural redesigns.

The WASI microkernel vision (components in merkle tree, VFS-mediated state access, FD-based capabilities) is ambitious and valuable long-term, but should be deferred for the MVP. A native bank module executing within BaseApp is the pragmatic path to a working two-node chain.
