//! Base application framework for gridway.
//!
//! Acts as a WASM microkernel host — **all** blockchain logic runs inside
//! WASM modules that interact with state through the Virtual Filesystem (VFS).
//!
//! Pipeline (fully WASM, no Rust-native fallback):
//!   1. hook.pre_execute(block_ctx)              → WASM hook module via ComponentHost
//!   2. for each tx:
//!      a. validator.validate(tx_ctx, raw_bytes) → WASM validator module via ComponentHost
//!      b. for each msg: module.handle(ctx, msg) → WASM domain modules via ComponentHost
//!   3. hook.post_execute(block_ctx, stats)       → WASM hook module via ComponentHost
//!   4. commit → state_root
//!
//! All state access: WASM module → kvstore WIT interface → VFS → MerkleStore

pub mod capabilities;
pub mod component_bindings;
pub mod component_host;
pub mod module_governance;
pub mod module_router;
pub mod vfs;
pub mod wasi_host;

use gridway_store::{GlobalAppStore, MerkleStore, KVStore};
use gridway_types::{Event, EventAttribute, TxResponse};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;

// Import microkernel components
use crate::capabilities::CapabilityManager;
use crate::component_host::{ComponentHost, ComponentInfo, ComponentType};
use crate::module_governance::ModuleGovernance;
use crate::module_router::ModuleRouter;
use crate::vfs::VirtualFilesystem;
use crate::wasi_host::WasiHost;

pub use module_governance::{
    CodeMetadata, ModuleInstallConfig, MsgInstallModule, MsgStoreCode, MsgUpgradeModule,
};

// ─── WIT-aligned types ───────────────────────────────────────────────────────

/// A validated and decoded transaction, ready for message dispatch.
/// Matches the `validated-tx` record in `validator.wit`.
#[derive(Debug, Clone)]
pub struct ValidatedTx {
    pub sender: String,
    pub messages: Vec<TxMessage>,
    pub sequence: u64,
    pub gas_limit: u64,
}

/// A message extracted from a validated transaction.
/// Matches the `message` record in `validator.wit`.
#[derive(Debug, Clone)]
pub struct TxMessage {
    pub type_url: String,
    pub data: String,
}

// ─── Account model ───────────────────────────────────────────────────────────

/// Account model for TX authentication.
/// Stored in the "auth" namespace as JSON under key `account_{address}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub public_key: String,
    pub sequence: u64,
}

// ─── Error types ─────────────────────────────────────────────────────────────

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

    #[error("auth error: {0}")]
    AuthError(String),

    #[error("WASM module not found: {0}")]
    ModuleNotFound(String),
}

pub type Result<T> = std::result::Result<T, BaseAppError>;

#[derive(Debug, Clone)]
pub struct BlockContext {
    pub height: u64,
    pub timestamp: u64,
    pub chain_id: String,
}

// ─── BaseApp ─────────────────────────────────────────────────────────────────

/// Base application — WASM microkernel host.
///
/// All execution goes through WASM components via ComponentHost:
/// - Validator: decode + ed25519 auth + message extraction
/// - Hook: block lifecycle (pre/post execute)
/// - Module: domain logic (bank, etc.)
///
/// No Rust-native fallback. WASM modules must be present.
pub struct BaseApp {
    name: String,
    context: Option<BlockContext>,
    #[allow(dead_code)]
    wasi_host: Arc<WasiHost>,
    component_host: Arc<ComponentHost>,
    vfs: Arc<VirtualFilesystem>,
    module_router: Arc<ModuleRouter>,
    #[allow(dead_code)]
    capability_manager: Arc<CapabilityManager>,
    module_governance: Arc<ModuleGovernance>,
    /// Module paths: name → .wasm file path
    module_paths: HashMap<String, String>,
    global_store: Arc<GlobalAppStore>,
    last_state_root: [u8; 32],
    /// Checkpoint of the last committed store state.
    /// Used to restore to a clean state before ephemeral execution
    /// (verify) and before each propose/report cycle.
    committed_checkpoint: Option<gridway_store::MerkleCheckpoint>,
}

