//! Base application framework for gridway.
//!
//! Acts as a WASM microkernel host — all blockchain logic runs inside
//! WASM modules that interact with state through the Virtual Filesystem (VFS).
//!
//! Pipeline architecture (WIT-aligned):
//!   1. hook.pre_execute(block_ctx)             — block lifecycle hook
//!   2. for each tx:
//!      a. validator.validate(tx_ctx, raw_bytes) — decode + auth + extract messages
//!      b. for each msg: module.handle(ctx, msg)  — dispatch to domain modules
//!   3. hook.post_execute(block_ctx, stats)      — block lifecycle hook
//!   4. commit → state_root
//!
//! Validator and hook logic is currently built-in (Rust native) but structured
//! for future replacement by WASM components when they are compiled.

pub mod capabilities;
pub mod component_bindings;
pub mod component_host;
pub mod module_governance;
pub mod module_router;
pub mod vfs;
pub mod wasi_host;

use commonware_codec::DecodeExt;

use gridway_store::{GlobalAppStore, MerkleStore, KVStore};
use gridway_types::{Event, EventAttribute, TxResponse};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
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
///
/// Produced by the validator pipeline (currently built-in, future WASM).
/// Contains the authenticated sender and extracted messages.
#[derive(Debug, Clone)]
pub struct ValidatedTx {
    /// Authenticated sender address (derived from signature verification)
    pub sender: String,
    /// Messages to be dispatched to modules
    pub messages: Vec<TxMessage>,
    /// Sequence number (for replay protection tracking)
    pub sequence: u64,
    /// Gas limit declared by the transaction
    pub gas_limit: u64,
}

/// A message extracted from a validated transaction.
/// Matches the `message` record in `validator.wit`.
///
/// Dispatched to the appropriate module's `handle()` function
/// based on the `type_url` prefix (e.g., "bank.MsgSend" → bank module).
#[derive(Debug, Clone)]
pub struct TxMessage {
    /// Message type identifier (e.g., "bank.MsgSend", "governance.MsgStoreCode")
    pub type_url: String,
    /// Message payload as JSON string
    pub data: String,
}

// ─── Account model ───────────────────────────────────────────────────────────

/// Account model for TX authentication.
///
/// Stored in the "auth" namespace as JSON under key `account_{address}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Hex-encoded 32-byte ed25519 public key
    pub public_key: String,
    /// Sequence number (nonce) for replay protection
    pub sequence: u64,
}

// ─── Error types ─────────────────────────────────────────────────────────────

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

    #[error("auth error: {0}")]
    AuthError(String),
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

// ─── BaseApp ─────────────────────────────────────────────────────────────────

