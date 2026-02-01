//! Base application framework for gridway.
//!
//! Acts as a WASM microkernel host — all blockchain logic runs inside
//! WASM modules that interact with state through the Virtual Filesystem (VFS).
//!
//! This version has been adapted for the Commonware Library migration:
//! - ABCI-specific code (begin_block/end_block/deliver_tx flow) removed
//! - Simplified to `execute_block(txs) -> StateRoot` for consensus integration
//! - VFS, ComponentHost, WASI bridge preserved intact

pub mod capabilities;
pub mod component_bindings;
pub mod component_host;
pub mod module_governance;
pub mod module_router;
pub mod vfs;
pub mod wasi_host;

use gridway_store::{GlobalAppStore, MerkleStore, KVStore};
#[cfg(test)]
use gridway_store::MemStore;
use gridway_types::{Event, EventAttribute, TxResponse};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;

// Import microkernel components
use crate::capabilities::CapabilityManager;
use crate::component_host::{ComponentHost, ComponentInfo, ComponentType};
use crate::module_governance::ModuleGovernance;
use crate::module_router::{ExecutionContext, ModuleRouter};
use crate::vfs::VirtualFilesystem;
use crate::wasi_host::WasiHost;

pub use module_governance::{
    CodeMetadata, ModuleInstallConfig, MsgInstallModule, MsgStoreCode, MsgUpgradeModule,
};

/// BaseApp errors
#[derive(Error, Debug)]
pub enum BaseAppError {
    #[error("invalid transaction: {0}")]
    InvalidTx(String),

    #[error("transaction execution failed: {0}")]
    TxFailed(String),

    #[error("invalid block: {0}")]
    InvalidBlock(String),

    #[error("store error: {0}")]
    Store(String),

    #[error("execution error: {0}")]
    ExecutionError(String),

    #[error("query failed: {0}")]
    QueryFailed(String),

    #[error("chain initialization failed: {0}")]
    InitChainFailed(String),
}

/// Result type alias
pub type Result<T> = std::result::Result<T, BaseAppError>;

/// Execution context for the current block
#[derive(Debug, Clone)]
pub struct BlockContext {
    pub height: u64,
    pub timestamp: u64,
    pub chain_id: String,
}

/// Base application — acts as microkernel host for WASM modules.
///
/// Simplified for Commonware integration:
/// - No ABCI begin_block/end_block lifecycle
/// - Single `execute_block()` entry point for consensus
/// - State committed via `commit()` returning Merkle root
pub struct BaseApp {
    /// Application name
    name: String,
    /// Current block context
    context: Option<BlockContext>,
    /// WASI runtime host for module execution
    #[allow(dead_code)]
    wasi_host: Arc<WasiHost>,
    /// Component host for preview2 components
    component_host: Arc<ComponentHost>,
    /// Virtual filesystem for state access
    vfs: Arc<VirtualFilesystem>,
    /// Module router for message dispatch
    module_router: Arc<ModuleRouter>,
    /// Capability manager for module security
    #[allow(dead_code)]
    capability_manager: Arc<CapabilityManager>,
    /// Module governance for WASM module lifecycle
    module_governance: Arc<ModuleGovernance>,
    /// Module paths for WASI modules
    module_paths: HashMap<String, String>,
    /// Global application store (MerkleStore backend)
    global_store: Arc<GlobalAppStore>,
    /// Last committed state root hash
    last_state_root: [u8; 32],
    /// Block heights already executed (for idempotency across propose/verify/report)
    executed_heights: HashSet<u64>,
}

