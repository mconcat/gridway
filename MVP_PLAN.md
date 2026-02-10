# Gridway MVP Plan: Multi-Node WASM Microkernel Blockchain

**Last Updated:** 2025-02-10  
**Goal:** Multi-node testnet with WASM-powered token transfers over Commonware Simplex consensus  
**Branch:** `experiment/commonware-migration`

---

## Executive Summary

The core WASM microkernel pipeline is complete. All block execution — TX validation (ed25519 in WASM), bank module (WASM), hooks (WASM) — works end-to-end with deterministic state roots from a Patricia Merkle Trie. 102 tests pass.

The remaining work is integration: genesis setup, multi-node networking, TX submission, and balance queries.

---

## What's Already Done

### Phase 0: State Foundation ✅ Complete

- [x] BaseApp uses MerkleStore (Patricia Merkle Trie via `trie-db`)
- [x] GlobalAppStore with namespace isolation (bank, auth, staking, gov)
- [x] VFS mounts NamespacedStore views
- [x] `commit()` returns real Merkle root hash
- [x] WASM modules access state via kvstore WIT → VFS → MerkleStore

### Phase 0.5: Full WASM Pipeline ✅ Complete

- [x] WASM validator: JSON decode, ed25519 verify, sequence check, message extraction
- [x] WASM bank module: MsgSend (balance check, debit, credit, events)
- [x] WASM hooks: pre_execute and post_execute
- [x] Account model with sequence tracking
- [x] Snapshot export/import

### Commonware Consensus Integration ✅ Complete (code-level)

- [x] `GridwayApp` implements Application, VerifyingApplication, Reporter
- [x] `GridwayBlock` implements Block, Heightable, Digestible, Committable, Write/Read
- [x] Simplex Engine with broadcast buffer, marshal actor, consensus engine
- [x] Block replay from archive on restart
- [x] `gridway-node` binary with CLI, YAML config
- [x] `gridway-setup` for key generation and config

---

## Remaining Phases

### Phase 1: Testnet Foundation (4-7 days)

#### Task 1.1: Genesis State Initialization (1-2 days)

**Current:** `gridway-setup` generates validator configs but no genesis state. BaseApp has `set_balance()` and `set_account()` but they aren't called at startup.

**Required:**
- Add genesis config to `gridway-setup` output: initial accounts with balances
- On first start (empty archive), `gridway-node` loads genesis state into BaseApp
- Genesis state: 2-3 accounts with `ugridway` balances for testing
- Store genesis hash as the initial state root

**Implementation:**
```rust
// In gridway-node startup, before engine creation:
if archive_is_empty {
    let mut app = baseapp.lock().unwrap();
    for (addr, balance) in genesis_accounts {
        app.set_balance(&addr, "ugridway", balance).unwrap();
        app.set_account(&addr, &Account { public_key: pk, sequence: 0 }).unwrap();
    }
    app.commit().unwrap();
}
```

#### Task 1.2: Multi-Node Integration Test (3-5 days)

**Current:** Engine wiring compiles and follows Alto's pattern. No end-to-end test.

**Required:**
- Run 3 `gridway-node` instances with generated configs
- Verify P2P peer discovery
- Verify block proposal → notarization → finalization
- Verify state root agreement across nodes
- Fix protocol/config issues as discovered

**Approach:**
1. Use `gridway-setup` to generate 3-node config
2. Start nodes locally on different ports
3. Monitor logs for block finalization
4. Verify height advances on all nodes

### Phase 2: User Interaction (3-5 days)

#### Task 2.1: TX Submission HTTP Endpoint (2-3 days)

**Current:** `GridwayApp::submit_tx()` exists. Node binary has `tx_port` config.

**Required:**
- HTTP POST endpoint: `POST /tx` accepting JSON `SignedTx`
- Validate basic structure, add to pending pool
- Return TX hash and acceptance status
- Script or simple CLI to build and sign TXs

**Signing uses existing infra:**
```bash
# Generate keypair from seed
gridway-keygen --seed 42

# Build and sign TX (new tool or script)
gridway-tx send --from <addr> --to <addr> --amount 100ugridway --key <privkey_hex>
# → POST to http://localhost:PORT/tx
```

#### Task 2.2: Balance Query Endpoint (1-2 days)

**Current:** `BaseApp::get_balance()` works but no external interface.

**Required:**
- HTTP GET endpoint: `GET /balance?address={addr}&denom={denom}`
- Returns JSON `{"address": "...", "denom": "...", "amount": "..."}`
- Also: `GET /account?address={addr}` for account info

### Phase 3: Verification and Hardening (2-3 days)

#### Task 3.1: End-to-End Transfer Test

**Test scenario:**
```
1. Genesis: alice=1000000ugridway, bob=0ugridway
2. POST /tx with MsgSend(alice → bob, 100ugridway)
3. Wait for finalization
4. GET /balance?address=alice → 999900ugridway
5. GET /balance?address=bob → 100ugridway
6. Verify same balances on all nodes
```

