# Phase 0 Final Code Review

**Date:** 2025-07-08  
**Reviewer:** Subagent (opus-level code review)  
**Scope:** Phase 0 "기반 배관 연결" — BaseApp→JMT, VFS→JMT, commit(), VFS↔WASI 브릿지  
**Test Status:** 82 passed, 0 failed

---

## Checklist Results

### 1. BaseApp이 JMTStore를 사용하는가? MemStore 잔재 없는가?

**⚠️ Workaround 있음**

BaseApp은 JMTStore를 올바르게 사용한다:
```rust
// lib.rs:253-256
let jmt_store = JMTStore::new("state".to_string(), data_dir.join("state.db"))
    .map_err(|e| BaseAppError::Store(...))?;
let global_store = Arc::new(GlobalAppStore::new(jmt_store));
```

**그러나** MemStore 잔재가 프로덕션 코드에 남아있다:
```rust
// lib.rs:264-265 (BaseApp::with_data_dir)
let store = Arc::new(std::sync::Mutex::new(MemStore::new()));  // ← LEGACY
```
이 MemStore는 `ComponentHost::new(store.clone())`에 전달되지만, ComponentHost 내부에서 `_base_store`로 받아서 **실제로 사용하지 않는다** (underscore prefix). VFS가 실제 state 경로를 담당한다.

**영향도:** 기능적으로 문제 없음. 불필요한 메모리 할당만 발생.  
**수정 제안:** MemStore 생성 제거, ComponentHost::new() 시그니처에서 base_store 파라미터 제거.

---

### 2. GlobalAppStore가 올바르게 생성되고 namespace별로 분리되는가?

**✅ 완전 구현**

- `GlobalAppStore::new(jmt_store)` — 단일 JMTStore를 shared `Arc<Mutex<JMTStore>>`로 래핑
- `register_namespace("bank", false)` 등으로 auth/bank/staking/gov 네 개 네임스페이스 등록
- `NamespacedStore`가 key prefix로 분리: `"{namespace}/{key}"` 형식
- 중복 등록 방지, read-only 지원, 미등록 namespace 접근 차단
- 테스트에서 isolation 검증 완료

---

### 3. VFS에 JMT-backed NamespacedStore가 올바르게 마운트되는가?

**✅ 완전 구현**

`BaseApp::setup_jmt_stores()` 메서드가 명확하게 동작:
```rust
for ns in ["auth", "bank", "staking", "gov"] {
    let ns_store = global_store.get_namespace(ns)?;          // NamespacedStore 생성
    let store_arc: Arc<Mutex<dyn KVStore>> = Arc::new(Mutex::new(ns_store));
    vfs.mount_store(ns.to_string(), store_arc)?;             // VFS에 마운트
    vfs.add_capability(Capability::Read(ns_path.clone()))?;  // R/W 권한 부여
    vfs.add_capability(Capability::Write(ns_path))?;
}
```

핵심 포인트: VFS에 마운트된 store와 GlobalAppStore가 **동일한 `Arc<Mutex<JMTStore>>`를 공유**하므로, 어느 경로로든 같은 JMT state에 접근한다.

---

### 4. commit()이 실제 merkle hash를 계산하는가? placeholder 없는가?

**⚠️ Workaround 있음 — 의도적 단순화**

`BaseApp::commit()`:
```rust
let root_hash = {
    let store_arc = self.global_store.get_store();
    let mut store = store_arc.lock()?;
    store.commit()?   // ← JMTStore::commit() 호출
};
```

`JMTStore::commit()` → `update_batch()` → `compute_root_hash()`:
```rust
pub fn compute_root_hash(&self) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(self.version.to_be_bytes());
    hasher.update(self.name.as_bytes());
    // ← 모든 committed entries를 정렬 후 해시
    let mut entries: Vec<_> = self.committed.iter().collect();
    entries.sort_by(...);
    for (key, value) in entries { ... }
    hasher.finalize().into()
}
```

**분석:**
- ❌ 진짜 Jellyfish Merkle Tree 아님 — sorted entries의 flat SHA256 hash
- ✅ 결정론적(deterministic) — 같은 state → 같은 hash (테스트 검증됨)
- ✅ State 변화 시 hash 변경됨 (테스트 검증됨)
- ✅ RocksDB에 버전별 root hash 저장
- ⚠️ Merkle proof가 진짜 proof 아님 (simple hash, not tree proof)

코드 내 주석이 이를 인정:
> "For now, this is a hybrid approach that uses RocksDB for persistence but maintains the JMT interface for future full integration"

**판정:** Phase 0으로는 acceptable. Phase 1에서 실제 JMT 라이브러리 연동 필요.

---

### 5. VFS read_key/write_key가 실제로 JMT store까지 도달하는가?

**✅ 완전 구현**

`vfs.rs`의 direct key access 메서드 체인:

```
VFS::write_key("bank", key, value)
  → check_access("/bank", "write")     // capability 검사
  → stores.get("bank")                 // NamespacedStore 조회
  → store.lock()                       // mutex lock
  → KVStore::set(key, value)           // NamespacedStore::set()
    → prefix_key(key)                  // "bank/{key}" 형식
    → self.store.lock()                // JMTStore mutex lock
    → JMTStore::set(prefixed_key, val) // pending에 stage
```

