# Gridway MVP Plan: WASM Microkernel Blockchain

**Date:** 2025-07-13  
**Goal:** Two-node chain with real token transfers, powered by WASM microkernel architecture  
**Author:** Generated from codebase analysis of ~35K LOC across 15 crates

---

## Executive Summary

이 플랜은 Gridway의 세 가지 치명적 갭(MemStore 사용, placeholder app hash, 비실행 deliver_tx)을 해결하면서, WASM 마이크로커널 비전을 MVP에 통합하는 구체적 구현 로드맵이다.

**핵심 전략:** "Hybrid Microkernel" — bank 모듈을 WASM 컴포넌트로 구현하되, VFS를 통해 JMT-backed GlobalAppStore에 접근하는 구조. 기존 native 코드 경로를 fallback으로 유지하여 리스크를 관리한다.

### 왜 WASM을 MVP에 포함하는가

GAP_ANALYSIS.md에서는 "WASM은 Phase 3"라고 권고했지만, 다음 이유로 MVP에 통합:

1. **이미 절반은 있다** — ComponentHost, WIT 인터페이스, WASI 런타임이 동작한다. VFS도 구현되어 있다. 연결만 안 되어 있을 뿐.
2. **아키텍처 검증이 핵심** — Gridway의 차별성은 WASM 마이크로커널. native-only MVP를 만들면 아키텍처 검증이 지연된다.
3. **bank 모듈은 간단하다** — MsgSend의 로직(잔액 확인 → 차감 → 증가)은 WASM으로 구현하기에 충분히 간단하다.

---

## Current State Assessment

### 동작하는 것 ✅

| Component | 위치 | 상태 |
|-----------|------|------|
| JMTStore + RocksDB | `gridway-store/src/jmt.rs` | `commit()` → root hash, 영속 저장, prefix iterator |
| GlobalAppStore | `gridway-store/src/global.rs` | 네임스페이스 격리, JMT 기반 |
| VFS | `gridway-baseapp/src/vfs.rs` | open/read/write/close/stat, 캐퍼빌리티 체크, 마운트 |
| ComponentHost | `gridway-baseapp/src/component_host.rs` | wasmtime 기반, 컴포넌트 로딩 & 실행, fuel 미터링 |
| WIT Interfaces | `wit/*.wit` | ante-handler, begin-blocker, end-blocker, tx-decoder, **module(!)** |
| WASI 모듈 소스 | `crates/wasi-modules/` | ante-handler, tx-decoder, begin-blocker, end-blocker 소스 존재 |
| Genesis Types | `gridway-types/src/genesis.rs` | AppGenesis, BankGenesis, 검증, 파일 IO |
| MsgSend Types | `gridway-types/src/msgs/bank.rs` | Protobuf encode/decode, validation |
| ABCI gRPC Server | `gridway-server/` | tonic 기반, 모든 ABCI 2.0 메서드 |
| ModuleRouter | `gridway-baseapp/src/module_router.rs` | 의존성 해결, IPC, 캐퍼빌리티 기반 라우팅 |

### 끊어진 연결 ❌ (이 플랜이 해결할 것)

| 갭 | 현재 | 필요 |
|----|------|------|
| BaseApp → JMT | `MemStore::new()` (L:180) | `GlobalAppStore::new(JMTStore)` |
| commit() | `[0u8; 32]` (L:773) | `jmt_store.commit()` → real root hash |
| deliver_tx | gas simulation only (L:713) | VFS를 통한 실제 상태 변경 |
| VFS ↔ WASI | VFS 존재하나 WASI에 미연결 | WASI 모듈이 VFS로 상태 접근 |
| init_chain | `Ok(())` (L:812) | genesis → bank store 로딩 |
| Bank 모듈 | BankService 서버 crate에만 존재 | WASM bank 컴포넌트 구현 |

---

## Architecture: Hybrid Microkernel MVP

