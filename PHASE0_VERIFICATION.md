# Phase 0 Verification Report

**Date:** 2025-07-15  
**Verifier:** Automated Code Analysis (no build available in sandbox)  
**Scope:** Phase 0 State Foundation — plumbing correctness

---

## 1. BaseApp → JMT 연결 검증

### 1.1 BaseApp이 실제로 JMTStore를 사용하는가?

**✅ 실제 동작 — 단, MemStore 잔재가 존재함**

**JMT 연결 (정상):**
- `BaseApp::with_data_dir()` (lib.rs:237-241)에서 `JMTStore::new()` → `GlobalAppStore::new()` 생성
- `global_store` 필드로 `Arc<GlobalAppStore>` 저장
- 4개 namespace (`bank`, `auth`, `staking`, `gov`) 등록
- `set_balance()` / `get_balance()`가 `self.global_store.set_namespaced("bank", ...)` 사용 — **실제 JMT 경로**

**⚠️ MemStore 잔재 (lib.rs:259-260):**
```rust
// Create a MemStore for ComponentHost compatibility (legacy)
let store = Arc::new(std::sync::Mutex::new(MemStore::new()));
```
이 MemStore는 `ComponentHost::new(store.clone())`에 전달됨. ComponentHost 내부에서 이 store는 `_base_store`로 받아서 사실상 **사용하지 않음** (component_host.rs:308 `_base_store`). 그러나 여전히 생성되고 있어 혼란을 줄 수 있음.

**테스트 코드에도 MemStore 잔재:**
- `test_minimal_wasi_module` (lib.rs:1689): `Arc::new(Mutex::new(MemStore::new()))` — ComponentHost 테스트용 (JMT와 무관)
- `test_wasi_module_direct` (lib.rs:1747): 동일

**결론:** 핵심 상태 경로(set_balance/get_balance/commit)는 JMT를 통하지만, ComponentHost에 전달되는 legacy MemStore가 코드를 혼란스럽게 함.

### 1.2 set_balance() → commit() → get_balance() 동작 검증

**✅ 실제 동작**

코드 경로 분석:
1. `set_balance("alice", "ugridway", 1000)` (lib.rs)
   → `global_store.set_namespaced("bank", b"balance_alice_ugridway", b"1000")`
   → `NamespacedStore::set()` (global.rs:94)
   → prefix_key: `"bank/balance_alice_ugridway"` 
   → `JMTStore::set()` (jmt.rs:148)
   → `stage_change()` — pending HashMap에 저장

2. `commit()` (lib.rs)
   → `global_store.get_store().lock().commit()` 
   → `JMTStore::commit()` (jmt.rs:127)
   → pending → `update_batch()` → RocksDB put + committed HashMap + SHA256 root hash

3. `get_balance("alice", "ugridway")` (lib.rs)
   → `global_store.get_namespaced("bank", b"balance_alice_ugridway")`
   → `NamespacedStore::get()` 
   → `JMTStore::get()` — pending → committed → RocksDB fallback

**전체 경로가 JMT를 관통함. 목업 없음.**

### 1.3 기존 테스트의 의미

**test_commit (lib.rs:1786-1795):**
```rust
let hash = app.commit().unwrap();
assert_eq!(hash.len(), 32);
app.set_balance("alice", "ugridway", 1000).unwrap();
let hash2 = app.commit().unwrap();
assert_ne!(hash2, vec![0u8; 32], "commit should return non-zero hash after state change");
```
⚠️ **부분적으로 의미있음.** hash.len() == 32는 형식 검증만 하고, 첫 번째 commit이 zero hash가 아닌지는 검증하지 않음 (빈 트리의 commit은 pending이 없으면 `root_hash()` 반환 → 초기엔 [0;32]일 수 있음). 두 번째 commit 후 non-zero 검증은 유효.

**빠진 assertion:** `hash != hash2` (상태 변경 전후 해시 달라야 함)