impl BaseApp {
    pub fn new(name: String) -> Result<Self> {
        let merkle_store = MerkleStore::new("state".to_string());
        let global_store = Arc::new(GlobalAppStore::new(merkle_store));

        for ns in ["bank", "auth", "staking", "gov", "system"] {
            global_store.register_namespace(ns, false)
                .map_err(|e| BaseAppError::Store(format!("Failed to register {ns} namespace: {e}")))?;
        }

        let wasi_host = Arc::new(WasiHost::new().map_err(|e| {
            BaseAppError::InitChainFailed(format!("Failed to initialize WASI host: {e}"))
        })?);

        let mut component_host_inner = ComponentHost::new().map_err(|e| {
            BaseAppError::InitChainFailed(format!("Failed to initialize Component host: {e}"))
        })?;

        let vfs = Arc::new(VirtualFilesystem::new());
        Self::setup_stores(&vfs, &global_store)?;

        component_host_inner.set_vfs(vfs.clone());
        let component_host = Arc::new(component_host_inner);

        let capability_manager = Arc::new(CapabilityManager::new());
        let module_router = Arc::new(ModuleRouter::new(wasi_host.clone(), vfs.clone()));
        let governance_authority = "gridway_governance".to_string();
        let module_governance = Arc::new(ModuleGovernance::new(
            module_router.clone(), vfs.clone(), governance_authority,
        ));

        // Load persisted governance registries from VFS
        if let Err(e) = module_governance.load_registries() {
            log::warn!("Failed to load governance registries: {e}");
        }

        let module_base_path = Self::find_module_base_path();
        let mut module_paths = HashMap::new();
        for name in ["hook", "validator", "bank"] {
            module_paths.insert(
                name.to_string(),
                format!("{module_base_path}/{name}_component.wasm"),
            );
        }

        let committed_checkpoint = {
            let store_arc = global_store.get_store();
            let store = store_arc.lock()
                .map_err(|e| BaseAppError::Store(format!("Failed to lock store for initial checkpoint: {e}")))?;
            Some(store.checkpoint())
        };

        Ok(Self {
            name, context: None, wasi_host, component_host, vfs,
            module_router, capability_manager, module_governance, module_paths,
            global_store, last_state_root: [0u8; 32], committed_checkpoint,
        })
    }

    /// Create a new BaseApp with sled-backed state persistence.
    ///
    /// The state database will be stored at `db_path`. On startup, if state
    /// exists on disk it will be loaded automatically. On `commit()`, state
    /// is automatically flushed to disk.
    ///
    /// Keeps full backward compatibility — use `new()` for in-memory only.
    pub fn with_persistence(name: String, db_path: &Path) -> Result<Self> {
        let global_store = Arc::new(
            GlobalAppStore::with_persistence(db_path)
                .map_err(|e| BaseAppError::Store(format!("Failed to create persistent store: {e}")))?
        );

        for ns in ["bank", "auth", "staking", "gov", "system"] {
            global_store.register_namespace(ns, false)
                .map_err(|e| BaseAppError::Store(format!("Failed to register {ns} namespace: {e}")))?;
        }

        let wasi_host = Arc::new(WasiHost::new().map_err(|e| {
            BaseAppError::InitChainFailed(format!("Failed to initialize WASI host: {e}"))
        })?);

        let mut component_host_inner = ComponentHost::new().map_err(|e| {
            BaseAppError::InitChainFailed(format!("Failed to initialize Component host: {e}"))
        })?;

        let vfs = Arc::new(VirtualFilesystem::new());
        Self::setup_stores(&vfs, &global_store)?;

        component_host_inner.set_vfs(vfs.clone());
        let component_host = Arc::new(component_host_inner);

        let capability_manager = Arc::new(CapabilityManager::new());
        let module_router = Arc::new(ModuleRouter::new(wasi_host.clone(), vfs.clone()));
        let governance_authority = "gridway_governance".to_string();
        let module_governance = Arc::new(ModuleGovernance::new(
            module_router.clone(), vfs.clone(), governance_authority,
        ));

        // Load persisted governance registries from VFS
        if let Err(e) = module_governance.load_registries() {
            log::warn!("Failed to load governance registries: {e}");
        }

        let module_base_path = Self::find_module_base_path();
        let mut module_paths = HashMap::new();
        for name in ["hook", "validator", "bank"] {
            module_paths.insert(
                name.to_string(),
                format!("{module_base_path}/{name}_component.wasm"),
            );
        }

        // Recover state root from the loaded persistent store
        let last_state_root = {
            let store = global_store.get_store();
            let store = store.lock()
                .map_err(|e| BaseAppError::Store(format!("Failed to lock store: {e}")))?;
            store.root_hash()
        };

        let committed_checkpoint = {
            let store_arc2 = global_store.get_store();
            let store2 = store_arc2.lock()
                .map_err(|e| BaseAppError::Store(format!("Failed to lock store for initial checkpoint: {e}")))?;
            Some(store2.checkpoint())
        };

        Ok(Self {
            name, context: None, wasi_host, component_host, vfs,
            module_router, capability_manager, module_governance, module_paths,
            global_store, last_state_root, committed_checkpoint,
        })
    }

    pub fn module_governance(&self) -> &Arc<ModuleGovernance> { &self.module_governance }
    pub fn module_router(&self) -> &Arc<ModuleRouter> { &self.module_router }
    pub fn global_store(&self) -> &Arc<GlobalAppStore> { &self.global_store }
    pub fn vfs(&self) -> &Arc<VirtualFilesystem> { &self.vfs }

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