테스트 `test_vfs_helper_methods_consistency`가 cross-path 일관성을 검증:
- helper로 쓰고 VFS로 읽기 ✅
- VFS로 쓰고 helper로 읽기 ✅

---

### 6. ComponentHost의 kvstore interface가 실제로 VFS를 호출하는가?

**✅ 완전 구현**

`ComponentHost::set_vfs(vfs)` 호출 후, 모든 `execute_*` 메서드에서:
```rust
ComponentState { vfs: self.vfs.clone(), ... }
```
로 VFS 참조를 ComponentState에 전달.

kvstore::HostStore 구현이 실제로 VFS를 호출하는지 확인:
- `get()` → `vfs.read_key(&namespace, &key)` ✅
- `set()` → `vfs.write_key(&namespace, &key, &value)` ✅
- `delete()` → `vfs.delete_key(&namespace, &key)` ✅
- `has()` → `vfs.has_key(&handle.namespace, &key)` ✅
- `range()` → `vfs.range_keys(&handle.namespace, ...)` ✅

**단순 `Ok(())` 반환 아님** — 모든 메서드가 VFS를 통해 실제 store 접근.

---

### 7. kvstore::Host::open_store()가 namespace 검증을 하는가?

**✅ 완전 구현**

```rust
fn open_store(&mut self, name: String) -> Result<Resource<Store>, String> {
    let vfs = self.vfs.as_ref()
        .ok_or_else(|| "VFS not available".to_string())?;  // VFS 존재 확인
    
    if !vfs.has_namespace(&name) {
        return Err(format!("Store '{name}' not found"));    // namespace 검증
    }
    // ... VfsStoreHandle 생성 및 resource table에 등록
}
```

테스트 검증:
- `test_kvstore_host_open_nonexistent_store` — 미존재 namespace 거부 ✅
- `test_kvstore_host_no_vfs` — VFS 없으면 거부 ✅

---

### 8. kvstore::HostStore의 get/set/delete/has/range가 모두 VFS를 통해 동작하는가?

**✅ 완전 구현**

모든 5개 메서드가 동일한 패턴으로 구현:
1. VFS 참조 획득
2. Resource handle에서 VfsStoreHandle 조회
3. namespace 추출
4. VFS의 해당 메서드 호출

`test_kvstore_host_via_vfs` 테스트가 set → get → has → delete → get 전체 사이클 검증.
`test_kvstore_host_range_query` 테스트가 range query 검증 (prefix + limit).

---

### 9. WASM 모듈이 kvstore를 통해 state에 접근할 수 있는 완전한 경로가 있는가?

**✅ 완전 구현**

Complete data path:
```
WASM module
  → kvstore WIT interface (open_store, get, set, ...)
  → wasmtime linker (kvstore::add_to_linker)
  → ComponentState (kvstore::Host/HostStore impl)
  → VirtualFilesystem (read_key/write_key/...)
  → NamespacedStore (prefixed key access)
  → JMTStore (pending → RocksDB)
  → commit() → merkle hash
```

모든 `execute_*` 메서드(ante_handler, tx_decoder, begin_blocker, end_blocker)에서:
```rust
kvstore::add_to_linker::<ComponentState, ...>(&mut linker, |state| state)?;
```
로 kvstore를 linker에 등록한다.

---

### 10. 테스트가 실제로 의미있는 assertion을 하는가?

**✅ 대부분 의미있음**

**강한 테스트 (meaningful assertions):**
| 테스트 | 검증 내용 |
|--------|----------|
| `test_vfs_jmt_end_to_end` | VFS write → VFS read 값 일치, GlobalAppStore cross-read |
| `test_vfs_jmt_commit_persistence` | commit 후 hash ≠ zero, 값 유지, hash 변경 |
| `test_vfs_jmt_deterministic_hash` | 동일 operation → 동일 hash |
| `test_vfs_namespace_isolation` | 같은 key, 다른 namespace → 다른 값 |
| `test_vfs_helper_methods_consistency` | helper↔VFS cross-path 일관성 |
| `test_kvstore_host_via_vfs` | WASI host → VFS 전체 CRUD 사이클 |
| `test_kvstore_host_range_query` | range prefix + limit |
| `test_namespace_isolation` (global.rs) | namespace 분리 실제 값 비교 |
| `test_jmt_store_persistence` | RocksDB 재시작 후 데이터 유지 |

**약한 테스트 (trivially passing 위험):**
| 테스트 | 문제 |
|--------|------|
| `test_minimal_wasi_module` | component 없으면 `return` (skip) |
| `test_wasi_module_direct` | component 없으면 `return` (skip) |
| `test_check_tx` | `assert_eq!(response.code, 1)` — 실패만 확인 |

---

### 11. #[allow(dead_code)]나 TODO/FIXME/placeholder 주석이 남아있는가?

**⚠️ 상당수 잔존**