impl BaseApp {
    /// Create a new base application with microkernel architecture
    pub fn new(name: String) -> Result<Self> {
        // Create MerkleStore backend (replaces JMT+RocksDB)
        let merkle_store = MerkleStore::new("state".to_string());
        let global_store = Arc::new(GlobalAppStore::new(merkle_store));

        // Register namespaces for core modules
        for ns in ["bank", "auth", "staking", "gov"] {
            global_store.register_namespace(ns, false)
                .map_err(|e| BaseAppError::Store(format!("Failed to register {ns} namespace: {e}")))?;
        }

        // Initialize WASI runtime host
        let wasi_host = Arc::new(WasiHost::new().map_err(|e| {
            BaseAppError::InitChainFailed(format!("Failed to initialize WASI host: {e}"))
        })?);

        // Initialize component host for preview2 components
        let mut component_host_inner = ComponentHost::new().map_err(|e| {
            BaseAppError::InitChainFailed(format!("Failed to initialize Component host: {e}"))
        })?;

        // Initialize virtual filesystem
        let vfs = Arc::new(VirtualFilesystem::new());

        // Mount stores into VFS
        Self::setup_stores(&vfs, &global_store)?;

        // Bridge VFS to ComponentHost
        component_host_inner.set_vfs(vfs.clone());
        let component_host = Arc::new(component_host_inner);

        // Initialize capability manager
        let capability_manager = Arc::new(CapabilityManager::new());

        // Initialize module router
        let module_router = Arc::new(ModuleRouter::new(wasi_host.clone(), vfs.clone()));

        // Initialize module governance
        let governance_authority = "gridway_governance".to_string();
        let module_governance = Arc::new(ModuleGovernance::new(
            module_router.clone(),
            vfs.clone(),
            governance_authority,
        ));

        // Initialize module paths
        let module_base_path = Self::find_module_base_path();
        let mut module_paths = HashMap::new();
        for name in ["begin_blocker", "end_blocker", "tx_decoder", "ante_handler", "bank"] {
            module_paths.insert(
                name.to_string(),
                format!("{module_base_path}/{name}_component.wasm"),
            );
        }

        Ok(Self {
            name,
            context: None,
            wasi_host,
            component_host,
            vfs,
            module_router,
            capability_manager,
            module_governance,
            module_paths,
            global_store,
            last_state_root: [0u8; 32],
            executed_heights: HashSet::new(),
        })
    }

    /// Get a reference to the module governance
    pub fn module_governance(&self) -> &Arc<ModuleGovernance> {
        &self.module_governance
    }

    /// Get a reference to the module router
    pub fn module_router(&self) -> &Arc<ModuleRouter> {
        &self.module_router
    }

    /// Get a reference to the global app store
    pub fn global_store(&self) -> &Arc<GlobalAppStore> {
        &self.global_store
    }

    /// Get a reference to the VFS
    pub fn vfs(&self) -> &Arc<VirtualFilesystem> {
        &self.vfs
    }

    /// Find the module base path
    fn find_module_base_path() -> String {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let modules_in_current = current_dir.join("modules");
        if modules_in_current.exists() {
            return "modules".to_string();
        }

        let mut path = current_dir.clone();
        for _ in 0..3 {
            if let Some(parent) = path.parent() {
                let modules_path = parent.join("modules");
                if modules_path.exists() {
                    return modules_path.to_string_lossy().to_string();
                }
                path = parent.to_path_buf();
            }
        }

        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let manifest_path = PathBuf::from(manifest_dir);
            if let Some(parent) = manifest_path.parent() {
                if let Some(grandparent) = parent.parent() {
                    let modules_path = grandparent.join("modules");
                    if modules_path.exists() {
                        return modules_path.to_string_lossy().to_string();
                    }
                }
            }
        }