    fn setup_stores(vfs: &Arc<VirtualFilesystem>, global_store: &Arc<GlobalAppStore>) -> Result<()> {
        use crate::vfs::Capability;
        for ns in ["auth", "bank", "staking", "gov", "system"] {
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
    // Account management (auth store)
    // =========================================================================

    pub fn set_account(&mut self, address: &str, account: &Account) -> Result<()> {
        let key = format!("account_{address}");
        let value = serde_json::to_string(account)
            .map_err(|e| BaseAppError::Store(format!("Failed to serialize account: {e}")))?;
        self.global_store.set_namespaced("auth", key.as_bytes(), value.as_bytes())
            .map_err(|e| BaseAppError::Store(format!("Failed to set account: {e}")))?;
        Ok(())
    }

    pub fn get_account(&self, address: &str) -> Option<Account> {
        let key = format!("account_{address}");
        match self.global_store.get_namespaced("auth", key.as_bytes()) {
            Ok(Some(value)) => {
                let json_str = String::from_utf8(value).ok()?;
                serde_json::from_str(&json_str).ok()
            }
            _ => None,
        }
    }

    pub fn increment_sequence(&mut self, address: &str) -> Result<()> {
        let mut account = self.get_account(address)
            .ok_or_else(|| BaseAppError::AuthError(format!("account not found: {address}")))?;
        account.sequence += 1;
        self.set_account(address, &account)
    }

    // =========================================================================
    // WASM module loading helper
    // =========================================================================

    /// Store WASM bytecode in VFS under the "system" namespace with key `code/{name}`.
    fn store_wasm_to_vfs(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let key = format!("code/{name}");
        self.vfs.write_key("system", key.as_bytes(), bytes)
            .map_err(|e| BaseAppError::Store(
                format!("Failed to store WASM for {name} in VFS: {e}")
            ))?;
        log::info!("Stored {name} WASM ({} bytes) to VFS", bytes.len());
        Ok(())
    }

    /// Load WASM bytecode from VFS. Returns None if not found.
    fn load_wasm_from_vfs(&self, name: &str) -> Option<Vec<u8>> {
        let key = format!("code/{name}");
        match self.vfs.read_key("system", key.as_bytes()) {
            Ok(Some(bytes)) if !bytes.is_empty() => {
                log::debug!("Loaded {name} WASM ({} bytes) from VFS", bytes.len());
                Some(bytes)
            }
            _ => None,
        }
    }

    /// Load a WASM component by name. Tries VFS first, falls back to filesystem.
    /// Skips loading if the component is already compiled and cached in ComponentHost.
    fn load_wasm_module(&self, name: &str, component_type: ComponentType) -> Result<()> {
        // Skip if already loaded — avoids redundant VFS reads and recompilation
        if self.component_host.has_component(name) {
            return Ok(());
        }

        // Try loading from VFS first (cached / previously stored)
        let component_bytes = if let Some(bytes) = self.load_wasm_from_vfs(name) {
            bytes
        } else {
            // Fall back to filesystem (genesis / migration)
            let wasm_path = self.module_paths.get(name)
                .ok_or_else(|| BaseAppError::ModuleNotFound(
                    format!("{name} module path not configured")
                ))?;

            let bytes = std::fs::read(wasm_path)
                .map_err(|e| BaseAppError::ModuleNotFound(
                    format!("failed to read {name}.wasm at {wasm_path}: {e}")
                ))?;

            // Store to VFS for future loads (genesis initialization)
            if let Err(e) = self.store_wasm_to_vfs(name, &bytes) {
                log::warn!("Failed to cache {name} WASM to VFS: {e}");
            }

            bytes
        };

        // Validator needs more fuel for ed25519 crypto in WASM
        let gas_limit = match component_type {
            ComponentType::Validator => 100_000_000,
            _ => 10_000_000,
        };

        let path = self.module_paths.get(name)
            .map(|p| p.clone().into())
            .unwrap_or_else(|| format!("vfs://system/wasm/{name}").into());

        let info = ComponentInfo {
            name: name.to_string(),
            path,
            component_type,
            gas_limit,
        };

        self.component_host.load_component(name, &component_bytes, info)
            .map_err(|e| BaseAppError::ExecutionError(
                format!("failed to load {name} component: {e}")
            ))
    }

    // =========================================================================
    // Validator — WASM only (ComponentHost.execute_validator)
    // =========================================================================

    /// Validate a raw transaction via the WASM validator component.
    /// All decoding, signature verification, and sequence checks happen inside WASM.
    /// State is accessed through kvstore WIT → VFS → MerkleStore.
    fn validate_tx(
        &self,
        tx_bytes: &[u8],
        height: u64,
        timestamp: u64,
        chain_id: &str,
    ) -> Result<ValidatedTx> {
        // Load validator WASM (no fallback)
        self.load_wasm_module("validator", ComponentType::Validator)?;

        // Execute through ComponentHost → wasmtime → WIT bindings → VFS kvstore
        let result = self.component_host.execute_validator(
            "validator", height, timestamp, chain_id, tx_bytes,
        ).map_err(|e| BaseAppError::ExecutionError(
            format!("validator WASM execution error: {e}")
        ))?;

        if !result.success {
            return Err(BaseAppError::InvalidTx(
                result.error.unwrap_or_else(|| "validation failed".into())
            ));
        }

        // Extract ValidatedTx from ComponentResult
        let data = result.data
            .ok_or_else(|| BaseAppError::InvalidTx("validator returned no data".into()))?;

        let tx_data = data.get("tx")
            .and_then(|v| if v.is_null() { None } else { Some(v) })
            .ok_or_else(|| BaseAppError::InvalidTx("validator returned no tx".into()))?;

        let sender = tx_data.get("sender")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BaseAppError::InvalidTx("missing sender in validated tx".into()))?
            .to_string();

        let sequence = tx_data.get("sequence")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let gas_limit = tx_data.get("gas_limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(200_000);

        let messages = tx_data.get("messages")
            .and_then(|v| v.as_array())
            .ok_or_else(|| BaseAppError::InvalidTx("missing messages in validated tx".into()))?
            .iter()
            .map(|m| {
                Ok(TxMessage {
                    type_url: m.get("type_url").and_then(|v| v.as_str())
                        .ok_or_else(|| BaseAppError::InvalidTx("message missing type_url".into()))?
                        .to_string(),
                    data: m.get("data").and_then(|v| v.as_str())
                        .ok_or_else(|| BaseAppError::InvalidTx("message missing data".into()))?
                        .to_string(),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(ValidatedTx { sender, messages, sequence, gas_limit })
    }

    // =========================================================================
    // Hook — WASM only (ComponentHost.execute_hook_pre/post)
    // =========================================================================

    /// Run pre-execute hook via WASM. Errors if hook WASM not found.
    fn run_hook_pre(&self, height: u64, timestamp: u64, chain_id: &str) -> Result<Vec<Event>> {
        self.load_wasm_module("hook", ComponentType::Hook)?;

        let result = self.component_host.execute_hook_pre(
            "hook", height, timestamp, chain_id, None,
        ).map_err(|e| BaseAppError::ExecutionError(
            format!("hook pre-execute WASM error: {e}")
        ))?;

        if !result.success {
            return Err(BaseAppError::ExecutionError(
                result.error.unwrap_or_else(|| "hook pre-execute failed".into())
            ));
        }

        Ok(Self::parse_component_events(&result.data))
    }

    /// Run post-execute hook via WASM. Errors if hook WASM not found.
    fn run_hook_post(
        &self, height: u64, timestamp: u64, chain_id: &str,
        tx_count: u32, total_gas: u64,
    ) -> Result<Vec<Event>> {
        self.load_wasm_module("hook", ComponentType::Hook)?;

        let result = self.component_host.execute_hook_post(
            "hook", height, timestamp, chain_id, None, tx_count, total_gas,
        ).map_err(|e| BaseAppError::ExecutionError(
            format!("hook post-execute WASM error: {e}")
        ))?;

        if !result.success {
            return Err(BaseAppError::ExecutionError(
                result.error.unwrap_or_else(|| "hook post-execute failed".into())
            ));
        }

        Ok(Self::parse_component_events(&result.data))
    }

    // =========================================================================
    // Module dispatch — WASM only
    // =========================================================================

    /// Resolve module name from type_url: "bank.MsgSend" → "bank"
    fn resolve_module(&self, type_url: &str) -> Result<String> {
        if let Some(dot_pos) = type_url.find('.') {
            let module_name = &type_url[..dot_pos];
            if !module_name.is_empty() {
                return Ok(module_name.to_string());
            }
        }
        Err(BaseAppError::ExecutionError(
            format!("cannot resolve module from type_url: {type_url}")
        ))
    }

    /// Dispatch a message to the appropriate WASM module.
    fn dispatch_message(
        &self, sender: &str, msg: &TxMessage,
        height: u64, timestamp: u64, chain_id: &str,
    ) -> Result<(u64, Vec<Event>)> {
        let module_name = self.resolve_module(&msg.type_url)?;

        // Governance messages are handled by built-in handler
        // (governance module is not a WASM component)
        if module_name == "governance" {
            return self.handle_governance_msg(msg);
        }

        // All other modules: WASM dispatch (no fallback)
        self.load_wasm_module(&module_name, ComponentType::Module)?;

        let result = self.component_host.execute_module(
            &module_name, height, timestamp, chain_id,
            &msg.type_url, &msg.data, sender, 100_000,
        ).map_err(|e| BaseAppError::ExecutionError(
            format!("{module_name} WASM module error: {e}")
        ))?;

        log::info!(
            "{module_name} WASM execute_module result: {:?}",
            (result.success, &result.error, result.gas_used)
        );

        if result.success {
            Ok((result.gas_used, Self::parse_component_events(&result.data)))
        } else {
            Err(BaseAppError::TxFailed(
                result.error.unwrap_or_else(|| format!("{module_name} WASM execution failed"))
            ))
        }
    }

    /// Handle governance messages (built-in — not a WASM module).
    fn handle_governance_msg(&self, msg: &TxMessage) -> Result<(u64, Vec<Event>)> {
        let msg_value: serde_json::Value = serde_json::from_str(&msg.data)
            .map_err(|e| BaseAppError::InvalidTx(format!("invalid governance message data: {e}")))?;

        let msg_name = msg.type_url.split('.').nth(1).unwrap_or("");

        match msg_name {
            "MsgStoreCode" => {
                let store_msg: MsgStoreCode = serde_json::from_value(msg_value)
                    .map_err(|e| BaseAppError::InvalidTx(format!("decode MsgStoreCode: {e}")))?;
                match self.module_governance.handle_store_code(store_msg) {
                    Ok(code_id) => Ok((50_000, vec![
                        Event::new("store_code", vec![EventAttribute::new("code_id", code_id.to_string())])
                    ])),
                    Err(e) => Err(BaseAppError::TxFailed(format!("store_code failed: {e}"))),
                }
            }
            "MsgInstallModule" => {
                let install_msg: MsgInstallModule = serde_json::from_value(msg_value)
                    .map_err(|e| BaseAppError::InvalidTx(format!("decode MsgInstallModule: {e}")))?;
                let name = install_msg.config.name.clone();
                let code_id = install_msg.code_id;
                match self.module_governance.handle_install_module(install_msg) {
                    Ok(_) => Ok((100_000, vec![
                        Event::new("install_module", vec![
                            EventAttribute::new("module_name", &name),
                            EventAttribute::new("code_id", code_id.to_string()),
                        ])
                    ])),
                    Err(e) => Err(BaseAppError::TxFailed(format!("install_module failed: {e}"))),
                }
            }
            "MsgUpgradeModule" => {
                let upgrade_msg: MsgUpgradeModule = serde_json::from_value(msg_value)
                    .map_err(|e| BaseAppError::InvalidTx(format!("decode MsgUpgradeModule: {e}")))?;
                let mname = upgrade_msg.module_name.clone();
                let new_code_id = upgrade_msg.new_code_id;
                match self.module_governance.handle_upgrade_module(upgrade_msg) {
                    Ok(_) => Ok((150_000, vec![
                        Event::new("upgrade_module", vec![
                            EventAttribute::new("module_name", &mname),
                            EventAttribute::new("new_code_id", new_code_id.to_string()),
                        ])
                    ])),
                    Err(e) => Err(BaseAppError::TxFailed(format!("upgrade_module failed: {e}"))),
                }
            }
            _ => Err(BaseAppError::ExecutionError(
                format!("unknown governance message: {}", msg.type_url)
            ))
        }
    }

    // =========================================================================
    // Helper: parse events from ComponentResult
    // =========================================================================

    fn parse_component_events(data: &Option<serde_json::Value>) -> Vec<Event> {
        let mut events = Vec::new();
        if let Some(data) = data {
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
        events
    }

    // =========================================================================
    // Consensus interface — fully WASM pipeline
    // =========================================================================

    /// Execute a block. All steps go through WASM components.
    pub fn execute_block(
        &mut self, height: u64, timestamp: u64, chain_id: &str, txs: &[Vec<u8>],
    ) -> Result<([u8; 32], Vec<TxResponse>)> {
        self.context = Some(BlockContext {
            height, timestamp, chain_id: chain_id.to_string(),
        });

        // Set deterministic block timestamp for module governance
        self.module_governance.set_block_timestamp(timestamp);

        // 1. Pre-execute hook (WASM)
        let _pre_events = self.run_hook_pre(height, timestamp, chain_id)?;

        let mut responses = Vec::with_capacity(txs.len());
        let mut block_gas_used = 0u64;

        // 2. Process transactions
        for (i, tx_bytes) in txs.iter().enumerate() {
            match self.process_transaction(tx_bytes, height, timestamp, chain_id) {
                Ok(response) => {
                    block_gas_used += response.gas_used as u64;
                    responses.push(response);
                }
                Err(e) => {
                    responses.push(TxResponse::failure(
                        1, format!("Transaction {i} failed: {e}"), 0, 0,
                    ));
                }
            }
        }

        // 3. Post-execute hook (WASM)
        let _post_events = self.run_hook_post(
            height, timestamp, chain_id, txs.len() as u32, block_gas_used,
        )?;

        // 4. Compute state root
        let state_root = {
            let store_arc = self.global_store.get_store();
            let store = store_arc.lock()
                .map_err(|e| BaseAppError::Store(format!("Failed to lock store: {e}")))?;
            store.root_hash()
        };

        self.context = None;
        Ok((state_root, responses))
    }

    /// Process a single transaction: validate (WASM) → dispatch (WASM) → increment sequence.
    fn process_transaction(
        &mut self, tx_bytes: &[u8], height: u64, timestamp: u64, chain_id: &str,
    ) -> Result<TxResponse> {
        // 2a. Validate via WASM validator
        let validated = self.validate_tx(tx_bytes, height, timestamp, chain_id)?;

        let mut total_gas_used = 0u64;
        let mut events = Vec::new();

        // 2b. Dispatch each message via WASM modules.
        //     Checkpoint before dispatch so partial failures roll back
        //     all state mutations for this TX (atomicity).
        let tx_checkpoint = self.store_checkpoint()?;

        for msg in &validated.messages {
            match self.dispatch_message(
                &validated.sender, msg, height, timestamp, chain_id,
            ) {
                Ok((gas, msg_events)) => {
                    total_gas_used += gas;
                    events.extend(msg_events);
                }
                Err(e) => {
                    // Rollback all state changes for this TX
                    self.store_restore(tx_checkpoint)
                        .map_err(|re| BaseAppError::Store(
                            format!("rollback after tx failure failed: {re} (original: {e})")
                        ))?;
                    return Err(e);
                }
            }
        }

        // 2c. Increment sequence
        self.increment_sequence(&validated.sender)?;

        Ok(TxResponse::success(
            "transaction executed successfully".to_string(),
            (total_gas_used + 10000) as i64,
            total_gas_used as i64,
            events,
        ))
    }

    pub fn commit(&mut self) -> Result<[u8; 32]> {
        let root_hash = {
            let store_arc = self.global_store.get_store();
            let mut store = store_arc.lock()
                .map_err(|e| BaseAppError::Store(format!("Failed to lock store: {e}")))?;
            store.commit()
                .map_err(|e| BaseAppError::Store(format!("Commit failed: {e}")))?
        };
        self.last_state_root = root_hash;
        // Save committed state checkpoint for future restore_to_committed() calls
        self.committed_checkpoint = Some(self.store_checkpoint()?);
        log::info!("Committed state with root: {}", hex::encode(&root_hash));
        Ok(root_hash)
    }

    pub fn last_state_root(&self) -> &[u8; 32] { &self.last_state_root }

    // =========================================================================
    // Store checkpoint/restore — overlay mechanism for consensus safety
    // =========================================================================

    /// Create a checkpoint of the current store state.
    /// The checkpoint can later be passed to `store_restore()` to roll back.
    pub fn store_checkpoint(&self) -> Result<gridway_store::MerkleCheckpoint> {
        self.global_store.checkpoint()
            .map_err(|e| BaseAppError::Store(format!("checkpoint failed: {e}")))
    }

    /// Restore store state from a checkpoint, discarding all changes since.
    pub fn store_restore(&self, cp: gridway_store::MerkleCheckpoint) -> Result<()> {
        self.global_store.restore(cp)
            .map_err(|e| BaseAppError::Store(format!("restore failed: {e}")))
    }

    /// Restore to the last committed state.
    ///
    /// This is essential before propose/report to ensure execution starts
    /// from a clean, committed baseline — not from leftover state of a
    /// previously verified or proposed block.
    pub fn restore_to_committed(&self) -> Result<()> {
        if let Some(ref cp) = self.committed_checkpoint {
            self.global_store.restore_from(cp)
                .map_err(|e| BaseAppError::Store(format!("restore to committed failed: {e}")))?;
        }
        Ok(())
    }

    /// Execute a block ephemerally — state is restored after execution.
    ///
    /// Returns only the computed state root.  Used by `verify()` so that
    /// verifying a block does NOT mutate the shared store state.
    pub fn execute_block_ephemeral(
        &mut self, height: u64, timestamp: u64, chain_id: &str, txs: &[Vec<u8>],
    ) -> Result<[u8; 32]> {
        // Start from committed state
        self.restore_to_committed()?;
        let cp = self.store_checkpoint()?;
        let result = self.execute_block(height, timestamp, chain_id, txs);
        // Always restore — ephemeral execution must not persist
        self.store_restore(cp)
            .map_err(|e| BaseAppError::Store(format!("ephemeral restore failed: {e}")))?;
        result.map(|(root, _)| root)
    }

    pub fn export_snapshot(&self) -> Result<gridway_store::merkle::StateSnapshot> {
        let store = self.global_store.get_store();
        let store = store.lock().map_err(|e| BaseAppError::Store(format!("lock: {e}")))?;
        Ok(store.to_snapshot())
    }

    pub fn import_snapshot(&mut self, snapshot: &gridway_store::merkle::StateSnapshot) -> Result<()> {
        let store = self.global_store.get_store();
        let mut store = store.lock().map_err(|e| BaseAppError::Store(format!("lock: {e}")))?;
        store.from_snapshot(snapshot).map_err(|e| BaseAppError::Store(format!("import: {e}")))?;
        self.last_state_root = store.root_hash();
        Ok(())
    }

    // =========================================================================
    // Helpers (testing / genesis)
    // =========================================================================

    pub fn set_balance(&mut self, address: &str, denom: &str, amount: u64) -> Result<()> {
        let key = format!("balance_{address}_{denom}");
        let value = amount.to_string();
        self.global_store.set_namespaced("bank", key.as_bytes(), value.as_bytes())
            .map_err(|e| BaseAppError::Store(format!("Failed to set balance: {e}")))?;
        Ok(())
    }

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

// ─── Tests ───────────────────────────────────────────────────────────────────

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
        // Empty block: hooks run via WASM, no txs to validate
        let (_state_root, responses) = app.execute_block(1, 1234567890, "test-chain", &[]).unwrap();
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
    fn test_ephemeral_execution_does_not_mutate_state() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        app.set_balance("alice", "ugridway", 1000).unwrap();
        let root_before = app.commit().unwrap();

        // Ephemeral execution: set a balance, get root, but state should restore
        let ephemeral_root = app.execute_block_ephemeral(1, 1000, "test", &[]).unwrap();

        // State should be unchanged after ephemeral execution
        let store_arc = app.global_store().get_store();
        let store = store_arc.lock().unwrap();
        let root_after = store.root_hash();
        assert_eq!(root_before, root_after, "ephemeral execution must not mutate state");
    }

    #[test]
    fn test_restore_to_committed() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        app.set_balance("alice", "ugridway", 1000).unwrap();
        let committed_root = app.commit().unwrap();

        // Modify state
        app.set_balance("alice", "ugridway", 9999).unwrap();
        let dirty_root = {
            let store_arc = app.global_store().get_store();
            let guard = store_arc.lock().unwrap();
            guard.root_hash()
        };
        assert_ne!(committed_root, dirty_root);

        // Restore to committed
        app.restore_to_committed().unwrap();
        let restored_root = {
            let store_arc = app.global_store().get_store();
            let guard = store_arc.lock().unwrap();
            guard.root_hash()
        };
        assert_eq!(committed_root, restored_root, "restore_to_committed must revert state");
    }

    #[test]
    fn test_re_execute_same_height_works() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        // Without executed_heights guard, re-executing at same height should work
        let (root1, _) = app.execute_block(1, 1000, "test", &[]).unwrap();
        let (root2, _) = app.execute_block(1, 1000, "test", &[]).unwrap();
        assert_eq!(root1, root2, "re-executing same block should give same root");
    }

    #[test]
    fn test_export_import_snapshot() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        app.set_balance("alice", "ugridway", 1000).unwrap();
        app.set_balance("bob", "ugridway", 500).unwrap();
        app.commit().unwrap();

        let snapshot = app.export_snapshot().unwrap();
        assert!(!snapshot.entries.is_empty());

        let mut app2 = BaseApp::new("test-app2".to_string()).unwrap();
        app2.import_snapshot(&snapshot).unwrap();
        assert_eq!(app2.get_balance("alice", "ugridway").unwrap(), 1000);
        assert_eq!(app2.get_balance("bob", "ugridway").unwrap(), 500);
        assert_eq!(app2.last_state_root(), app.last_state_root());
    }

    #[test]
    fn test_balance_helpers() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        app.set_balance("alice", "uatom", 1000).unwrap();
        assert_eq!(app.get_balance("alice", "uatom").unwrap(), 1000);
    }

    #[test]
    fn test_account_crud() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        let account = Account { public_key: "abcd1234".to_string(), sequence: 0 };
        app.set_account("test_addr", &account).unwrap();
        let loaded = app.get_account("test_addr").unwrap();
        assert_eq!(loaded.public_key, "abcd1234");
        assert_eq!(loaded.sequence, 0);
        app.increment_sequence("test_addr").unwrap();
        let loaded = app.get_account("test_addr").unwrap();
        assert_eq!(loaded.sequence, 1);
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
        let modules = app.module_governance().list_modules().unwrap();
        assert_eq!(modules.len(), 0);
    }

    #[test]
    fn test_resolve_module() {
        let app = BaseApp::new("test-app".to_string()).unwrap();
        assert_eq!(app.resolve_module("bank.MsgSend").unwrap(), "bank");
        assert_eq!(app.resolve_module("governance.MsgStoreCode").unwrap(), "governance");
        assert_eq!(app.resolve_module("staking.MsgDelegate").unwrap(), "staking");
        assert!(app.resolve_module("NoModule").is_err());
        assert!(app.resolve_module("").is_err());
        assert!(app.resolve_module(".MsgSend").is_err());
    }

    #[test]
    fn test_execute_block_with_hooks_via_wasm() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        // Empty block: hooks run via WASM, should succeed
        let (root, responses) = app.execute_block(1, 1000, "test-chain", &[]).unwrap();
        assert_eq!(responses.len(), 0);
        let (root2, _) = app.execute_block(2, 2000, "test-chain", &[]).unwrap();
        assert_eq!(root, root2); // Same root since no state changed
    }