```
┌─────────────────────────────────────────────────────┐
│                    CometBFT 0.38                     │
│              (grpc://127.0.0.1:26658)                │
└───────────────────────┬─────────────────────────────┘
                        │ ABCI 2.0 (gRPC)
┌───────────────────────▼─────────────────────────────┐
│                   ABCI Server                        │
│              (gridway-server/abci)                    │
└───────────────────────┬─────────────────────────────┘
                        │
┌───────────────────────▼─────────────────────────────┐
│                    BaseApp                            │
│   ┌─────────────────────────────────────────────┐   │
│   │           Module Router                       │   │
│   │   "/cosmos.bank.v1beta1.MsgSend" → bank.wasm │   │
│   └──────────────────┬──────────────────────────┘   │
│                      │ execute(msg)                   │
│   ┌──────────────────▼──────────────────────────┐   │
│   │         ComponentHost (wasmtime)              │   │
│   │   ┌─────────────────────────────────────┐   │   │
│   │   │  bank.wasm (WASI Component)          │   │   │
│   │   │  - handle(MsgSend) → VFS writes      │   │   │
│   │   │  - query(balance) → VFS reads         │   │   │
│   │   └────────────┬────────────────────────┘   │   │
│   └────────────────┼────────────────────────────┘   │
│                    │ VFS file ops                      │
│   ┌────────────────▼────────────────────────────┐   │
│   │     Virtual Filesystem (VFS)                  │   │
│   │   /state/bank/balance_{addr}_{denom}          │   │
│   │   /state/auth/account_{addr}                  │   │
│   └────────────────┬────────────────────────────┘   │
│                    │ namespaced KVStore ops            │
│   ┌────────────────▼────────────────────────────┐   │
│   │   GlobalAppStore (NamespacedStore views)      │   │
│   │   bank/ → prefix "bank/"                      │   │
│   │   auth/ → prefix "auth/"                      │   │
│   └────────────────┬────────────────────────────┘   │
│                    │                                  │
│   ┌────────────────▼────────────────────────────┐   │
│   │       JMTStore (RocksDB backend)              │   │
│   │   commit() → SHA256 root hash                 │   │
│   │   Versioned, persistent, deterministic        │   │
│   └─────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
```

---

## Phase 구조 및 크리티컬 패스

```
Phase 0 ─────────────────── Phase 1A ─────── Phase 1B ──── Phase 2 ──── Phase 3
[JMT+VFS 연결]             [WASM Bank]      [Genesis]     [ABCI]       [TX/Test]
 (5일)                      (5일)            (3일)         (3일)        (4일)
                                                                         
                            ┌─ Phase 1B ─┐                               
Phase 0 ─── Phase 1A ──────┤             ├─── Phase 2 ─── Phase 3       
             ↕ 병렬가능     └─────────────┘                               
             Phase 1B                                                     
```

### Critical Path (최장 경로)
```
Phase 0 → Phase 1A → Phase 2 → Phase 3
  5d         5d         3d        4d    = 17일

Phase 1B는 Phase 0 완료 후 Phase 1A와 병렬 가능
총 예상: 17~20 working days (약 4주)
```

---

## Phase 0: State Foundation — JMT ↔ VFS 연결 (5일)

> **목표:** BaseApp이 JMT-backed GlobalAppStore를 사용하고, VFS가 이를 마운트

### Task 0.1: BaseApp에 JMT 통합 (2일) 🔴 CRITICAL

**현재:** `BaseApp::new()` → `MemStore::new()` × 4개 (auth, bank, staking, gov)

**변경:**
```rust
// crates/gridway-baseapp/src/lib.rs

pub struct BaseApp {
    // 추가
    global_store: Arc<GlobalAppStore>,
    data_dir: PathBuf,
    // ... 기존 필드
}

impl BaseApp {
    pub fn new(name: String, data_dir: Option<PathBuf>) -> Result<Self> {
        let data_dir = data_dir.unwrap_or_else(|| PathBuf::from(".gridway/data"));
        std::fs::create_dir_all(&data_dir)?;
        
        // JMT-backed GlobalAppStore 생성
        let jmt_store = JMTStore::new("state".to_string(), data_dir.join("state.db"))?;
        let global_store = Arc::new(GlobalAppStore::new(jmt_store));
        
        // 네임스페이스 등록
        global_store.register_namespace("bank", false)?;
        global_store.register_namespace("auth", false)?;
        global_store.register_namespace("staking", false)?;
        global_store.register_namespace("gov", false)?;
        
        // ... 기존 초기화 코드
    }
}
```

