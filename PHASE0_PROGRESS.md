# Phase 0: State Foundation — Progress

**Date:** 2025-07-14
**Status:** Implementation Complete — Needs Build Verification

---

## Summary

Phase 0 implements the "plumbing" that connects the existing JMT store to BaseApp and VFS,
replacing the ephemeral MemStore with persistent, merkle-authenticated storage.

---

## Task 0.1: BaseApp JMT Integration ✅

### Changes
- **`crates/gridway-baseapp/src/lib.rs`**:
  - Added `global_store: Arc<GlobalAppStore>`, `data_dir: PathBuf`, `last_app_hash: Vec<u8>` fields to `BaseApp`
  - Added `with_data_dir(name, data_dir)` constructor that creates JMTStore + GlobalAppStore
  - `new()` delegates to `with_data_dir()` with auto-generated temp dir
  - Registers namespaces: `bank`, `auth`, `staking`, `gov`
  - Added `global_store()` accessor method
  - `set_balance()` / `get_balance()` now write/read from JMT-backed bank namespace

- **`crates/gridway-server/src/bin/gridway-server.rs`**:
  - `Start` command now passes `cli.home.join("data")` to `BaseApp::with_data_dir()`

### Design Decisions
- `BaseApp::new()` auto-creates a temp dir using `{temp_dir}/gridway-{name}-{pid}` for backward compatibility with test code
- For production, `with_data_dir()` should always be used with an explicit path
- ComponentHost still receives a legacy MemStore for compatibility; actual state flows through GlobalAppStore → VFS

---

## Task 0.2: VFS Mounted to GlobalAppStore ✅

### Changes
- **`crates/gridway-baseapp/src/lib.rs`**:
  - Replaced `setup_default_stores()` (which created 4 MemStores) with `setup_jmt_stores()`
  - `setup_jmt_stores()` gets `NamespacedStore` views from `GlobalAppStore` and mounts them as `Arc<Mutex<dyn KVStore>>` into VFS
  - Added read/write VFS capabilities for each namespace (`/auth`, `/bank`, `/staking`, `/gov`)

### Architecture Note
The VFS now provides a filesystem-like interface to JMT state:
```
/bank/balance_cosmos1abc_ugridway  →  GlobalAppStore["bank"]["balance_cosmos1abc_ugridway"]
/auth/account_cosmos1abc           →  GlobalAppStore["auth"]["account_cosmos1abc"]
```
Each namespace is isolated. WASI modules will access state through VFS file operations.

---

## Task 0.3: commit() Returns Real Merkle Root Hash ✅

### Changes
- **`crates/gridway-baseapp/src/lib.rs`**:
  - `commit()` now calls `self.global_store.get_store().lock().commit()` which flushes pending JMT changes and returns SHA256 root hash
  - Stores `last_app_hash` for `get_last_app_hash()` queries
  - Logs block height and hex-encoded app hash

### Before → After
```rust
// BEFORE:
pub fn commit(&mut self) -> Result<Vec<u8>> {
    Ok(vec![0u8; 32]) // placeholder
}

// AFTER:
pub fn commit(&mut self) -> Result<Vec<u8>> {
    let root_hash = self.global_store.get_store().lock()?.commit()?;
    self.last_app_hash = root_hash.to_vec();
    Ok(root_hash.to_vec())
}
```

---

## Task 0.4: VFS↔WASI Bridge ✅

### Changes
- **`crates/gridway-baseapp/src/component_host.rs`**:
  - Added `vfs: Option<Arc<VirtualFilesystem>>` field to `ComponentHost`
  - Added `vfs: Option<Arc<VirtualFilesystem>>` field to `ComponentState`
  - Added `set_vfs()` and `vfs()` methods to `ComponentHost`
  - All `ComponentState` instantiations (ante-handler, begin-blocker, end-blocker, tx-decoder) now receive VFS reference

- **`crates/gridway-baseapp/src/lib.rs`**:
  - After creating VFS and ComponentHost, calls `component_host_inner.set_vfs(vfs.clone())` to bridge them
  - ComponentHost is now created as mutable first, VFS is set, then wrapped in `Arc`

### Architecture Note
The VFS bridge enables future WASI modules (e.g., bank.wasm in Phase 1A) to access state:
```
WASI Module → ComponentState.vfs → VFS → NamespacedStore → JMTStore → RocksDB
```
The actual kvstore WIT interface (currently commented out) will be re-implemented to use VFS in Phase 1A.

---

## Test Updates

Updated tests in `crates/gridway-baseapp/src/lib.rs`:
- `test_baseapp_integration` — uses temp dir, verifies balance write/read through JMT
- `test_commit` — verifies commit returns non-zero hash after state changes

---

## Files Modified

| File | Changes |
|------|---------|
| `crates/gridway-baseapp/src/lib.rs` | JMT integration, VFS mount, commit(), balance helpers, tests |
| `crates/gridway-baseapp/src/component_host.rs` | VFS bridge to ComponentHost/ComponentState |
| `crates/gridway-server/src/bin/gridway-server.rs` | Pass data_dir to BaseApp |

---

## Verification Needed

1. `cargo build --workspace --exclude ante-handler --exclude begin-blocker --exclude end-blocker --exclude tx-decoder`
2. `cargo test --workspace --exclude ante-handler --exclude begin-blocker --exclude end-blocker --exclude tx-decoder`
3. Key tests:
   - `test_baseapp_integration` — balance round-trip through JMT
   - `test_commit` — non-zero hash after state change
   - Existing tests should still pass (BaseApp::new() backward compatible)

---

## Next Steps (Phase 1A)

1. Create bank WASM component (`crates/wasi-modules/bank/`)
2. Implement kvstore WIT interface backed by VFS
3. Route `/cosmos.bank.v1beta1.MsgSend` to bank.wasm in `execute_transaction()`