        "modules".to_string()
    }

    /// Set up stores in VFS for blockchain modules
    fn setup_stores(vfs: &Arc<VirtualFilesystem>, global_store: &Arc<GlobalAppStore>) -> Result<()> {
        use crate::vfs::Capability;

        for ns in ["auth", "bank", "staking", "gov"] {
            let ns_store = global_store.get_namespace(ns)
                .map_err(|e| BaseAppError::Store(format!("Failed to get {ns} namespace: {e}")))?;
            let store_arc: Arc<std::sync::Mutex<dyn KVStore>> =
                Arc::new(std::sync::Mutex::new(ns_store));
            vfs.mount_store(ns.to_string(), store_arc)
                .map_err(|e| BaseAppError::Store(format!("Failed to mount {ns} store: {e}")))?;

            let ns_path = PathBuf::from(format!("/{ns}"));
            vfs.add_capability(Capability::Read(ns_path.clone()))
                .map_err(|e| BaseAppError::Store(format!("Failed to add read cap for {ns}: {e}")))?;
            vfs.add_capability(Capability::Write(ns_path))
                .map_err(|e| BaseAppError::Store(format!("Failed to add write cap for {ns}: {e}")))?;
        }

        Ok(())
    }

    // =========================================================================
    // Consensus interface — simplified for Commonware integration
    // =========================================================================

    /// Execute a block of transactions and return the state root.
    ///
    /// This is the primary entry point for consensus:
    /// 1. Set block context
    /// 2. Execute each transaction through WASM modules
    /// 3. Return (state_root, tx_responses)
    pub fn execute_block(
        &mut self,
        height: u64,
        timestamp: u64,
        chain_id: &str,
        txs: &[Vec<u8>],
    ) -> Result<([u8; 32], Vec<TxResponse>)> {
        // Idempotency: if this block height was already executed, return current state root
        // without re-applying transactions. This is critical because consensus calls
        // execute_block multiple times (propose, verify, report) for the same block.
        if self.executed_heights.contains(&height) {
            let state_root = {
                let store_arc = self.global_store.get_store();
                let store = store_arc.lock()
                    .map_err(|e| BaseAppError::Store(format!("Failed to lock store: {e}")))?;
                store.root_hash()
            };
            return Ok((state_root, Vec::new()));
        }

        self.context = Some(BlockContext {
            height,
            timestamp,
            chain_id: chain_id.to_string(),
        });

        let mut responses = Vec::with_capacity(txs.len());

        for (i, tx_bytes) in txs.iter().enumerate() {
            match self.execute_transaction(tx_bytes, height) {
                Ok(response) => responses.push(response),
                Err(e) => {
                    responses.push(TxResponse::failure(
                        1,
                        format!("Transaction {i} failed: {e}"),
                        0,
                        0,
                    ));
                }
            }
        }

        // Compute state root (without committing)
        let state_root = {
            let store_arc = self.global_store.get_store();
            let store = store_arc.lock()
                .map_err(|e| BaseAppError::Store(format!("Failed to lock store: {e}")))?;
            store.root_hash()
        };

        self.executed_heights.insert(height);
        self.context = None;
        Ok((state_root, responses))
    }

    /// Commit the current state — finalize pending writes and return state root.
    pub fn commit(&mut self) -> Result<[u8; 32]> {
        let root_hash = {
            let store_arc = self.global_store.get_store();
            let mut store = store_arc.lock()
                .map_err(|e| BaseAppError::Store(format!("Failed to lock store: {e}")))?;
            store.commit()
                .map_err(|e| BaseAppError::Store(format!("Commit failed: {e}")))?
        };

        self.last_state_root = root_hash;
        log::info!(
            "Committed state with root: {}",
            hex::encode(&root_hash)
        );
        Ok(root_hash)
    }

    /// Get last committed state root
    pub fn last_state_root(&self) -> &[u8; 32] {
        &self.last_state_root
    }

    /// Execute a single transaction
    pub fn execute_transaction(&mut self, tx_bytes: &[u8], height: u64) -> Result<TxResponse> {
        // Try to decode as JSON transaction
        let decoded_tx: serde_json::Value = match serde_json::from_slice(tx_bytes) {
            Ok(v) => v,
            Err(_) => {
                // Try WASM tx decoder if JSON fails
                return self.execute_transaction_via_wasm(tx_bytes, height);
            }
        };

        let messages = decoded_tx
            .get("body")
            .and_then(|b| b.get("messages"))
            .and_then(|m| m.as_array())
            .ok_or_else(|| BaseAppError::InvalidTx("no messages in transaction".to_string()))?;

        let mut total_gas_used = 0u64;
        let mut events = Vec::new();

        for (idx, msg_value) in messages.iter().enumerate() {
            let type_url = msg_value
                .get("@type")
                .and_then(|t| t.as_str())
                .ok_or_else(|| BaseAppError::InvalidTx(format!("message {idx} missing @type")))?;

            match type_url {
                "/cosmos.bank.v1beta1.MsgSend" | "/gridway.bank.v1.MsgSend" => {
                    // Route to bank WASM module
                    self.execute_bank_msg(msg_value, height, &mut total_gas_used, &mut events)?;
                }
                "/gridway.baseapp.v1.MsgStoreCode" => {
                    let msg: MsgStoreCode = serde_json::from_value(msg_value.clone())
                        .map_err(|e| BaseAppError::InvalidTx(format!("decode MsgStoreCode: {e}")))?;
                    match self.module_governance.handle_store_code(msg) {
                        Ok(code_id) => {
                            events.push(Event::new("store_code", vec![
                                EventAttribute::new("code_id", code_id.to_string()),
                            ]));
                            total_gas_used += 50000;
                        }
                        Err(e) => {
                            return Ok(TxResponse::failure(1, format!("store_code failed: {e}"), 0, total_gas_used as i64));
                        }
                    }
                }
                "/gridway.baseapp.v1.MsgInstallModule" => {
                    let msg: MsgInstallModule = serde_json::from_value(msg_value.clone())
                        .map_err(|e| BaseAppError::InvalidTx(format!("decode MsgInstallModule: {e}")))?;
                    match self.module_governance.handle_install_module(msg.clone()) {
                        Ok(_) => {
                            events.push(Event::new("install_module", vec![
                                EventAttribute::new("module_name", &msg.config.name),
                                EventAttribute::new("code_id", msg.code_id.to_string()),
                            ]));
                            total_gas_used += 100000;
                        }
                        Err(e) => {
                            return Ok(TxResponse::failure(1, format!("install_module failed: {e}"), 0, total_gas_used as i64));
                        }
                    }
                }
                "/gridway.baseapp.v1.MsgUpgradeModule" => {
                    let msg: MsgUpgradeModule = serde_json::from_value(msg_value.clone())
                        .map_err(|e| BaseAppError::InvalidTx(format!("decode MsgUpgradeModule: {e}")))?;
                    match self.module_governance.handle_upgrade_module(msg.clone()) {
                        Ok(_) => {
                            events.push(Event::new("upgrade_module", vec![
                                EventAttribute::new("module_name", &msg.module_name),
                                EventAttribute::new("new_code_id", msg.new_code_id.to_string()),
                            ]));
                            total_gas_used += 150000;
                        }
                        Err(e) => {
                            return Ok(TxResponse::failure(1, format!("upgrade_module failed: {e}"), 0, total_gas_used as i64));
                        }
                    }
                }
                _ => {
                    return Ok(TxResponse::failure(
                        1,
                        format!("unhandled message type: {type_url}"),
                        0,
                        total_gas_used as i64,
                    ));
                }
            }
        }

        Ok(TxResponse::success(
            "transaction executed successfully".to_string(),
            (total_gas_used + 10000) as i64,
            total_gas_used as i64,
            events,
        ))
    }

    /// Execute a bank message — tries WASM module first, falls back to native execution.
    fn execute_bank_msg(
        &self,
        msg_value: &serde_json::Value,
        height: u64,
        total_gas_used: &mut u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        // Try WASM execution first
        if let Some(bank_path) = self.module_paths.get("bank") {
            if let Ok(component_bytes) = std::fs::read(bank_path) {
                let info = ComponentInfo {
                    name: "bank".to_string(),
                    path: bank_path.clone().into(),
                    component_type: ComponentType::BeginBlocker,
                    gas_limit: 1_000_000,
                };
                let _ = self.component_host.load_component("bank", &component_bytes, info);

                let type_url = msg_value.get("@type").and_then(|t| t.as_str()).unwrap_or("");
                let msg_data = serde_json::to_string(msg_value)
                    .map_err(|e| BaseAppError::InvalidTx(format!("serialize MsgSend: {e}")))?;
                let sender = msg_value.get("from_address")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let timestamp = self.context.as_ref().map(|c| c.timestamp).unwrap_or(0);
                let chain_id = self.context.as_ref().map(|c| c.chain_id.as_str()).unwrap_or("gridway-1");

                let result = self.component_host.execute_module(
                    "bank", height, timestamp, chain_id,
                    type_url, &msg_data, sender, 100_000,
                );

                match result {
                    Ok(comp_result) if comp_result.success => {
                        *total_gas_used += comp_result.gas_used;
                        if let Some(data) = &comp_result.data {
                            if let Some(evt_array) = data.get("events").and_then(|e| e.as_array()) {
                                for evt in evt_array {
                                    events.push(Event {
                                        r#type: evt.get("event_type").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                                        attributes: evt.get("attributes").and_then(|a| a.as_array()).map(|attrs| {
                                            attrs.iter().map(|a| EventAttribute {
                                                key: a.get("key").and_then(|k| k.as_str()).unwrap_or("").to_string(),
                                                value: a.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                                index: true,
                                            }).collect()
                                        }).unwrap_or_default(),
                                    });
                                }
                            }
                        }
                        return Ok(());
                    }
                    _ => {
                        // WASM execution failed, fall through to native
                        log::info!("Bank WASM module execution failed, using native fallback");
                    }
                }
            }
        }

        // Native bank execution fallback
        self.execute_native_bank_msg(msg_value, total_gas_used, events)
    }

    /// Native bank message execution — handles MsgSend without WASM.
    ///
    /// Operates directly on the GlobalAppStore (which goes through the
    /// Patricia Merkle Trie), ensuring state root changes are tracked correctly.
    fn execute_native_bank_msg(
        &self,
        msg_value: &serde_json::Value,
        total_gas_used: &mut u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let from = msg_value.get("from_address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BaseAppError::InvalidTx("missing from_address".to_string()))?;
        let to = msg_value.get("to_address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BaseAppError::InvalidTx("missing to_address".to_string()))?;
        let amounts = msg_value.get("amount")
            .and_then(|v| v.as_array())
            .ok_or_else(|| BaseAppError::InvalidTx("missing amount array".to_string()))?;

        for coin in amounts {
            let denom = coin.get("denom")
                .and_then(|v| v.as_str())
                .ok_or_else(|| BaseAppError::InvalidTx("missing denom in coin".to_string()))?;
            let amount_str = coin.get("amount")
                .and_then(|v| v.as_str())
                .ok_or_else(|| BaseAppError::InvalidTx("missing amount in coin".to_string()))?;
            let amount: u64 = amount_str.parse()
                .map_err(|e| BaseAppError::InvalidTx(format!("invalid amount '{amount_str}': {e}")))?;

            if amount == 0 {
                continue;
            }

            // Read sender balance via GlobalAppStore (goes through Merkle trie)
            let sender_key = format!("balance_{from}_{denom}");
            let sender_bal: u64 = match self.global_store.get_namespaced("bank", sender_key.as_bytes()) {
                Ok(Some(v)) => String::from_utf8(v)
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
                _ => 0,
            };

            if sender_bal < amount {
                return Err(BaseAppError::TxFailed(format!(
                    "insufficient funds: {} has {} {}, need {}",
                    from, sender_bal, denom, amount
                )));
            }

            // Read recipient balance
            let recv_key = format!("balance_{to}_{denom}");
            let recv_bal: u64 = match self.global_store.get_namespaced("bank", recv_key.as_bytes()) {
                Ok(Some(v)) => String::from_utf8(v)
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
                _ => 0,
            };

            // Update balances (writes go through GlobalAppStore → Merkle trie)
            let new_sender_bal = sender_bal - amount;
            let new_recv_bal = recv_bal + amount;

            self.global_store.set_namespaced("bank", sender_key.as_bytes(), new_sender_bal.to_string().as_bytes())
                .map_err(|e| BaseAppError::Store(format!("Failed to update sender balance: {e}")))?;
            self.global_store.set_namespaced("bank", recv_key.as_bytes(), new_recv_bal.to_string().as_bytes())
                .map_err(|e| BaseAppError::Store(format!("Failed to update recipient balance: {e}")))?;

            log::info!(
                "Native bank transfer: {} -> {} ({} {}), sender: {} -> {}, recipient: {} -> {}",
                from, to, amount, denom,
                sender_bal, new_sender_bal,
                recv_bal, new_recv_bal
            );
        }

        // Emit transfer event
        let amount_json = serde_json::to_string(&msg_value["amount"]).unwrap_or_default();
        events.push(Event::new("transfer", vec![
            EventAttribute::new("sender", from),
            EventAttribute::new("recipient", to),
            EventAttribute::new("amount", &amount_json),
        ]));

        *total_gas_used += 10_000;
        Ok(())
    }

    /// Fallback: execute transaction via WASM tx decoder
    fn execute_transaction_via_wasm(&mut self, tx_bytes: &[u8], _height: u64) -> Result<TxResponse> {
        // If tx-decoder WASM is not available, return placeholder
        Ok(TxResponse::success(
            "placeholder: tx decoder not loaded".to_string(),
            200000, 0, vec![],
        ))
    }

    // =========================================================================
    // Helper methods (kept for testing and direct state access)
    // =========================================================================

    /// Set balance directly in bank store (for testing/genesis)
    pub fn set_balance(&mut self, address: &str, denom: &str, amount: u64) -> Result<()> {
        let key = format!("balance_{address}_{denom}");
        let value = amount.to_string();
        self.global_store.set_namespaced("bank", key.as_bytes(), value.as_bytes())
            .map_err(|e| BaseAppError::Store(format!("Failed to set balance: {e}")))?;
        Ok(())
    }

    /// Get balance from bank store
    pub fn get_balance(&self, address: &str, denom: &str) -> Result<u64> {
        let key = format!("balance_{address}_{denom}");
        match self.global_store.get_namespaced("bank", key.as_bytes()) {
            Ok(Some(value)) => {
                let amount_str = String::from_utf8(value)
                    .map_err(|e| BaseAppError::Store(format!("Invalid balance encoding: {e}")))?;
                let amount: u64 = amount_str.parse()
                    .map_err(|e| BaseAppError::Store(format!("Invalid balance value: {e}")))?;
                Ok(amount)
            }
            Ok(None) => Ok(0),
            Err(e) => Err(BaseAppError::Store(format!("Failed to get balance: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_base_app() {
        let app = BaseApp::new("test-app".to_string()).unwrap();
        assert_eq!(app.name, "test-app");
        assert!(app.context.is_none());
    }

    #[test]
    fn test_execute_block_empty() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        let (state_root, responses) = app.execute_block(1, 1234567890, "test-chain", &[]).unwrap();
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn test_commit() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        let hash = app.commit().unwrap();
        assert_eq!(hash.len(), 32);

        app.set_balance("alice", "ugridway", 1000).unwrap();
        let hash2 = app.commit().unwrap();
        assert_ne!(hash2, [0u8; 32]);
    }

    #[test]
    fn test_balance_helpers() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        app.set_balance("alice", "uatom", 1000).unwrap();
        let balance = app.get_balance("alice", "uatom").unwrap();
        assert_eq!(balance, 1000);
    }

    #[test]
    fn test_vfs_jmt_end_to_end() {
        let app = BaseApp::new("test-vfs-e2e".to_string()).unwrap();
        app.vfs.write_key("bank", b"balance_alice_ugridway", b"1000").unwrap();
        let alice = app.vfs.read_key("bank", b"balance_alice_ugridway").unwrap();
        assert_eq!(alice, Some(b"1000".to_vec()));
    }

    #[test]
    fn test_vfs_namespace_isolation() {
        let app = BaseApp::new("test-ns".to_string()).unwrap();
        app.vfs.write_key("bank", b"key1", b"bank_value").unwrap();
        app.vfs.write_key("auth", b"key1", b"auth_value").unwrap();
        assert_eq!(app.vfs.read_key("bank", b"key1").unwrap(), Some(b"bank_value".to_vec()));
        assert_eq!(app.vfs.read_key("auth", b"key1").unwrap(), Some(b"auth_value".to_vec()));
    }

    #[test]
    fn test_deterministic_hash() {
        let run = || {
            let mut app = BaseApp::new("test-det".to_string()).unwrap();
            app.vfs.write_key("bank", b"balance_alice", b"1000").unwrap();
            app.vfs.write_key("bank", b"balance_bob", b"2000").unwrap();
            app.commit().unwrap()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn test_module_governance_integration() {
        let app = BaseApp::new("test-app".to_string()).unwrap();
        let governance = app.module_governance();
        let modules = governance.list_modules().unwrap();
        assert_eq!(modules.len(), 0);
    }
}

/// Execution mode (kept for compatibility with module_router)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecMode {
    Check,
    ReCheck,
    Simulate,
    PrepareProposal,
    ProcessProposal,
    VoteExtension,
    VerifyVoteExtension,
    Finalize,
}