**파일 변경:**
- `crates/gridway-baseapp/src/lib.rs` — `BaseApp` struct, `new()`, `setup_default_stores()`
- `crates/gridway-baseapp/Cargo.toml` — `gridway-store` 의존성에 `jmt` feature 확인
- `crates/gridway-server/src/bin/gridway-server.rs` — `data_dir` 전달

**테스트:** JMTStore에 값 쓰기 → 앱 재시작 → 값 읽기 확인

### Task 0.2: VFS를 GlobalAppStore에 마운트 (1일) 🔴 CRITICAL

**현재:** VFS는 개별 MemStore를 마운트. GlobalAppStore와 연결 없음.

**변경:**
```rust
fn setup_vfs_stores(vfs: &VirtualFilesystem, global_store: &GlobalAppStore) -> Result<()> {
    // NamespacedStore를 KVStore trait으로 VFS에 마운트
    // 주의: NamespacedStore는 Arc<Mutex<JMTStore>> 공유
    for ns in ["bank", "auth", "staking", "gov"] {
        let ns_store = global_store.get_namespace(ns)?;
        let store_arc: Arc<Mutex<dyn KVStore>> = Arc::new(Mutex::new(ns_store));
        vfs.mount_store(ns.to_string(), store_arc)?;
    }
    Ok(())
}
```

**주의점:**
- `NamespacedStore`는 `KVStore` trait을 이미 구현함 — 바로 사용 가능
- VFS mount 후에는 `/bank/balance_cosmos1abc_ugridway` 같은 경로로 접근 가능
- VFS capabilities 자동 설정 필요 (bank 모듈에 bank namespace write 권한)

**테스트:** VFS.open("/bank/test_key") → write → close → VFS.open → read → GlobalAppStore에서 직접 확인

### Task 0.3: commit() 구현 (1일) 🔴 CRITICAL

**현재:** `Ok(vec![0u8; 32])`

**변경:**
```rust
pub fn commit(&mut self) -> Result<Vec<u8>> {
    let root_hash = {
        let mut store = self.global_store.get_store().lock()
            .map_err(|e| BaseAppError::Store(e.to_string()))?;
        store.commit()
            .map_err(|e| BaseAppError::Store(e.to_string()))?
    };
    
    // height ↔ app_hash 매핑 저장 (복구용)
    let height = self.get_height();
    let height_key = format!("__app_hash_{}", height);
    {
        let mut store = self.global_store.get_store().lock()
            .map_err(|e| BaseAppError::Store(e.to_string()))?;
        store.set(height_key.as_bytes(), &root_hash)?;
    }
    
    log::info!("Committed block {} with app hash: {}", height, hex::encode(&root_hash));
    Ok(root_hash.to_vec())
}
```

**JMTStore.commit()은 이미 구현되어 있음** — pending changes를 flush하고 SHA256 root hash 반환.

**테스트:** 상태 변경 → commit → root hash != [0;32] 확인 / 같은 상태 → 같은 hash 확인

### Task 0.4: VFS-WASI 브릿지 (1일) 🟡 HIGH

**현재:** VFS는 독립적. WASI 모듈이 VFS에 접근할 방법 없음.

**방법 A (MVP 권장): WIT kvstore 인터페이스를 VFS 기반으로 재구현**

현재 `kvstore` WIT 인터페이스가 이미 정의되어 있고, ComponentHost에 주석 처리된 연결 코드가 있음. 이것을 VFS 기반으로 재구현:

```rust
// component_host.rs 내 ComponentState에 VFS 참조 추가
pub struct ComponentState {
    table: wasmtime_wasi::ResourceTable,
    wasi: WasiCtx,
    vfs: Arc<VirtualFilesystem>,  // 추가!
    component_name: String,
}

// kvstore::Host 구현에서 VFS 사용
impl kvstore::Host for ComponentState {
    fn open_store(&mut self, name: String) -> Result<Resource<kvstore::Store>, String> {
        // VFS에 capability 추가 후 fd 열기
        let path = PathBuf::from(format!("/{}", name));
        self.vfs.add_capability(Capability::Read(path.clone()))?;
        self.vfs.add_capability(Capability::Write(path.clone()))?;
        // ... Store resource 생성
    }
}

impl kvstore::HostStore for ComponentState {
    fn get(&mut self, store: Resource<Store>, key: Vec<u8>) -> Option<Vec<u8>> {
        let key_str = String::from_utf8_lossy(&key);
        let path = PathBuf::from(format!("/bank/{}", key_str));
        let fd = self.vfs.open(&path, false).ok()?;
        let mut buf = vec![0u8; 4096];
        let n = self.vfs.read(fd, &mut buf).ok()?;
        self.vfs.close(fd).ok();
        Some(buf[..n].to_vec())
    }
    // set, delete, has, range도 유사하게 VFS 연산으로 구현
}
```

