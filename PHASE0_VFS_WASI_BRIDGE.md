# Phase 0: VFS↔WASI Bridge Completion

**Date:** 2025-07-16  
**Scope:** Activate the end-to-end path: WASM module → kvstore WIT interface → VFS → NamespacedStore → JMTStore  
**Status:** ✅ Complete

---

## Summary

The VFS↔WASI bridge was structurally present but non-functional:
- `ComponentState.vfs` existed but was `#[allow(dead_code)]`
- The kvstore WIT Host/HostStore implementations were commented out
- No actual WASM→VFS→JMT data path existed

This changeset activates the full bridge, making WASI modules able to read/write blockchain state through the kvstore interface.

---

## Architecture: Data Flow

```
WASM Module (bank.wasm, ante-handler, etc.)
     │
     │  kvstore::open_store("bank")
     │  kvstore::store.set(key, value)
     │  kvstore::store.get(key)
     ▼
ComponentState (kvstore::Host + HostStore)
     │
     │  VFS capability check
     │  VFS direct key access
     ▼
VirtualFilesystem (vfs.rs)
     │
     │  read_key / write_key / delete_key / has_key / range_keys
     ▼
NamespacedStore (global.rs)
     │
     │  prefix_key: "bank/" + key
     ▼
JMTStore (jmt.rs)
     │
     │  pending → committed → RocksDB
     ▼
RocksDB (persistent storage)
```

---

## Files Modified

### 1. `crates/gridway-baseapp/src/vfs.rs`

**Added 6 direct key access methods** to `VirtualFilesystem`:

| Method | Description |
|--------|-------------|
| `has_namespace(ns)` | Check if a namespace store is mounted |
| `read_key(ns, key)` | Read a value — checks capabilities, accesses store directly |
| `write_key(ns, key, value)` | Write a value — checks capabilities, accesses store directly |
| `delete_key(ns, key)` | Delete a key — checks capabilities, accesses store directly |
| `has_key(ns, key)` | Check key existence — checks capabilities, accesses store directly |
| `range_keys(ns, start, end, limit)` | Range query with prefix filtering |

**Design decision:** These methods check VFS capabilities (same as file operations) but access the underlying store directly, avoiding file descriptor overhead. This is appropriate for the kvstore interface which needs efficient key-value semantics, not file-system semantics.

**Added tests:**
- `test_direct_key_operations` — basic CRUD through direct key methods
- `test_range_keys` — range query with prefix and limit
- `test_direct_key_access_denied` — capability enforcement
- `test_direct_key_ops_namespace_not_found` — error handling

### 2. `crates/gridway-baseapp/src/component_host.rs`

**Major changes:**

1. **Activated kvstore import:**
   ```rust
   use crate::component_bindings::ante_handler::gridway::framework::kvstore;
   ```

2. **Added `VfsStoreHandle` struct:**
   Host-side resource handle mapping WIT `resource store` to a VFS namespace.

3. **Implemented `kvstore::HostStore` for `ComponentState`:**
   - `get` → `vfs.read_key(namespace, key)`
   - `set` → `vfs.write_key(namespace, key, value)`
   - `delete` → `vfs.delete_key(namespace, key)`
   - `has` → `vfs.has_key(namespace, key)`
   - `range` → `vfs.range_keys(namespace, start, end, limit)`
   - `drop` → cleanup resource from table

4. **Implemented `kvstore::Host` for `ComponentState`:**
   - `open_store(name)` → validates namespace exists in VFS, creates `VfsStoreHandle`, returns resource handle

5. **Added `add_kvstore_to_linker` method:**
   Registers the VFS-backed kvstore interface with the wasmtime linker.

6. **Activated kvstore in all `execute_*` methods:**
   - `execute_ante_handler`
   - `execute_tx_decoder`
   - `execute_begin_blocker`
   - `execute_end_blocker`