**test_baseapp_integration (lib.rs:1800-1810):**
```rust
app.set_balance("alice", "uatom", 1000).unwrap();
let balance = app.get_balance("alice", "uatom").unwrap();
assert_eq!(balance, 1000);
```
⚠️ **부분적으로 의미있음.** commit() 없이 read-after-write만 테스트. 이것은 JMT의 pending 상태에서 읽는 것이므로, JMT persistence나 merkle tree를 실제로 검증하지 않음. commit → reopen → read가 더 의미있는 테스트.

---

## 2. commit() 검증

### 2.1 실제 SHA256 Merkle Root 반환?

**✅ 실제 SHA256 해시 반환 — 단, "진짜 JMT"는 아님**

`JMTStore::compute_root_hash()` (jmt.rs:84-98):
```rust
pub fn compute_root_hash(&self) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(self.version.to_be_bytes());
    hasher.update(self.name.as_bytes());
    let mut entries: Vec<_> = self.committed.iter().collect();
    entries.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (key, value) in entries {
        hasher.update(key);
        hasher.update(value);
    }
    hasher.finalize().into()
}
```

**⚠️ 중요한 한계:** 이것은 "flat hash" — 모든 key-value를 정렬 후 연결하여 SHA256. **Jellyfish Merkle Tree가 아님.** 진짜 JMT는 트리 구조의 내부 노드 해시를 사용. 현재 구현은:
- 결정론적 ✅ (sorted entries)
- Non-zero after state change ✅
- SHA256 ✅
- 실제 Merkle proof 지원 ❌ (generate_simple_proof는 트리 기반이 아님)
- O(n) 해시 계산 (전체 데이터 스캔) — 스케일 문제 있음

### 2.2 placeholder인가?

**✅ 아님.** `[0; 32]`은 빈 상태(pending 없음)에서만 반환됨. 데이터가 있으면 실제 SHA256 해시.

### 2.3 결정론적인가?

**✅ 결정론적.** `committed` HashMap을 정렬 후 해싱하므로 동일 입력 → 동일 해시.

### 2.4 상태 변경 후 다른 해시?

**✅ 다른 해시.** version이 포함되고 entries가 변경되므로 해시가 달라짐.

단, `commit()`의 빈 호출 (pending.is_empty()) 경우:
```rust
if self.pending.is_empty() {
    return Ok(self.root_hash());  // 마지막 저장된 root hash 또는 [0;32]
}
```
첫 commit이 빈 상태에서 [0;32] 반환 가능 → test_commit의 첫 assertion이 이를 놓침.

---

## 3. VFS ↔ WASI 브릿지 검증 (가장 중요)

### 3.1 ComponentHost에 VFS가 전달되는가?

**✅ 전달됨**

lib.rs:
```rust
let vfs = Arc::new(VirtualFilesystem::new());
Self::setup_jmt_stores(&vfs, &global_store)?;
component_host_inner.set_vfs(vfs.clone());
```

component_host.rs:
```rust
pub fn set_vfs(&mut self, vfs: Arc<VirtualFilesystem>) {
    self.vfs = Some(vfs);
}
```

각 ComponentState에도 전달:
```rust
ComponentState {
    ...
    vfs: self.vfs.clone(),  // 모든 execute_* 메서드에서
}
```

### 3.2 WASI 모듈이 VFS를 통해 실제로 read/write 할 수 있는가?

**❌ 아직 연결만 됨. 실제 사용 불가.**

핵심 문제: VFS는 ComponentState에 `Option<Arc<VirtualFilesystem>>`로 전달되지만, **실제로 WASI 런타임에서 접근하는 경로가 없음.**

현재 상태:
1. ComponentState에 `vfs` 필드 존재 → `#[allow(dead_code)]`로 마킹됨 (component_host.rs:104)
2. WASI 모듈은 WIT interface를 통해 host function을 호출해야 state에 접근
3. 기존 kvstore WIT interface는 **전부 주석 처리됨** (component_host.rs의 `/* ... */` 블록들)
4. VFS를 사용하는 새 WIT interface는 **아직 구현되지 않음**