**방법 B (단순화): 직접 NamespacedStore를 WASI에 노출**

VFS를 건너뛰고 GlobalAppStore의 NamespacedStore를 직접 WASI 모듈에 전달. 기능적으로 동일하나 비전과 안 맞음.

**결정: 방법 A 선택** — VFS 경유가 아키텍처 비전에 부합. VFS가 이미 구현되어 있으므로 오버헤드 적음.

**테스트:** WASM 모듈 내에서 `kvstore::open_store("bank")` → `store.set(key, value)` → 호스트에서 JMT로 직접 읽기 확인

---

## Phase 1A: WASM Bank Module (5일)

> **목표:** bank.wasm이 MsgSend를 처리하여 VFS를 통해 잔액을 실제로 변경

### Task 1A.1: Bank WASM Component 생성 (3일) 🔴 CRITICAL

**새 crate:** `crates/wasi-modules/bank/`

WIT `module` 인터페이스를 구현하는 bank 컴포넌트:

```rust
// crates/wasi-modules/bank/src/lib.rs

mod bindings;
use bindings::exports::gridway::framework::module::{
    Guest, ModuleContext, Message, ModuleResponse, Event, EventAttribute,
};
use bindings::gridway::framework::kvstore;

struct BankModule;

impl Guest for BankModule {
    fn handle(context: ModuleContext, msg: Message) -> ModuleResponse {
        match msg.type_url.as_str() {
            "/cosmos.bank.v1beta1.MsgSend" => handle_msg_send(&context, &msg),
            _ => ModuleResponse {
                success: false,
                error: Some(format!("unknown message type: {}", msg.type_url)),
                ..Default::default()
            },
        }
    }
    
    fn query(path: String, data: Vec<u8>) -> Result<Vec<u8>, String> {
        match path.as_str() {
            "/cosmos.bank.v1beta1.Query/Balance" => query_balance(&data),
            _ => Err(format!("unknown query path: {}", path)),
        }
    }
}

fn handle_msg_send(ctx: &ModuleContext, msg: &Message) -> ModuleResponse {
    let send: MsgSendData = serde_json::from_str(&msg.data).unwrap();
    
    // VFS를 통한 상태 접근
    let store = kvstore::open_store("bank").unwrap();
    
    // sender 잔액 확인
    let sender_key = format!("balance_{}_{}", send.from_address, send.denom);
    let sender_balance: u128 = store.get(sender_key.as_bytes())
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    
    if sender_balance < send.amount {
        return ModuleResponse {
            success: false,
            error: Some("insufficient funds".to_string()),
            ..Default::default()
        };
    }
    
    // 차감 & 증가
    let new_sender = sender_balance - send.amount;
    store.set(sender_key.as_bytes(), new_sender.to_string().as_bytes());
    
    let recipient_key = format!("balance_{}_{}", send.to_address, send.denom);
    let recipient_balance: u128 = store.get(recipient_key.as_bytes())
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let new_recipient = recipient_balance + send.amount;
    store.set(recipient_key.as_bytes(), new_recipient.to_string().as_bytes());
    
    // 이벤트 발행
    ModuleResponse {
        success: true,
        events: vec![
            Event {
                event_type: "transfer".to_string(),
                attributes: vec![
                    EventAttribute { key: "sender".to_string(), value: send.from_address },
                    EventAttribute { key: "recipient".to_string(), value: send.to_address },
                    EventAttribute { key: "amount".to_string(), value: format!("{}{}", send.amount, send.denom) },
                ],
            }
        ],
        gas_used: 65000,
        ..Default::default()
    }
}
```