    #[test]
    fn test_validate_tx_via_wasm_rejects_invalid_json() {
        let app = BaseApp::new("test-app".to_string()).unwrap();
        let result = app.validate_tx(b"not json", 1, 1000, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_tx_via_wasm_rejects_missing_fields() {
        let app = BaseApp::new("test-app".to_string()).unwrap();
        // Missing signature
        let tx = serde_json::json!({"public_key": "aa", "body": {}});
        let result = app.validate_tx(
            serde_json::to_vec(&tx).unwrap().as_slice(), 1, 1000, "test",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_full_wasm_tx_pipeline() {
        // End-to-end test: create account → sign tx → validate via WASM → dispatch
        let mut app = BaseApp::new("test-e2e".to_string()).unwrap();

        // Set up balances
        app.set_balance("alice_addr", "ugridway", 10_000).unwrap();
        app.set_balance("bob_addr", "ugridway", 0).unwrap();

        // Create an account for the signer
        // We need a real ed25519 keypair. Use gridway-crypto's deterministic key.
        // For this test we verify the full WASM pipeline works end-to-end.

        // Use gridway_crypto to create a keypair and sign a tx
        use gridway_crypto::{sign_tx_body, Address};
        use commonware_cryptography::ed25519::PrivateKey;
        use commonware_cryptography::Signer as _;

        let private_key = PrivateKey::from_seed(42);
        let public_key = private_key.public_key();
        let pk_hex = hex::encode(public_key.as_ref());
        let address = Address::from_public_key(&public_key).to_hex();

        // Register account
        app.set_account(&address, &Account {
            public_key: pk_hex.clone(),
            sequence: 0,
        }).unwrap();
        app.set_balance(&address, "ugridway", 5000).unwrap();

        // Create a signed transaction
        let body = serde_json::json!({
            "messages": [{
                "@type": "bank.MsgSend",
                "from_address": address,
                "to_address": "bob_addr",
                "amount": [{"denom": "ugridway", "amount": "100"}]
            }],
            "sequence": 0
        });
        let body_str = serde_json::to_string(&body).unwrap();
        let sig = sign_tx_body(&private_key, body_str.as_bytes());
        let sig_hex = hex::encode(sig.as_ref());

        let signed_tx = serde_json::json!({
            "public_key": pk_hex,
            "signature": sig_hex,
            "body": body
        });
        let tx_bytes = serde_json::to_vec(&signed_tx).unwrap();

        // Execute block with this transaction — all through WASM
        let (state_root, responses) = app.execute_block(1, 1000, "test-chain", &[tx_bytes]).unwrap();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].code, 0, "tx should succeed: {}", responses[0].log);

        // Verify balance changed
        assert_eq!(app.get_balance(&address, "ugridway").unwrap(), 4900);
        assert_eq!(app.get_balance("bob_addr", "ugridway").unwrap(), 100);

        // Verify sequence incremented
        let acct = app.get_account(&address).unwrap();
        assert_eq!(acct.sequence, 1);

        // Commit and verify state root
        let committed_root = app.commit().unwrap();
        assert_ne!(committed_root, [0u8; 32]);
    }

    #[test]
    fn test_parse_component_events() {
        let events = BaseApp::parse_component_events(&None);
        assert!(events.is_empty());

        let data = serde_json::json!({"events": [{
            "event_type": "transfer",
            "attributes": [
                {"key": "sender", "value": "alice"},
                {"key": "recipient", "value": "bob"}
            ]
        }]});
        let events = BaseApp::parse_component_events(&Some(data));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].r#type, "transfer");
        assert_eq!(events[0].attributes.len(), 2);
    }

    #[test]
    fn test_validated_tx_struct() {
        let tx = ValidatedTx {
            sender: "alice".to_string(),
            messages: vec![TxMessage {
                type_url: "bank.MsgSend".to_string(),
                data: r#"{"to":"bob"}"#.to_string(),
            }],
            sequence: 0,
            gas_limit: 200_000,
        };
        assert_eq!(tx.sender, "alice");
        assert_eq!(tx.messages.len(), 1);
    }
}

/// Execution mode (kept for compatibility with module_router)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecMode {
    Check, ReCheck, Simulate, PrepareProposal,
    ProcessProposal, VoteExtension, VerifyVoteExtension, Finalize,
}