**#[allow(dead_code)] — 9개:**
| 위치 | 대상 | 수준 |
|------|------|------|
| lib.rs:192 | `name` field | struct field, 경미 |
| lib.rs:197 | `wasi_host` field | struct field, 경미 |
| lib.rs:208 | `capability_manager` field | struct field, 경미 |
| lib.rs:486 | `BeginBlockResponse.success` | serde struct, OK |
| lib.rs:664 | `ConsensusParams` | serde struct, OK |
| lib.rs:670 | `ConsensusParams` fields | serde struct, OK |
| lib.rs:974 | `DecodeResponse` fields | serde struct, OK |
| component_host.rs:98 | `component_name` | struct field, 경미 |
| global.rs:135 | `strip_prefix` method | 유틸리티, 경미 |

**TODO 주석 — 17개:**
| 수준 | 내용 | Phase 0 영향 |
|------|------|-------------|
| Phase 1+ | `byzantine_validators` from tendermint | 없음 |
| Phase 1+ | `total_power`, `proposer_address` from modules | 없음 |
| Phase 1+ | Process validator updates | 없음 |
| Phase 1+ | `min_gas_price` from config | 없음 |
| Phase 1+ | WASI module state query for height | 없음 |
| Phase 1+ | Transaction simulation via WASI | 없음 |
| Phase 1+ | Proposal validation/ordering via WASI | 없음 |
| Phase 1+ | Vote extension via WASI | 없음 |
| Phase 1+ | SdkMsg for unknown message types | 없음 |
| **주의** | `rollback()` → `Ok(())` (no-op) | ⚠️ 에러 복구 불가 |
| **주의** | `init_chain()` → `Ok(())` (no-op) | ⚠️ genesis 미처리 |
| **주의** | `query()` → placeholder response | ⚠️ 쿼리 미작동 |

**Placeholder 코드 — 3곳:**
- `decode_transaction_wasi()`: component 없으면 빈 tx JSON 반환 (line 1043-1054)
- `execute_begin_block_wasi()`: component 없으면 빈 events 반환 (line 584-591)
- `execute_end_block_wasi()`: component 없으면 빈 events 반환 (line 779-786)

---

### 12. 이전 검증에서 발견된 이슈들이 해결되었는가?

**⚠️ 부분 해결**

| 이슈 | 상태 | 설명 |
|------|------|------|
| MemStore → JMT 전환 | ✅ 해결 | GlobalAppStore가 JMTStore 사용 |
| VFS JMT 연결 | ✅ 해결 | setup_jmt_stores()로 마운트 |
| commit() placeholder | ✅ 해결 | SHA256 hash 계산 (진짜 JMT 아님, 의도적) |
| VFS↔WASI 브릿지 | ✅ 해결 | kvstore Host impl → VFS → JMT |
| MemStore 잔재 | ⚠️ 미완 | lib.rs:264-265에 legacy MemStore 생성 존재 |
| dead_code | ⚠️ 미완 | struct field 3개, method 1개 |
| E2E 테스트 | ✅ 해결 | VFS→JMT end-to-end, deterministic hash 등 추가 |

---

## 위험도 분석

### 🟢 Low Risk (Phase 0 목적 완전 달성)
- **배관 연결 완료**: BaseApp → GlobalAppStore → JMTStore → RocksDB
- **VFS 브릿지 완료**: VFS ← NamespacedStore ← JMTStore (공유 Arc)
- **WASI 브릿지 완료**: kvstore WIT → ComponentState → VFS → store
- **commit() 작동**: deterministic hash, state 변화 반영, RocksDB 영속
- **Namespace 격리**: key prefix 방식, capability 기반 접근 제어

### 🟡 Medium Risk (Phase 1에서 반드시 해결)
1. **JMT 미완성**: `compute_root_hash()`가 flat hash — Merkle proof 불가. Light client 검증 불가능.
2. **MemStore 잔재**: 기능적 영향 없지만 혼란 유발. 제거 필요.
3. **rollback() no-op**: 블록 실행 실패 시 state 복구 불가.
4. **init_chain() no-op**: genesis state 초기화 미구현.
5. **query() placeholder**: state query가 빈 응답 반환.

### 🔴 High Risk (없음)
Phase 0 범위 내에서 critical blocker 없음.

---

## 최종 판정

**Phase 0: PASS ✅ (조건부)**

핵심 "배관 연결" 목표 — BaseApp→JMT, VFS→JMT, commit(), VFS↔WASI 브릿지 — 는 모두 달성됨. 데이터가 WASM module에서 JMT store까지 완전한 경로로 흐르며, commit() 시 deterministic hash가 생성됨.

**Phase 1 진입 전 권장 정리:**
1. `lib.rs:264-265` MemStore 생성 제거 + ComponentHost 시그니처 정리
2. `rollback()`, `init_chain()`, `query()` 메서드에 명시적 `unimplemented!()` 또는 `Phase1` 주석 추가
3. struct field dead_code 중 `wasi_host`, `capability_manager`가 실제 사용 계획 있는지 확인

---

*총 53개 테스트 (#[test] 기준), 모든 Phase 0 관련 data path가 테스트로 검증됨.*