**빌드:** `cargo component build -p bank-wasm --target wasm32-wasip2`

**단순화 가능 지점:**
- 멀티 denom 송금 → 단일 denom만 (ugridway)
- 서명 검증 → MVP에서 건너뜀 (Phase 3에서 추가)
- 수수료 공제 → MVP에서 건너뜀

**반드시 완전 구현:**
- 잔액 확인 (underflow 방지)
- 원자적 차감/증가 (하나가 실패하면 둘 다 취소)
- 이벤트 발행 (블록 익스플로러 호환성)

### Task 1A.2: BaseApp에서 WASM Bank 라우팅 (2일) 🔴 CRITICAL

**현재:** `execute_transaction()`의 wildcard 분기가 "unhandled message type" 반환

**변경:**
```rust
// baseapp/src/lib.rs execute_transaction() 내

"/cosmos.bank.v1beta1.MsgSend" => {
    // WASM bank 컴포넌트로 라우팅
    let bank_wasm = self.load_or_get_component("bank")?;
    
    let module_ctx = module::ModuleContext {
        block_height: height,
        block_time: self.context.as_ref().map(|c| c.block_time).unwrap_or(0),
        chain_id: self.context.as_ref()
            .map(|c| c.chain_id.clone())
            .unwrap_or_default(),
        simulate: false,
    };
    
    let msg = module::Message {
        type_url: type_url.to_string(),
        data: serde_json::to_string(msg_value)?,
        sender: msg_value.get("from_address")
            .and_then(|v| v.as_str())
            .unwrap_or("").to_string(),
    };
    
    let result = self.component_host.execute_module(
        "bank", &module_ctx, &msg, 100_000
    )?;
    
    if !result.success {
        return Err(BaseAppError::TxFailed(
            result.error.unwrap_or("bank execution failed".into())
        ));
    }
    
    total_gas_used += result.gas_used;
    events.extend(result.events);
}
```

**추가 작업:**
- `ComponentHost`에 `execute_module()` 메서드 추가 (WIT module 인터페이스용)
- bank.wasm 로딩 로직 (파일 경로 또는 genesis에서)
- fallback: WASM 로딩 실패 시 native bank 로직으로 대체 (안전장치)

---

## Phase 1B: Genesis & Query (3일, Phase 0 완료 후 시작, 1A와 병렬 가능)

> **목표:** genesis 파일에서 초기 잔액 로딩, 잔액 쿼리 API

### Task 1B.1: init_chain 구현 (2일) 🔴 CRITICAL

**현재:** `Ok(())`

**변경:**
```rust
pub fn init_chain(&mut self, chain_id: String, genesis_bytes: &[u8]) -> Result<()> {
    let genesis: AppGenesis = serde_json::from_slice(genesis_bytes)
        .map_err(|e| BaseAppError::InitChainFailed(e.to_string()))?;
    
    genesis.validate()
        .map_err(|e| BaseAppError::InitChainFailed(e.to_string()))?;
    
    // Bank balances 로딩
    if let Some(bank) = &genesis.app_state.bank {
        for balance in &bank.balances {
            for coin in &balance.coins {
                let key = format!("balance_{}_{}", balance.address, coin.denom);
                self.global_store.set_namespaced("bank", key.as_bytes(), coin.amount.as_bytes())
                    .map_err(|e| BaseAppError::InitChainFailed(e.to_string()))?;
            }
        }
    }
    
    // Auth accounts 로딩
    if let Some(auth) = &genesis.app_state.auth {
        for account in &auth.accounts {
            let key = format!("account_{}", account.address);
            let value = serde_json::to_vec(account)
                .map_err(|e| BaseAppError::InitChainFailed(e.to_string()))?;
            self.global_store.set_namespaced("auth", key.as_bytes(), &value)
                .map_err(|e| BaseAppError::InitChainFailed(e.to_string()))?;
        }
    }
    
    // Chain ID 설정
    self.chain_id = chain_id;
    
    // 첫 commit으로 initial app hash 생성
    let app_hash = self.commit()?;
    log::info!("Genesis initialized with app hash: {}", hex::encode(&app_hash));
    
    Ok(())
}
```