7. **Cleaned up `ComponentState`:**
   - Removed `kvstore_manager: SimpleKVStoreManager` (unused, replaced by VFS)
   - Removed `#[allow(dead_code)]` from `vfs` field
   - Removed all legacy kvstore TODO comments

8. **Cleaned up `ComponentHost`:**
   - Removed `kvstore_manager` field
   - Removed commented-out `KVStoreResourceHost` code

**Added tests:**
- `test_component_host_with_vfs` — VFS attachment
- `test_kvstore_host_via_vfs` — full CRUD through kvstore Host traits
- `test_kvstore_host_range_query` — range queries through kvstore interface
- `test_kvstore_host_open_nonexistent_store` — error handling
- `test_kvstore_host_no_vfs` — graceful failure without VFS

### 3. `crates/gridway-baseapp/src/lib.rs`

**Removed `#[allow(dead_code)]` from `vfs` field** in `BaseApp` — the VFS is now actively used through the WASI bridge.

**Added integration tests:**
- `test_vfs_jmt_end_to_end` — Write via VFS, read via VFS and GlobalAppStore, cross-verify
- `test_vfs_jmt_commit_persistence` — Write, commit, verify hash changes, read after commit
- `test_vfs_jmt_deterministic_hash` — Same operations produce identical commit hashes
- `test_vfs_namespace_isolation` — Same key in different namespaces yields different values
- `test_vfs_helper_methods_consistency` — set_balance/get_balance consistent with VFS direct access

---

## Resource Model

The WIT kvstore interface uses the Component Model's resource system:

```
open_store("bank")
    → Push VfsStoreHandle { namespace: "bank" } to ResourceTable
    → Return Resource<kvstore::Store> with same rep index
    
store.get(key)
    → Convert Resource<kvstore::Store> → Resource<VfsStoreHandle> via rep
    → Look up VfsStoreHandle in ResourceTable
    → Call vfs.read_key(handle.namespace, key)
    
store.drop()
    → Delete VfsStoreHandle from ResourceTable
```

The `VfsStoreHandle` is a lightweight handle — the actual store access goes through VFS on each operation, ensuring capability checks and store consistency.

---

## What's NOT Changed

- **WIT interface definitions** — `module.wit` kvstore interface unchanged
- **VFS file operations** — `open/read/write/close` still work as before
- **JMTStore** — no changes to the store layer
- **GlobalAppStore** — no changes to namespace management
- **BaseApp public API** — no breaking changes
- **Existing tests** — all remain compatible

---

## Verification Checklist

| Check | Status |
|-------|--------|
| VFS direct key methods work with MemStore | ✅ (unit tests) |
| VFS direct key methods work with JMT-backed stores | ✅ (integration tests) |
| kvstore Host/HostStore impl passes through VFS | ✅ (unit tests) |
| Capability enforcement works | ✅ (access denied tests) |
| Namespace isolation verified | ✅ (isolation test) |
| JMT commit produces deterministic hashes | ✅ (determinism test) |
| Data survives commit cycle | ✅ (persistence test) |
| VFS and GlobalAppStore share same JMT state | ✅ (consistency test) |
| kvstore linker added to all execute_* methods | ✅ (code review) |
| No dead code annotations on bridge components | ✅ |

---

## Updated Bridge Status (was from PHASE0_VERIFICATION.md)

```
WASI Module                        
     │                              
     ├─ stdout/stdin JSON ─── ComponentHost (동작 ✅)
     │                              
     ├─ kvstore WIT ──── VFS-backed implementation (동작 ✅)
     │                              
     └─ VFS WIT ──── Future (Phase 1+)     
                                    
ComponentHost.vfs ──── VFS (연결됨 ✅, 사용됨 ✅)
                        │
                  NamespacedStore (연결됨 ✅)
                        │
                    JMTStore/RocksDB (동작 ✅)
```

**Phase 0 VFS↔WASI Bridge: COMPLETE** — WASI modules can now access JMT-backed state through the kvstore WIT interface via VFS.