**현재 WASI 모듈이 state에 접근하는 유일한 경로:**
- `ComponentState.kvstore_manager` (SimpleKVStoreManager) — 빈 상태로 생성됨
- 주석 처리된 `kvstore::Host` implementation
- WASI 모듈은 stdout/stdin JSON 교환만 가능

### 3.3 VFS file operations가 JMT store까지 도달하는가?

**⚠️ VFS 자체는 JMT까지 연결됨. 하지만 WASI에서 VFS를 호출하는 경로가 없음.**

VFS 내부 경로 (이것 자체는 동작):
```
VFS.open("/bank/balance_alice") 
  → parse_path → namespace="bank", key="balance_alice"
  → stores["bank"] = NamespacedStore (JMT-backed)
  → NamespacedStore.get("balance_alice") 
  → JMTStore.get("bank/balance_alice")
```

VFS 단위 테스트가 이를 증명 (vfs.rs의 tests). 하지만 이 테스트는 MemStore를 사용하지, JMT를 사용하지 않음.

**결론: VFS → JMT 경로는 코드상 올바르지만, WASI → VFS 경로가 끊어져 있음.**

### 3.4 브릿지 상태 정리

```
WASI Module                        
     │                              
     ├─ stdout/stdin JSON ─── ComponentHost (동작)
     │                              
     ├─ kvstore WIT ──── 주석 처리됨 (❌)
     │                              
     └─ VFS WIT ──── 미구현 (❌)     
                                    
ComponentHost.vfs ──── VFS (연결됨 ✅, 사용 안 됨)
                        │
                  NamespacedStore (연결됨 ✅)
                        │
                    JMTStore/RocksDB (동작 ✅)
```

---

## 4. 테스트 상태 확인

### 4.1 기존 테스트의 Phase 0 호환성

| 테스트 | 상태 | 비고 |
|--------|------|------|
| `test_new_base_app` | ✅ 통과 예상 | JMTStore가 temp dir로 생성 |
| `test_begin_end_block` | ⏸ `#[ignore]` | kvstore interface 제거 중이라 무시 |
| `test_check_tx` | ✅ 통과 예상 | WASI 모듈 없으면 code=1 반환 |
| `test_deliver_tx` | ✅ 통과 예상 | context 없으면 에러, 있으면 code≠0 |
| `test_commit` | ✅ 통과 예상 | tempdir + JMT commit |
| `test_finalize_block` | ⏸ `#[ignore]` | kvstore interface 제거 중 |
| `test_baseapp_integration` | ✅ 통과 예상 | tempdir + JMT set/get |
| `test_module_governance_integration` | ✅ 통과 예상 | governance 독립적 |
| `test_governance_message_handling` | ✅ 통과 예상 | check_tx → code=1 |
| `test_prepare_proposal` | ✅ 통과 예상 | 단순 passthrough |
| `test_process_proposal` | ✅ 통과 예상 | 항상 accept |
| `test_extend_vote` | ✅ 통과 예상 | 빈 extension 반환 |
| `test_verify_vote_extension` | ✅ 통과 예상 | 항상 valid |
| `test_check_tx_with_recheck_mode` | ✅ 통과 예상 | code=1 |
| `test_execution_context_exec_mode` | ✅ 통과 예상 | context enum 확인만 |
| `test_minimal_wasi_module` | ⏭ 스킵 | .wasm 파일 없으면 early return |
| `test_wasi_module_direct` | ⏭ 스킵 | .wasm 파일 없으면 early return |

### 4.2 gridway-store 테스트

| 테스트 | 상태 | 비고 |
|--------|------|------|
| JMT store tests (6개) | ✅ | RocksDB 기반, tempdir 사용 |
| GlobalAppStore tests (5개) | ✅ | JMT + namespace isolation |
| MemStore/CacheStore tests | ✅ | 변경 없음 |

### 4.3 새로 추가된 테스트