**`gridway init` CLI 업데이트:**
- `AppGenesis::with_default_setup(chain_id)` 호출
- CometBFT genesis.json에 app_state 포함
- data 디렉토리 생성

### Task 1B.2: Balance Query 구현 (1일) 🟡 HIGH

**변경:**
```rust
pub fn query(&self, path: String, data: &[u8], height: u64, _prove: bool) -> Result<QueryResponse> {
    match path.as_str() {
        "/cosmos.bank.v1beta1.Query/Balance" | "bank/balance" => {
            // data에서 address, denom 파싱
            let query: BalanceQuery = serde_json::from_slice(data)
                .map_err(|e| BaseAppError::QueryFailed(e.to_string()))?;
            
            let key = format!("balance_{}_{}", query.address, query.denom);
            let value = self.global_store.get_namespaced("bank", key.as_bytes())
                .map_err(|e| BaseAppError::QueryFailed(e.to_string()))?
                .unwrap_or_else(|| b"0".to_vec());
            
            Ok(QueryResponse {
                code: 0,
                log: "".to_string(),
                value,
                height,
                proof: None,
            })
        }
        _ => Ok(QueryResponse {
            code: 1,
            log: format!("unknown query path: {}", path),
            value: vec![],
            height,
            proof: None,
        })
    }
}
```

---

## Phase 2: ABCI Integration (3일)

> **목표:** CometBFT와 연결, FinalizeBlock 올바르게 동작

### Task 2.1: ABCI Server 검증 및 수정 (2일) 🟡 HIGH

**확인 사항:**
1. `AbciServer::start()` tonic gRPC가 CometBFT 0.38과 호환되는지 테스트
2. `--proxy_app=grpc://127.0.0.1:26658` 으로 CometBFT 연결 테스트
3. `FinalizeBlock` → `begin_block` + 각 tx `deliver_tx` + `end_block` + `commit` → `app_hash` 반환 검증

**알려진 이슈:**
- `finalize_block()`에서 chain_id를 하드코딩 ("gridway-1") — 수정 필요
- ABCI TCP handler (`start_abci_server`)는 placeholder — gRPC만 사용하도록 정리

### Task 2.2: Testnet Setup (1일) 🟡 HIGH

**`scripts/setup-testnet.sh` 업데이트:**
```bash
#!/bin/bash
# 2-node testnet setup

# Node 1
gridway init --chain-id gridway-testnet-1 --home node1
# Node 2  
gridway init --chain-id gridway-testnet-1 --home node2

# Genesis with balances
# 공유 genesis 생성 (두 노드 동일)
# Validator keys 생성
# persistent_peers 설정
# docker-compose.multi.yml 업데이트
```

---

## Phase 3: Transaction Submission & E2E Test (4일)

> **목표:** 실제 토큰 송금을 두 노드에서 확인

### Task 3.1: TX Building & Broadcasting (2일) 🟡 HIGH

**MVP 단순화: 서명 없는 JSON TX 제출**

```bash
# CLI 또는 REST로 TX 제출
gridway tx bank send \
  cosmos1alice... cosmos1bob... 100ugridway \
  --chain-id gridway-testnet-1

# 내부 동작:
# 1. MsgSend JSON 생성
# 2. CometBFT RPC broadcast_tx_commit 호출
# 3. 결과 출력
```

**구현:**
- `gridway-client/src/tx_builder.rs` — 기존 scaffolding 활용
- `gridway-server/src/bin/gridway-server.rs` — `tx` 서브커맨드 추가
- CometBFT RPC 연결 (`/broadcast_tx_commit`)

**단순화:**
- 서명 검증 건너뜀 (ante handler에서 bypass)
- nonce/sequence 검증 건너뜀
- 수수료 0 허용

### Task 3.2: E2E Integration Test (2일) 🟡 HIGH

**테스트 시나리오:**
```
1. genesis: alice=1000000ugridway, bob=0ugridway
2. gridway tx bank send alice bob 100ugridway
3. query alice balance → 999900ugridway
4. query bob balance → 100ugridway
5. 두 번째 노드에서도 같은 잔액 확인
6. commit → app hash 두 노드 일치 확인
```