/// Base application — acts as microkernel host for WASM modules.
///
/// WIT-aligned pipeline:
/// - `execute_block()` orchestrates the full block lifecycle
/// - Hooks (pre/post) for block-level logic (no-op if WASM not loaded)
/// - Built-in validator for tx decode + auth (future: WASM validator)
/// - Dynamic message dispatch to WASM domain modules
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
    /// Module paths for WASI modules (name → wasm file path)
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
        for name in ["hook", "validator", "bank"] {
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
    // Account management (auth store)
    // =========================================================================

    /// Store an account in the auth namespace.
    pub fn set_account(&mut self, address: &str, account: &Account) -> Result<()> {
        let key = format!("account_{address}");
        let value = serde_json::to_string(account)
            .map_err(|e| BaseAppError::Store(format!("Failed to serialize account: {e}")))?;
        self.global_store.set_namespaced("auth", key.as_bytes(), value.as_bytes())
            .map_err(|e| BaseAppError::Store(format!("Failed to set account: {e}")))?;
        Ok(())
    }

    /// Get an account from the auth namespace.
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

    /// Increment the sequence number for an account.
    pub fn increment_sequence(&mut self, address: &str) -> Result<()> {
        let mut account = self.get_account(address)
            .ok_or_else(|| BaseAppError::AuthError(format!("account not found: {address}")))?;
        account.sequence += 1;
        self.set_account(address, &account)
    }

    // =========================================================================
    // Transaction validation (built-in validator)
    // =========================================================================

    /// Validate and decode a raw transaction.
    ///
    /// Built-in implementation of the `validator.wit` interface.
    /// Performs: JSON decode → ed25519 signature verification → sequence check
    /// → message extraction → ValidatedTx.
    ///
    /// When a WASM validator module is compiled, this will be replaced by
    /// `component_host.execute_validator()`.
    fn validate_tx(
        &self,
        tx_bytes: &[u8],
        _height: u64,
        _timestamp: u64,
        _chain_id: &str,
    ) -> Result<ValidatedTx> {
        // 1. Decode JSON envelope
        let decoded_tx: serde_json::Value = serde_json::from_slice(tx_bytes)
            .map_err(|e| BaseAppError::InvalidTx(format!("invalid JSON: {e}")))?;

        // 2. Extract envelope fields
        let public_key_hex = decoded_tx.get("public_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BaseAppError::AuthError("missing public_key field".into()))?;

        let signature_hex = decoded_tx.get("signature")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BaseAppError::AuthError("missing signature field".into()))?;

        let body = decoded_tx.get("body")
            .ok_or_else(|| BaseAppError::InvalidTx("missing body field".into()))?;

        // 3. Verify ed25519 signature
        let pk_bytes = hex::decode(public_key_hex)
            .map_err(|e| BaseAppError::AuthError(format!("invalid public_key hex: {e}")))?;
        let pk = commonware_cryptography::ed25519::PublicKey::decode(pk_bytes.as_ref())
            .map_err(|e| BaseAppError::AuthError(format!("invalid ed25519 public key: {e}")))?;

        let sig_bytes = hex::decode(signature_hex)
            .map_err(|e| BaseAppError::AuthError(format!("invalid signature hex: {e}")))?;
        let sig = commonware_cryptography::ed25519::Signature::decode(sig_bytes.as_ref())
            .map_err(|e| BaseAppError::AuthError(format!("invalid ed25519 signature: {e}")))?;

        // Derive address from public key
        let signer_address = gridway_crypto::Address::from_public_key(&pk).to_hex();

        // Serialize body to canonical JSON bytes for verification
        let body_bytes = serde_json::to_string(body)
            .map_err(|e| BaseAppError::AuthError(format!("failed to serialize body: {e}")))?;

        // Verify signature
        if !gridway_crypto::verify_tx_body(&pk, body_bytes.as_bytes(), &sig) {
            return Err(BaseAppError::AuthError("signature verification failed".into()));
        }

        // 4. Check sequence number
        let tx_sequence = body.get("sequence")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let account = self.get_account(&signer_address)
            .ok_or_else(|| BaseAppError::AuthError(format!("account not found: {signer_address}")))?;

        if tx_sequence != account.sequence {
            return Err(BaseAppError::AuthError(format!(
                "sequence mismatch: expected {}, got {tx_sequence}",
                account.sequence
            )));
        }

        // Verify public key matches stored account
        if account.public_key != public_key_hex {
            return Err(BaseAppError::AuthError(format!(
                "public key mismatch for address {signer_address}"
            )));
        }

        // 5. Extract messages into TxMessage format
        let messages_array = body.get("messages")
            .and_then(|m| m.as_array())
            .ok_or_else(|| BaseAppError::InvalidTx("no messages in transaction".into()))?;

        let mut messages = Vec::with_capacity(messages_array.len());
        for (idx, msg_value) in messages_array.iter().enumerate() {
            let type_url = msg_value.get("@type")
                .and_then(|t| t.as_str())
                .ok_or_else(|| BaseAppError::InvalidTx(format!("message {idx} missing @type")))?
                .to_string();

            // Verify from_address matches signer (if present)
            if let Some(from_addr) = msg_value.get("from_address").and_then(|v| v.as_str()) {
                if from_addr != signer_address {
                    return Err(BaseAppError::AuthError(format!(
                        "message {idx}: from_address '{from_addr}' does not match signer '{signer_address}'"
                    )));
                }
            }

            let data = serde_json::to_string(msg_value)
                .map_err(|e| BaseAppError::InvalidTx(
                    format!("failed to serialize message {idx}: {e}")
                ))?;

            messages.push(TxMessage { type_url, data });
        }

        Ok(ValidatedTx {
            sender: signer_address,
            messages,
            sequence: tx_sequence,
            gas_limit: 200_000,
        })
    }

    // =========================================================================
    // Module resolution and message dispatch
    // =========================================================================

    /// Resolve a module name from a message type_url.
    ///
    /// Format: `"module_name.MsgName"` → `"module_name"`
    ///
    /// Examples:
    /// - `"bank.MsgSend"` → `"bank"`
    /// - `"governance.MsgStoreCode"` → `"governance"`
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

    /// Dispatch a message to the appropriate module.
    ///
    /// Routing order:
    /// 1. If a WASM module exists for the resolved module name → WASM dispatch
    /// 2. Otherwise → built-in handler (governance, etc.)
    ///
    /// Returns (gas_used, events) on success.
    fn dispatch_message(
        &self,
        sender: &str,
        msg: &TxMessage,
        height: u64,
        timestamp: u64,
        chain_id: &str,
    ) -> Result<(u64, Vec<Event>)> {
        let module_name = self.resolve_module(&msg.type_url)?;

        // Try WASM module dispatch first
        if let Some(wasm_path) = self.module_paths.get(&module_name) {
            if std::path::Path::new(wasm_path).exists() {
                return self.dispatch_to_wasm(
                    &module_name, sender, msg, height, timestamp, chain_id,
                );
            }
        }

        // Fall back to built-in handlers
        self.dispatch_builtin(&module_name, msg)
    }

    /// Dispatch a message to a WASM module via ComponentHost.
    fn dispatch_to_wasm(
        &self,
        module_name: &str,
        sender: &str,
        msg: &TxMessage,
        height: u64,
        timestamp: u64,
        chain_id: &str,
    ) -> Result<(u64, Vec<Event>)> {
        let wasm_path = self.module_paths.get(module_name)
            .ok_or_else(|| BaseAppError::ExecutionError(
                format!("{module_name} module path not configured")
            ))?;

        let component_bytes = std::fs::read(wasm_path)
            .map_err(|e| BaseAppError::ExecutionError(
                format!("failed to read {module_name}.wasm at {wasm_path}: {e}")
            ))?;

        let info = ComponentInfo {
            name: module_name.to_string(),
            path: wasm_path.clone().into(),
            component_type: ComponentType::Module,
            gas_limit: 1_000_000,
        };
        let _ = self.component_host.load_component(module_name, &component_bytes, info);

        let result = self.component_host.execute_module(
            module_name, height, timestamp, chain_id,
            &msg.type_url, &msg.data, sender, 100_000,
        );

        log::info!(
            "{module_name} WASM execute_module result: {:?}",
            result.as_ref().map(|r| (r.success, &r.error, r.gas_used))
        );

        match result {
            Ok(comp_result) if comp_result.success => {
                let events = Self::parse_component_events(&comp_result.data);
                Ok((comp_result.gas_used, events))
            }
            Ok(comp_result) => {
                Err(BaseAppError::TxFailed(
                    comp_result.error.unwrap_or_else(|| format!("{module_name} WASM execution failed"))
                ))
            }
            Err(e) => {
                Err(BaseAppError::ExecutionError(
                    format!("{module_name} WASM module error: {e}")
                ))
            }
        }
    }

    /// Dispatch a message to a built-in handler.
    fn dispatch_builtin(
        &self,
        module_name: &str,
        msg: &TxMessage,
    ) -> Result<(u64, Vec<Event>)> {
        match module_name {
            "governance" => self.handle_governance_msg(msg),
            _ => Err(BaseAppError::ExecutionError(
                format!("no handler for module '{module_name}' (type_url: {})", msg.type_url)
            ))
        }
    }

    /// Handle governance messages (built-in module).
    ///
    /// Handles: MsgStoreCode, MsgInstallModule, MsgUpgradeModule
    fn handle_governance_msg(&self, msg: &TxMessage) -> Result<(u64, Vec<Event>)> {
        let msg_value: serde_json::Value = serde_json::from_str(&msg.data)
            .map_err(|e| BaseAppError::InvalidTx(
                format!("invalid governance message data: {e}")
            ))?;

        // Extract message name from type_url: "governance.MsgStoreCode" → "MsgStoreCode"
        let msg_name = msg.type_url.split('.').nth(1).unwrap_or("");

        match msg_name {
            "MsgStoreCode" => {
                let store_msg: MsgStoreCode = serde_json::from_value(msg_value)
                    .map_err(|e| BaseAppError::InvalidTx(format!("decode MsgStoreCode: {e}")))?;
                match self.module_governance.handle_store_code(store_msg) {
                    Ok(code_id) => Ok((50_000, vec![
                        Event::new("store_code", vec![
                            EventAttribute::new("code_id", code_id.to_string()),
                        ])
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
                let module_name = upgrade_msg.module_name.clone();
                let new_code_id = upgrade_msg.new_code_id;
                match self.module_governance.handle_upgrade_module(upgrade_msg) {
                    Ok(_) => Ok((150_000, vec![
                        Event::new("upgrade_module", vec![
                            EventAttribute::new("module_name", &module_name),
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
    // Hook execution (pre/post block lifecycle)
    // =========================================================================

    /// Run pre-execute hook. No-op if hook WASM module is not loaded.
    ///
    /// Called before any transaction processing in a block.
    /// Use for: inflation minting, epoch transitions, parameter updates, etc.
    fn run_hook_pre(&self, height: u64, timestamp: u64, chain_id: &str) -> Result<Vec<Event>> {
        if let Some(hook_path) = self.module_paths.get("hook") {
            if std::path::Path::new(hook_path).exists() {
                let component_bytes = std::fs::read(hook_path)
                    .map_err(|e| BaseAppError::ExecutionError(
                        format!("failed to read hook.wasm: {e}")
                    ))?;
                let info = ComponentInfo {
                    name: "hook".to_string(),
                    path: hook_path.clone().into(),
                    component_type: ComponentType::Hook,
                    gas_limit: 1_000_000,
                };
                let _ = self.component_host.load_component("hook", &component_bytes, info);

                let result = self.component_host.execute_hook_pre(
                    "hook", height, timestamp, chain_id, None,
                ).map_err(|e| BaseAppError::ExecutionError(
                    format!("hook pre-execute failed: {e}")
                ))?;

                if !result.success {
                    return Err(BaseAppError::ExecutionError(
                        result.error.unwrap_or_else(|| "hook pre-execute failed".into())
                    ));
                }

                return Ok(Self::parse_component_events(&result.data));
            }
        }
        // No hook module loaded — no-op
        Ok(vec![])
    }

    /// Run post-execute hook. No-op if hook WASM module is not loaded.
    ///
    /// Called after all transactions in a block have been processed.
    /// Use for: reward distribution, settlement, validator set updates, etc.
    fn run_hook_post(
        &self,
        height: u64,
        timestamp: u64,
        chain_id: &str,
        tx_count: u32,
        total_gas: u64,
    ) -> Result<Vec<Event>> {
        if let Some(hook_path) = self.module_paths.get("hook") {
            if std::path::Path::new(hook_path).exists() {
                let component_bytes = std::fs::read(hook_path)
                    .map_err(|e| BaseAppError::ExecutionError(
                        format!("failed to read hook.wasm: {e}")
                    ))?;
                let info = ComponentInfo {
                    name: "hook".to_string(),
                    path: hook_path.clone().into(),
                    component_type: ComponentType::Hook,
                    gas_limit: 1_000_000,
                };
                let _ = self.component_host.load_component("hook", &component_bytes, info);

                let result = self.component_host.execute_hook_post(
                    "hook", height, timestamp, chain_id, None, tx_count, total_gas,
                ).map_err(|e| BaseAppError::ExecutionError(
                    format!("hook post-execute failed: {e}")
                ))?;

                if !result.success {
                    return Err(BaseAppError::ExecutionError(
                        result.error.unwrap_or_else(|| "hook post-execute failed".into())
                    ));
                }

                return Ok(Self::parse_component_events(&result.data));
            }
        }
        // No hook module loaded — no-op
        Ok(vec![])
    }

    // =========================================================================
    // Helper: parse events from ComponentResult
    // =========================================================================

    /// Extract Event list from a ComponentResult's data field.
    fn parse_component_events(data: &Option<serde_json::Value>) -> Vec<Event> {
        let mut events = Vec::new();
        if let Some(data) = data {
            if let Some(evt_array) = data.get("events").and_then(|e| e.as_array()) {
                for evt in evt_array {
                    events.push(Event {
                        r#type: evt.get("event_type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .to_string(),
                        attributes: evt.get("attributes")
                            .and_then(|a| a.as_array())
                            .map(|attrs| {
                                attrs.iter().map(|a| EventAttribute {
                                    key: a.get("key").and_then(|k| k.as_str()).unwrap_or("").to_string(),
                                    value: a.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    index: true,
                                }).collect()
                            })
                            .unwrap_or_default(),
                    });
                }
            }
        }
        events
    }

    // =========================================================================
    // Consensus interface — WIT-aligned pipeline
    // =========================================================================

    /// Execute a block of transactions using the WIT-aligned pipeline.
    ///
    /// Pipeline:
    /// 1. Pre-execute hook (no-op if hook WASM not loaded)
    /// 2. For each tx: validate → dispatch messages → increment sequence
    /// 3. Post-execute hook (no-op if hook WASM not loaded)
    /// 4. Compute state root
    ///
    /// Returns (state_root, tx_responses).
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

        // 1. Pre-execute hook
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
                        1,
                        format!("Transaction {i} failed: {e}"),
                        0,
                        0,
                    ));
                }
            }
        }

        // 3. Post-execute hook
        let _post_events = self.run_hook_post(
            height, timestamp, chain_id, txs.len() as u32, block_gas_used,
        )?;

        // 4. Compute state root (without committing)
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

    /// Process a single transaction through the validate → dispatch pipeline.
    ///
    /// Steps:
    /// 1. Validate: decode + signature verify + sequence check + extract messages
    /// 2. Dispatch: route each message to the appropriate module
    /// 3. Increment sequence on success
    fn process_transaction(
        &mut self,
        tx_bytes: &[u8],
        height: u64,
        timestamp: u64,
        chain_id: &str,
    ) -> Result<TxResponse> {
        // 2a. Validate (decode + auth + extract messages)
        let validated = self.validate_tx(tx_bytes, height, timestamp, chain_id)?;

        let mut total_gas_used = 0u64;
        let mut events = Vec::new();

        // 2b. Execute each message through module dispatch
        for msg in &validated.messages {
            let (gas, msg_events) = self.dispatch_message(
                &validated.sender, msg, height, timestamp, chain_id,
            )?;
            total_gas_used += gas;
            events.extend(msg_events);
        }

        // 2c. Increment sequence after successful execution
        self.increment_sequence(&validated.sender)?;

        Ok(TxResponse::success(
            "transaction executed successfully".to_string(),
            (total_gas_used + 10000) as i64,
            total_gas_used as i64,
            events,
        ))
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

    /// Clear the set of executed block heights.
    /// Used before block replay to allow re-execution of previously seen heights.
    pub fn clear_executed_heights(&mut self) {
        self.executed_heights.clear();
    }

    /// Export complete application state as a snapshot.
    pub fn export_snapshot(&self) -> Result<gridway_store::merkle::StateSnapshot> {
        let store = self.global_store.get_store();
        let store = store.lock().map_err(|e| BaseAppError::Store(format!("lock: {e}")))?;
        Ok(store.to_snapshot())
    }

    /// Import application state from a snapshot, rebuilding the trie.
    pub fn import_snapshot(&mut self, snapshot: &gridway_store::merkle::StateSnapshot) -> Result<()> {
        let store = self.global_store.get_store();
        let mut store = store.lock().map_err(|e| BaseAppError::Store(format!("lock: {e}")))?;
        store.from_snapshot(snapshot).map_err(|e| BaseAppError::Store(format!("import: {e}")))?;
        self.last_state_root = store.root_hash();
        Ok(())
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
    fn test_clear_executed_heights() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        let _ = app.execute_block(1, 1000, "test", &[]).unwrap();
        let _ = app.execute_block(2, 2000, "test", &[]).unwrap();

        // Heights are tracked
        let (root1, _) = app.execute_block(1, 1000, "test", &[]).unwrap();
        // Idempotent — returns current state root without re-executing

        // Clear and re-execute
        app.clear_executed_heights();
        let (root2, _) = app.execute_block(1, 1000, "test", &[]).unwrap();
        // Should still produce the same state root for empty blocks
        assert_eq!(root1, root2);
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
        let balance = app.get_balance("alice", "uatom").unwrap();
        assert_eq!(balance, 1000);
    }

    #[test]
    fn test_account_crud() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        let account = Account {
            public_key: "abcd1234".to_string(),
            sequence: 0,
        };
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
        let governance = app.module_governance();
        let modules = governance.list_modules().unwrap();
        assert_eq!(modules.len(), 0);
    }

    // ─── New pipeline tests ──────────────────────────────────────────────

    #[test]
    fn test_resolve_module() {
        let app = BaseApp::new("test-app".to_string()).unwrap();

        // Standard format
        assert_eq!(app.resolve_module("bank.MsgSend").unwrap(), "bank");
        assert_eq!(app.resolve_module("governance.MsgStoreCode").unwrap(), "governance");
        assert_eq!(app.resolve_module("staking.MsgDelegate").unwrap(), "staking");

        // Invalid formats
        assert!(app.resolve_module("NoModule").is_err());
        assert!(app.resolve_module("").is_err());
        assert!(app.resolve_module(".MsgSend").is_err());
    }

    #[test]
    fn test_validate_tx_rejects_invalid_json() {
        let app = BaseApp::new("test-app".to_string()).unwrap();
        let result = app.validate_tx(b"not json", 1, 1000, "test");
        assert!(result.is_err());
        assert!(matches!(result, Err(BaseAppError::InvalidTx(_))));
    }

    #[test]
    fn test_validate_tx_rejects_missing_fields() {
        let app = BaseApp::new("test-app".to_string()).unwrap();

        // Missing public_key
        let tx = serde_json::json!({"signature": "aa", "body": {}});
        let result = app.validate_tx(
            serde_json::to_vec(&tx).unwrap().as_slice(), 1, 1000, "test",
        );
        assert!(matches!(result, Err(BaseAppError::AuthError(_))));

        // Missing signature
        let tx = serde_json::json!({"public_key": "aa", "body": {}});
        let result = app.validate_tx(
            serde_json::to_vec(&tx).unwrap().as_slice(), 1, 1000, "test",
        );
        assert!(matches!(result, Err(BaseAppError::AuthError(_))));

        // Missing body
        let tx = serde_json::json!({"public_key": "aa", "signature": "bb"});
        let result = app.validate_tx(
            serde_json::to_vec(&tx).unwrap().as_slice(), 1, 1000, "test",
        );
        assert!(matches!(result, Err(BaseAppError::InvalidTx(_))));
    }

    #[test]
    fn test_dispatch_unknown_module() {
        let app = BaseApp::new("test-app".to_string()).unwrap();
        let msg = TxMessage {
            type_url: "unknown.MsgFoo".to_string(),
            data: "{}".to_string(),
        };
        let result = app.dispatch_message("sender", &msg, 1, 1000, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_hook_noop_without_wasm() {
        let app = BaseApp::new("test-app".to_string()).unwrap();

        // Hooks should be no-op when WASM files don't exist
        let pre_events = app.run_hook_pre(1, 1000, "test").unwrap();
        assert!(pre_events.is_empty());

        let post_events = app.run_hook_post(1, 1000, "test", 0, 0).unwrap();
        assert!(post_events.is_empty());
    }

    #[test]
    fn test_execute_block_with_hooks() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();

        // Execute empty block — hooks are no-op, should succeed
        let (root, responses) = app.execute_block(1, 1000, "test-chain", &[]).unwrap();
        assert_eq!(responses.len(), 0);

        // Second block
        let (root2, _) = app.execute_block(2, 2000, "test-chain", &[]).unwrap();
        // Same root since no state changed
        assert_eq!(root, root2);
    }

    #[test]
    fn test_parse_component_events() {
        // No data
        let events = BaseApp::parse_component_events(&None);
        assert!(events.is_empty());

        // Empty events
        let data = serde_json::json!({"events": []});
        let events = BaseApp::parse_component_events(&Some(data));
        assert!(events.is_empty());

        // With events
        let data = serde_json::json!({
            "events": [{
                "event_type": "transfer",
                "attributes": [
                    {"key": "sender", "value": "alice"},
                    {"key": "recipient", "value": "bob"}
                ]
            }]
        });
        let events = BaseApp::parse_component_events(&Some(data));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].r#type, "transfer");
        assert_eq!(events[0].attributes.len(), 2);
    }

    #[test]
    fn test_validated_tx_struct() {
        let tx = ValidatedTx {
            sender: "alice".to_string(),
            messages: vec![
                TxMessage {
                    type_url: "bank.MsgSend".to_string(),
                    data: r#"{"to_address":"bob","amount":"100"}"#.to_string(),
                },
            ],
            sequence: 0,
            gas_limit: 200_000,
        };
        assert_eq!(tx.sender, "alice");
        assert_eq!(tx.messages.len(), 1);
        assert_eq!(tx.messages[0].type_url, "bank.MsgSend");
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