Phase 0에서 **새로 추가되거나 수정된** 테스트:
- `test_commit` — hash 32바이트 + non-zero 검증 (의미있으나 불완전)
- `test_baseapp_integration` — tempdir + balance round-trip (의미있으나 commit 없음)

**추가 필요한 테스트:**
1. **결정론적 해시 테스트:** 동일 연산 2회 → 동일 해시
2. **해시 변경 테스트:** set_balance → commit → set_balance(다른 값) → commit → hash1 ≠ hash2
3. **Persistence 테스트:** commit → 새 BaseApp 열기(같은 dir) → 데이터 존재 확인
4. **VFS→JMT 통합 테스트:** VFS.open("/bank/...") → write → close → JMT에서 읽기
5. **빈 commit 테스트:** 첫 commit이 [0;32]인지 아닌지 명확히

---

## 5. 발견된 문제점 요약

### 🔴 Critical
1. **VFS↔WASI 브릿지가 실질적으로 끊어져 있음.** ComponentState.vfs는 `#[allow(dead_code)]`이고, WASI 모듈이 VFS를 호출할 WIT interface가 없음. Phase 1A에서 해결 예정이지만, Phase 0 완료 기준에서는 "연결만 해놓은" 상태.

### 🟡 Important  
2. **JMTStore가 진짜 JMT가 아님.** flat SHA256 해시. Merkle proof는 dummy. 이름이 "JMTStore"이므로 오해 유발. 스케일 커지면 O(n) 해시 계산이 병목.
3. **MemStore 잔재** — ComponentHost에 전달되는 MemStore는 사실상 unused. 코드 혼란.
4. **test_commit의 첫 commit은 빈 pending → [0;32] 반환 가능.** 이것이 non-zero인지 검증 안 함.

### 🟢 Minor
5. **test_baseapp_integration에서 commit() 없이 read-after-write만 테스트** — JMT pending에서 읽는 것이라 persistence 검증 안 됨.
6. **VFS 테스트가 MemStore 사용** — JMT-backed VFS의 end-to-end 테스트 없음.

---

## 6. 수정 권장 사항

### 즉시 수정 (Phase 0 완료 전)
1. `test_commit`에 추가: 첫 commit이 [0;32]인지 확인, hash1 ≠ hash2 검증
2. `test_baseapp_integration`에 commit() 추가
3. ComponentHost에 전달되는 unused MemStore에 주석 강화 또는 제거

### Phase 1A에서 수정
4. VFS-backed kvstore WIT interface 구현 → WASI 모듈이 실제로 VFS 통해 state 접근
5. JMTStore를 실제 Jellyfish Merkle Tree로 교체 (또는 이름 변경)

### 추가 테스트 작성
6. 결정론적 해시 테스트
7. JMT-backed VFS end-to-end 테스트
8. Persistence (reopen) 테스트

---

## 7. 최종 판정

| 검증 항목 | 판정 | 설명 |
|-----------|------|------|
| BaseApp→JMT 연결 | ✅ 실제 동작 | 핵심 경로 (balance, commit) 모두 JMT 관통 |
| commit() SHA256 | ⚠️ 부분적 | 실제 SHA256이지만 진짜 JMT Merkle Tree 아님 |
| commit() 결정론 | ✅ 실제 동작 | sorted entries + version → 결정론적 |
| VFS→JMT 마운트 | ✅ 실제 동작 | NamespacedStore가 VFS에 올바르게 마운트 |
| VFS↔WASI 브릿지 | ❌ 목업 | 연결 구조만 존재, 실제 호출 경로 없음 |
| 기존 테스트 호환 | ✅ 통과 예상 | backward-compatible temp dir |
| 새 테스트 의미 | ⚠️ 부분적 | 기본 검증은 하나 핵심 assertion 누락 |

**전체 평가: Phase 0 핵심 목표(JMT 연결, commit)는 실제로 동작하지만, VFS↔WASI 브릿지는 구조만 있고 실제 사용은 Phase 1A 의존.**