**자동화:**
```bash
# tests/e2e/two_node_transfer.sh
docker compose -f docker-compose.multi.yml up -d
sleep 10  # 노드 시작 대기

# 초기 잔액 확인
assert_balance node1 alice 1000000
assert_balance node2 alice 1000000

# 송금
send_tx node1 alice bob 100ugridway

# 최종 잔액 확인
sleep 5  # 블록 확인 대기
assert_balance node1 alice 999900
assert_balance node1 bob 100
assert_balance node2 alice 999900  # 합의 확인
assert_balance node2 bob 100
```

---

## 단순화 vs 완전 구현 매트릭스

| 항목 | MVP 전략 | 이유 |
|------|----------|------|
| **JMT 연결** | ✅ 완전 구현 | 영속성 없이는 체인 불가 |
| **commit() root hash** | ✅ 완전 구현 | 합의의 핵심 |
| **Bank 잔액 변경** | ✅ 완전 구현 | 목표의 핵심 |
| **VFS ↔ WASI 브릿지** | ✅ 완전 구현 | 마이크로커널 비전 핵심 |
| **Bank WASM 컴포넌트** | ✅ 완전 구현 | 마이크로커널 비전 핵심 |
| **Genesis 로딩** | ✅ 완전 구현 | 초기 상태 없이 테스트 불가 |
| **서명 검증** | ⚡ 건너뜀 | MVP에서 trust 가정 |
| **수수료 처리** | ⚡ 건너뜀 | 잔액 변경과 무관 |
| **Staking/Gov 모듈** | ⚡ 건너뜀 | bank만 필요 |
| **State Sync** | ⚡ 건너뜀 | 2노드 테스트넷에 불필요 |
| **머클 트리 컴포넌트 저장** | ⚡ 단순화 | 파일시스템에서 로딩 (비전은 머클트리지만 MVP에선 불필요) |
| **FD 기반 캐퍼빌리티** | ⚡ 단순화 | 현재 Path 기반으로 동작. 보안 강화는 후속 |
| **Multi-denom 송금** | ⚡ 단순화 | 단일 denom (ugridway)만 |
| **gRPC query 서비스** | ⚡ 단순화 | 직접 state query만 |
| **Vote Extensions** | ⚡ 건너뜀 | CometBFT 기본값 사용 |

---

## 리스크 및 완화

| 리스크 | 가능성 | 영향 | 완화 |
|--------|--------|------|------|
| **WASM bank 모듈 성능** | Low | Medium | fuel 미터링으로 가스 제한. 벤치마크 후 최적화 |
| **VFS-WASI 연결 복잡도** | Medium | High | VFS 직접 사용 대신 NamespacedStore 직접 노출하는 fallback 준비 |
| **CometBFT ABCI 프로토콜 불일치** | Medium | High | Phase 2 초반에 단독 CometBFT 연결 테스트 |
| **JMT root hash 비결정성** | Low | Critical | 키 정렬 순서 보장, SHA256 사용. 테스트로 검증 |
| **wasmtime 버전 호환성** | Low | Medium | Cargo.lock 고정, CI에서 빌드 확인 |
| **WASM 컴포넌트 빌드 실패** | Medium | High | native fallback 경로 유지. cargo-component 설치 필수 |

---

## 의존성 및 도구 요구사항

### 빌드 도구
- `rustc` nightly (wasm32-wasip2 target)
- `cargo-component` (WASM 컴포넌트 빌드)
- `wasm-tools` (컴포넌트 조합)
- CometBFT 0.38.x

### Crate 의존성 (추가 필요)
- `gridway-baseapp` → `gridway-store` (JMT + GlobalAppStore)
- `gridway-baseapp` → `hex` (app hash 로깅)

### 환경 설정
```bash
# WASM 타겟 추가
rustup target add wasm32-wasip2
# cargo-component 설치
cargo install cargo-component
# CometBFT 설치
wget https://github.com/cometbft/cometbft/releases/download/v0.38.x/cometbft_0.38.x_linux_amd64.tar.gz
```

---

## 파일 변경 요약