#### Task 3.2: Restart and Replay Test

**Test scenario:**
```
1. Execute several transfers across blocks
2. Stop one node
3. Restart it
4. Verify block replay rebuilds correct state
5. Verify node catches up and participates in consensus
```

---

## What Can Be Deferred

| Feature | Why It Can Wait |
|---------|----------------|
| Persistent trie backend | Block replay from archive is functional for short-lived testnet |
| Component storage in Merkle trie | Filesystem loading works |
| FD-based capabilities | Path-based capabilities provide isolation |
| Staking/governance modules | Bank-only is sufficient for MVP |
| IBC | Cross-chain is post-MVP |
| State sync protocol | Snapshot import exists; P2P sync is post-MVP |
| Production monitoring | Logs are sufficient for testnet |
| Multi-denom support | Single denom (ugridway) for MVP |

---

## Architecture (Current)

```
                    ┌─────────────────────────────────┐
                    │        gridway-node              │
                    │  (Commonware tokio runtime)       │
                    └──────────┬──────────────────────┘
                               │
    ┌──────────────────────────┼──────────────────────────┐
    │                          │                          │
    ▼                          ▼                          ▼
┌────────┐           ┌──────────────┐          ┌──────────────┐
│  P2P   │           │   Simplex    │          │   HTTP API   │
│(authed)│           │  Consensus   │          │  (tx/query)  │
└────────┘           └──────┬───────┘          └──────┬───────┘
                            │                         │
                    ┌───────▼─────────────────────────▼───┐
                    │            GridwayApp                 │
                    │   (Application + Reporter traits)     │
                    └───────────────┬──────────────────────┘
                                    │
                    ┌───────────────▼──────────────────────┐
                    │             BaseApp                    │
                    │   ┌─────────────────────────────┐    │
                    │   │      ComponentHost           │    │
                    │   │   ┌────────┬────────┬─────┐ │    │
                    │   │   │validate│  bank  │ hook│ │    │
                    │   │   │ .wasm  │ .wasm  │.wasm│ │    │
                    │   │   └───┬────┴───┬────┴──┬──┘ │    │
                    │   └───────┼────────┼───────┼────┘    │
                    │           │ kvstore WIT    │          │
                    │   ┌───────▼────────▼───────▼────┐    │
                    │   │    Virtual Filesystem (VFS)   │    │
                    │   │  /bank/  /auth/  /staking/    │    │
                    │   └───────────────┬──────────────┘    │
                    │                   │                    │
                    │   ┌───────────────▼──────────────┐    │
                    │   │  GlobalAppStore (namespaced)   │    │
                    │   └───────────────┬──────────────┘    │
                    │                   │                    │
                    │   ┌───────────────▼──────────────┐    │
                    │   │  MerkleStore (Patricia trie)  │    │
                    │   │  SHA-256, deterministic root   │    │
                    │   └──────────────────────────────┘    │
                    └──────────────────────────────────────┘
```

---

## Timeline

```
Week 1: Phase 1 (Testnet Foundation)
  Day 1-2:  Task 1.1 — Genesis state initialization
  Day 3-7:  Task 1.2 — Multi-node integration test

Week 2: Phase 2 + 3 (User Interaction + Verification)
  Day 1-3:  Task 2.1 — TX submission HTTP endpoint
  Day 3-4:  Task 2.2 — Balance query endpoint
  Day 4-5:  Task 3.1 — End-to-end transfer test
  Day 5:    Task 3.2 — Restart and replay test
```

**Total: ~10-12 working days (2 weeks)**

---

## Milestones

### M1: Multi-Node Consensus (Week 1)
- [ ] Genesis state loaded on first start
- [ ] 3 nodes connected via P2P
- [ ] Blocks finalized with matching state roots
- [ ] Height advances on all nodes

### M2: Token Transfer (Week 2 mid)
- [ ] TX submission via HTTP
- [ ] Balance query via HTTP
- [ ] MsgSend executed through WASM bank module
- [ ] Balance changes reflected on all nodes

### M3: Resilience (Week 2 end)
- [ ] Node restart with block replay
- [ ] State consistency after replay
- [ ] Node re-joins consensus after catch-up

---

## Post-MVP Roadmap

1. **Persistent trie backend** — RocksDB/sled for MerkleStore (eliminates replay dependency)
2. **Component storage in Merkle trie** — WASM binaries stored on-chain, governance upgrades
3. **FD-based capabilities** — Unforgeable handles replacing path-based model
4. **Additional WASM modules** — Staking, governance voting, IBC handler
5. **State sync protocol** — P2P snapshot transfer for fast node sync
6. **Performance optimization** — Parallel WASM execution, AOT compilation, trie caching