| 파일 | Phase | 변경 유형 |
|------|-------|-----------|
| `crates/gridway-baseapp/src/lib.rs` | 0, 1A, 1B | JMT 통합, commit(), deliver_tx, init_chain, query |
| `crates/gridway-baseapp/Cargo.toml` | 0 | gridway-store 의존성 추가 |
| `crates/gridway-baseapp/src/component_host.rs` | 0, 1A | VFS 연결, execute_module() 추가 |
| `crates/gridway-baseapp/src/vfs.rs` | 0 | GlobalAppStore 마운트 연결 |
| `crates/wasi-modules/bank/` | 1A | **새로 생성** — WASM bank 컴포넌트 |
| `wit/module.wit` | 1A | 필요시 확장 (query 인터페이스) |
| `crates/gridway-server/src/bin/gridway-server.rs` | 1B, 2 | data_dir, init 커맨드, ABCI 수정 |
| `crates/gridway-server/src/abci_server.rs` | 2 | gRPC ABCI 검증 |
| `crates/gridway-client/src/tx_builder.rs` | 3 | TX 빌딩 구현 |
| `scripts/setup-testnet.sh` | 2 | 테스트넷 스크립트 |
| `docker-compose.multi.yml` | 2 | ABCI 프로토콜 업데이트 |
| `tests/e2e/` | 3 | **새로 생성** — E2E 테스트 |

---

## 타임라인

```
Week 1: Phase 0 (5d)
  Mon-Tue: Task 0.1 — JMT 통합
  Wed:     Task 0.2 — VFS 마운트
  Thu:     Task 0.3 — commit()
  Fri:     Task 0.4 — VFS-WASI 브릿지

Week 2: Phase 1A + 1B (병렬, 5d + 3d)
  Mon-Wed: Task 1A.1 — Bank WASM 컴포넌트
  Mon-Tue: Task 1B.1 — init_chain (병렬)
  Thu-Fri: Task 1A.2 — BaseApp 라우팅
  Wed:     Task 1B.2 — Balance Query (병렬)

Week 3: Phase 2 + 3 (3d + 4d)
  Mon-Tue: Task 2.1 — ABCI 검증
  Wed:     Task 2.2 — Testnet Setup
  Thu-Fri: Task 3.1 — TX Building

Week 4: Phase 3 완료 + 버퍼
  Mon-Tue: Task 3.2 — E2E Test
  Wed-Fri: 버퍼 / 버그 수정 / 문서화
```

**총 소요: 17~20 working days (약 4주)**  
병렬 작업 최적화 시 3주 가능. 예상치 못한 이슈 감안하여 4주 권장.

---

## 마일스톤

### M1: 영속 상태 (Week 1 완료)
- [ ] BaseApp이 JMTStore 사용
- [ ] commit()이 실제 root hash 반환
- [ ] VFS가 GlobalAppStore에 마운트
- [ ] WASI 모듈이 VFS를 통해 상태 접근

### M2: WASM 토큰 송금 (Week 2 완료)
- [ ] bank.wasm이 MsgSend 처리
- [ ] genesis에서 초기 잔액 로딩
- [ ] 잔액 쿼리 동작
- [ ] 단일 노드에서 송금 → 잔액 변경 확인

### M3: 두 노드 합의 (Week 3 중반)
- [ ] CometBFT 연결 동작
- [ ] 두 노드 같은 genesis로 시작
- [ ] 블록 생산, app hash 일치

### M4: E2E 토큰 송금 (Week 4 초반)
- [ ] CLI로 TX 제출
- [ ] 두 노드에서 잔액 변경 확인
- [ ] 자동화된 E2E 테스트 통과

---

## Post-MVP Roadmap (참고)

MVP 이후 아키텍처 비전 완성을 위한 경로:

1. **머클 트리 컴포넌트 저장** — WASM 바이너리를 JMT `/sbin/`, `/bin/` 경로에 저장, 거버넌스로 업그레이드
2. **FD 기반 캐퍼빌리티** — 현재 Path 기반 → unforgeable file descriptor 기반으로 전환
3. **서명 검증** — ed25519/secp256k1 검증을 ante handler에서 활성화
4. **IBC 모듈** — WASM 컴포넌트로 IBC 핸들러 구현
5. **Staking/Gov** — 거버넌스 모듈을 WASM으로, 컴포넌트 업그레이드 투표
6. **State Sync** — JMT 스냅샷 지원
7. **성능 최적화** — 병렬 실행, WASM AOT 컴파일, 캐싱
