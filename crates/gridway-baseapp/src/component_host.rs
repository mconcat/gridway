//! WASI Component Host
//!
//! This module provides the WASI component runtime host that enables dynamic loading
//! and execution of WASM components using the component model and WIT interfaces.
//!
//! Supports three component types:
//! - **Module**: Domain-specific logic (bank, staking, gov, etc.)
//! - **Hook**: Block lifecycle hooks (pre/post execute)
//! - **Validator**: Transaction validation pipeline

use crate::component_bindings::hook::HookWorld;
use crate::component_bindings::validator::ValidatorWorld;
use crate::vfs::VirtualFilesystem;
// VFS-backed kvstore interface — bridging WASI modules to JMT state
use crate::component_bindings::module::gridway::framework::kvstore;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::{debug, error, info};
use wasmtime::component::*;
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::p2::{WasiCtx, WasiCtxBuilder, WasiView};

/// Component Host errors
#[derive(Error, Debug)]
pub enum ComponentHostError {
    #[error("Engine configuration error: {0}")]
    EngineConfig(String),

    #[error("Component compilation error: {0}")]
    ComponentCompilation(String),

    #[error("Component instantiation error: {0}")]
    ComponentInstantiation(String),

    #[error("Component execution error: {0}")]
    ComponentExecution(String),

    #[error("Component not found: {0}")]
    ComponentNotFound(String),

    #[error("Invalid component: {0}")]
    InvalidComponent(String),

    #[error("WASI setup error: {0}")]
    WasiSetup(String),

    #[error("Resource error: {0}")]
    ResourceError(String),
}

type Result<T> = std::result::Result<T, ComponentHostError>;

// ─── Resource Limits Configuration ───────────────────────────────────────────

/// Maximum allowed WASM binary size (default: 10 MB)
pub const DEFAULT_MAX_WASM_BINARY_SIZE: usize = 10 * 1024 * 1024;

/// Default maximum memory size per linear memory (256 MB)
pub const DEFAULT_MAX_MEMORY_SIZE: usize = 256 * 1024 * 1024;

/// Default maximum table elements per table
pub const DEFAULT_MAX_TABLE_ELEMENTS: usize = 10_000;

/// Default maximum instances per store
pub const DEFAULT_MAX_INSTANCES: usize = 10;

/// Default maximum tables per store
pub const DEFAULT_MAX_TABLES: usize = 10;

/// Default maximum memories per store
pub const DEFAULT_MAX_MEMORIES: usize = 10;

/// Resource limits configuration for WASM component execution.
///
/// Controls memory, table, instance, and binary size limits.
/// All values have production-safe defaults.
#[derive(Debug, Clone)]
pub struct ResourceConfig {
    /// Maximum memory size per linear memory (bytes)
    pub max_memory_size: usize,
    /// Maximum number of table elements per table
    pub max_table_elements: usize,
    /// Maximum number of instances per store
    pub max_instances: usize,
    /// Maximum number of tables per store
    pub max_tables: usize,
    /// Maximum number of memories per store
    pub max_memories: usize,
    /// Maximum WASM binary size in bytes
    pub max_wasm_binary_size: usize,
    /// Whether to trap (instead of returning -1) on memory/table grow failure
    pub trap_on_grow_failure: bool,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            max_memory_size: DEFAULT_MAX_MEMORY_SIZE,
            max_table_elements: DEFAULT_MAX_TABLE_ELEMENTS,
            max_instances: DEFAULT_MAX_INSTANCES,
            max_tables: DEFAULT_MAX_TABLES,
            max_memories: DEFAULT_MAX_MEMORIES,
            max_wasm_binary_size: DEFAULT_MAX_WASM_BINARY_SIZE,
            trap_on_grow_failure: true,
        }
    }
}

impl ResourceConfig {
    /// Build wasmtime StoreLimits from this configuration
    fn to_store_limits(&self) -> StoreLimits {
        StoreLimitsBuilder::new()
            .memory_size(self.max_memory_size)
            .table_elements(self.max_table_elements)
            .instances(self.max_instances)
            .tables(self.max_tables)
            .memories(self.max_memories)
            .trap_on_grow_failure(self.trap_on_grow_failure)
            .build()
    }
}

/// Component metadata
#[derive(Clone, Debug)]
pub struct ComponentInfo {
    /// Component name
    pub name: String,
    /// Component path
    pub path: PathBuf,
    /// Component type
    pub component_type: ComponentType,
    /// Gas limit for execution
    pub gas_limit: u64,
}

/// Component types
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentType {
    /// Domain module (bank, staking, gov, etc.)
    Module,
    /// Block execution hook (pre/post execute)
    Hook,
    /// Transaction validator (decode + validate + auth)
    Validator,
}

/// Execution result from a component
#[derive(Debug)]
pub struct ComponentResult {
    /// Success flag
    pub success: bool,
    /// Exit code (0 for success)
    pub exit_code: i32,
    /// Result data (usually JSON)
    pub data: Option<serde_json::Value>,
    /// Any error message
    pub error: Option<String>,
    /// Standard output (for compatibility with old WASI interface)
    pub stdout: Vec<u8>,
    /// Standard error (for compatibility with old WASI interface)
    pub stderr: Vec<u8>,
    /// Gas consumed
    pub gas_used: u64,
}

/// Component host state that implements WasiView.
/// Provides VFS-backed kvstore access to WASI modules.
pub struct ComponentState {
    table: wasmtime_wasi::ResourceTable,
    wasi: WasiCtx,
    #[allow(dead_code)]
    component_name: String,
    /// VFS reference for state access from WASI modules.
    /// When set, WASI modules can use the kvstore interface to access
    /// JMT-backed state through VFS namespace stores.
    vfs: Option<Arc<VirtualFilesystem>>,
    /// Resource limits for memory, tables, instances
    limits: StoreLimits,
}

impl wasmtime_wasi::p2::IoView for ComponentState {
    fn table(&mut self) -> &mut wasmtime_wasi::ResourceTable {
        &mut self.table
    }
}

impl WasiView for ComponentState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

// ─── VFS-backed kvstore host implementation ──────────────────────────────────
// This bridges the WIT kvstore interface to the VFS, allowing WASI modules
// to access JMT-backed blockchain state through standard KVStore operations.
// Path: WASM module → kvstore WIT interface → ComponentState → VFS → NamespacedStore → JMTStore

/// Host-side handle for a VFS-backed KVStore resource.
/// Maps to the WIT `resource store` in the kvstore interface.
/// Each handle represents access to a single namespace in the VFS.
pub struct VfsStoreHandle {
    /// The namespace this store provides access to (e.g., "bank", "auth")
    pub namespace: String,
}

impl kvstore::HostStore for ComponentState {
    fn get(
        &mut self,
        store_handle: wasmtime::component::Resource<kvstore::Store>,
        key: Vec<u8>,
    ) -> Option<Vec<u8>> {
        let vfs = self.vfs.as_ref()?;
        let vfs_handle =
            wasmtime::component::Resource::<VfsStoreHandle>::new_own(store_handle.rep());
        let handle = self.table.get(&vfs_handle).ok()?;
        let namespace = handle.namespace.clone();
        vfs.read_key(&namespace, &key).ok()?
    }

    fn set(
        &mut self,
        store_handle: wasmtime::component::Resource<kvstore::Store>,
        key: Vec<u8>,
        value: Vec<u8>,
    ) {
        if let Some(vfs) = self.vfs.as_ref() {
            let vfs_handle =
                wasmtime::component::Resource::<VfsStoreHandle>::new_own(store_handle.rep());
            match self.table.get(&vfs_handle) {
                Ok(handle) => {
                    let namespace = handle.namespace.clone();
                    if let Err(e) = vfs.write_key(&namespace, &key, &value) {
                        error!("kvstore::set failed for {namespace}: {e}");
                    }
                }
                Err(e) => {
                    error!(rep = store_handle.rep(), "kvstore::set table lookup failed: {e}");
                }
            }
            // Prevent drop from removing the resource
            std::mem::forget(vfs_handle);
        } else {
            error!("kvstore::set — VFS not available!");
        }
    }

    fn delete(
        &mut self,
        store_handle: wasmtime::component::Resource<kvstore::Store>,
        key: Vec<u8>,
    ) {
        if let Some(vfs) = self.vfs.as_ref() {
            let vfs_handle =
                wasmtime::component::Resource::<VfsStoreHandle>::new_own(store_handle.rep());
            if let Ok(handle) = self.table.get(&vfs_handle) {
                let namespace = handle.namespace.clone();
                if let Err(e) = vfs.delete_key(&namespace, &key) {
                    tracing::error!("kvstore delete failed for {namespace}:: {e}");
                }
            }
        }
    }

    fn has(
        &mut self,
        store_handle: wasmtime::component::Resource<kvstore::Store>,
        key: Vec<u8>,
    ) -> bool {
        self.vfs
            .as_ref()
            .and_then(|vfs| {
                let vfs_handle =
                    wasmtime::component::Resource::<VfsStoreHandle>::new_own(store_handle.rep());
                let handle = self.table.get(&vfs_handle).ok()?;
                vfs.has_key(&handle.namespace, &key).ok()
            })
            .unwrap_or(false)
    }

    fn range(
        &mut self,
        store_handle: wasmtime::component::Resource<kvstore::Store>,
        start: Option<Vec<u8>>,
        end: Option<Vec<u8>>,
        limit: u32,
    ) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.vfs
            .as_ref()
            .and_then(|vfs| {
                let vfs_handle =
                    wasmtime::component::Resource::<VfsStoreHandle>::new_own(store_handle.rep());
                let handle = self.table.get(&vfs_handle).ok()?;
                vfs.range_keys(
                    &handle.namespace,
                    start.as_deref(),
                    end.as_deref(),
                    limit,
                )
                .ok()
            })
            .unwrap_or_default()
    }

    fn drop(
        &mut self,
        rep: wasmtime::component::Resource<kvstore::Store>,
    ) -> wasmtime::Result<()> {
        let vfs_handle = wasmtime::component::Resource::<VfsStoreHandle>::new_own(rep.rep());
        let _ = self.table.delete(vfs_handle);
        Ok(())
    }
}

impl kvstore::Host for ComponentState {
    fn open_store(
        &mut self,
        name: String,
    ) -> std::result::Result<wasmtime::component::Resource<kvstore::Store>, String> {
        let vfs = self
            .vfs
            .as_ref()
            .ok_or_else(|| "VFS not available".to_string())?;

        // Validate that the namespace exists in VFS
        if !vfs.has_namespace(&name) {
            return Err(format!("Store '{name}' not found"));
        }

        // Create a VfsStoreHandle and push to resource table
        let handle = VfsStoreHandle {
            namespace: name.clone(),
        };
        let resource = self
            .table
            .push(handle)
            .map_err(|e| format!("Failed to create store resource:: {e}"))?;

        // Convert VfsStoreHandle resource to kvstore::Store resource
        debug!(namespace = %name, rep = resource.rep(), "kvstore::open_store created handle");
        Ok(wasmtime::component::Resource::<kvstore::Store>::new_own(
            resource.rep(),
        ))
    }
}

/// WASI Component Host
pub struct ComponentHost {
    /// Wasmtime engine
    engine: Engine,
    /// Loaded components
    components: Arc<Mutex<HashMap<String, Component>>>,
    /// Component metadata
    component_info: Arc<Mutex<HashMap<String, ComponentInfo>>>,
    /// Default gas limit
    default_gas_limit: u64,
    /// Resource limits configuration
    resource_config: ResourceConfig,
    /// VFS reference for state access bridging.
    /// When set, WASI components can access JMT-backed state through
    /// the kvstore WIT interface via VFS namespace stores.
    vfs: Option<Arc<VirtualFilesystem>>,
}

impl ComponentHost {
    /// Create a new component host with default configuration
    pub fn new() -> Result<Self> {
        Self::with_resource_config(ResourceConfig::default())
    }

    /// Create a new component host with custom resource configuration
    pub fn with_resource_config(resource_config: ResourceConfig) -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.async_support(false);
        Self::with_config(config, resource_config)
    }

    /// Create a new component host with custom wasmtime and resource configuration
    pub fn with_config(
        mut config: Config,
        resource_config: ResourceConfig,
    ) -> Result<Self> {
        // Ensure component model is enabled
        config.wasm_component_model(true);

        // Configure engine for security and performance
        config.wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Enable);
        config.wasm_multi_memory(true);
        config.wasm_memory64(false); // Disable 64-bit memory for security
        config.consume_fuel(true); // Enable fuel metering for gas tracking

        // Stack size limit for WASM execution (1 MB)
        config.max_wasm_stack(1024 * 1024);

        let engine =
            Engine::new(&config).map_err(|e| ComponentHostError::EngineConfig(e.to_string()))?;

        info!(
            "Component host initialized with secure configuration              (memory_limit={}MB, max_binary={}MB, fuel=enabled)",
            resource_config.max_memory_size / (1024 * 1024),
            resource_config.max_wasm_binary_size / (1024 * 1024),
        );

        Ok(Self {
            engine,
            components: Arc::new(Mutex::new(HashMap::new())),
            component_info: Arc::new(Mutex::new(HashMap::new())),
            default_gas_limit: 10_000_000, // 10 million units
            resource_config,
            vfs: None,
        })
    }

    /// Get the current resource configuration
    pub fn resource_config(&self) -> &ResourceConfig {
        &self.resource_config
    }

    /// Set the VFS reference for state access bridging between WASI modules and the store
    pub fn set_vfs(&mut self, vfs: Arc<VirtualFilesystem>) {
        self.vfs = Some(vfs);
    }

    /// Get the VFS reference
    pub fn vfs(&self) -> Option<&Arc<VirtualFilesystem>> {
        self.vfs.as_ref()
    }

    /// Load a component from bytes
    pub fn load_component(&self, name: &str, bytes: &[u8], info: ComponentInfo) -> Result<()> {
        debug!("Loading component: {} ({} bytes)", name, bytes.len());

        // Enforce WASM binary size limit
        if bytes.len() > self.resource_config.max_wasm_binary_size {
            return Err(ComponentHostError::ResourceError(format!(
                "WASM binary size {} bytes exceeds maximum allowed {} bytes",
                bytes.len(),
                self.resource_config.max_wasm_binary_size,
            )));
        }

        // Compile the component
        let component = Component::new(&self.engine, bytes)
            .map_err(|e| ComponentHostError::ComponentCompilation(e.to_string()))?;

        // Store component and metadata
        {
            let mut components = self.components.lock().map_err(|e| {
                ComponentHostError::ComponentCompilation(format!("Lock poisoned: {e}"))
            })?;
            components.insert(name.to_string(), component);
        }

        {
            let mut component_info = self.component_info.lock().map_err(|e| {
                ComponentHostError::ComponentCompilation(format!("Lock poisoned: {e}"))
            })?;
            component_info.insert(name.to_string(), info);
        }

        info!("Component {} loaded successfully", name);
        Ok(())
    }

    /// Create a store with WASI context, fuel limit, and resource limits for a component
    fn create_store(&self, component_name: &str) -> Result<Store<ComponentState>> {
        let wasi = WasiCtxBuilder::new().build();
        let state = ComponentState {
            table: wasmtime_wasi::ResourceTable::new(),
            wasi,
            component_name: component_name.to_string(),
            vfs: self.vfs.clone(),
            limits: self.resource_config.to_store_limits(),
        };
        let mut store = Store::new(&self.engine, state);

        // Apply resource limiter for memory, tables, instances
        store.limiter(|state| &mut state.limits);

        let gas_limit = {
            let info = self.component_info.lock().map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Lock poisoned: {e}"))
            })?;
            info.get(component_name)
                .map(|i| i.gas_limit)
                .unwrap_or(self.default_gas_limit)
        };
        store
            .set_fuel(gas_limit)
            .map_err(|e| ComponentHostError::ComponentExecution(e.to_string()))?;

        Ok(store)
    }

    /// Create a linker with WASI and kvstore bindings
    fn create_linker(&self) -> Result<Linker<ComponentState>> {
        let mut linker: Linker<ComponentState> = Linker::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
            .map_err(|e| ComponentHostError::WasiSetup(e.to_string()))?;
        kvstore::add_to_linker::<ComponentState, wasmtime::component::HasSelf<ComponentState>>(
            &mut linker,
            |state| state,
        )
        .map_err(|e| {
            ComponentHostError::ComponentInstantiation(format!("Failed to add kvstore: {e}"))
        })?;
        Ok(linker)
    }

    /// Get a loaded component by name
    fn get_component(&self, name: &str) -> Result<Component> {
        let components = self.components.lock().map_err(|e| {
            ComponentHostError::ComponentExecution(format!("Lock poisoned: {e}"))
        })?;
        components
            .get(name)
            .ok_or_else(|| ComponentHostError::ComponentNotFound(name.to_string()))
            .cloned()
    }

    /// Get fuel-based gas consumption from a store
    fn fuel_gas_used(&self, store: &mut Store<ComponentState>, component_name: &str) -> u64 {
        let gas_limit = {
            let info = self.component_info.lock().ok();
            info.and_then(|i| {
                i.get(component_name)
                    .map(|v| v.gas_limit)
            })
            .unwrap_or(self.default_gas_limit)
        };
        gas_limit.saturating_sub(store.get_fuel().unwrap_or(0))
    }

    /// Convert component events to JSON
    fn events_to_json(
        events: &[(String, Vec<(String, String)>)],
    ) -> Vec<serde_json::Value> {
        events
            .iter()
            .map(|(event_type, attributes)| {
                let attrs: Vec<serde_json::Value> = attributes
                    .iter()
                    .map(|(key, value)| {
                        serde_json::json!({ "key": key, "value": value })
                    })
                    .collect();
                serde_json::json!({
                    "event_type": event_type,
                    "attributes": attrs
                })
            })
            .collect()
    }

    // =========================================================================
    // Hook execution (replaces begin-blocker / end-blocker)
    // =========================================================================

    /// Execute the pre-execute hook (called before TX processing).
    pub fn execute_hook_pre(
        &self,
        component_name: &str,
        height: u64,
        timestamp: u64,
        chain_id: &str,
        proposer: Option<Vec<u8>>,
    ) -> Result<ComponentResult> {
        debug!("Executing hook pre-execute: {}", component_name);

        let component = self.get_component(component_name)?;
        let mut store = self.create_store(component_name)?;
        let linker = self.create_linker()?;

        let bindings = HookWorld::instantiate(&mut store, &component, &linker)
            .map_err(|e| ComponentHostError::ComponentInstantiation(e.to_string()))?;

        let ctx = crate::component_bindings::hook::exports::gridway::framework::hook::BlockContext {
            height,
            timestamp,
            chain_id: chain_id.to_string(),
            proposer,
        };

        let response = bindings
            .gridway_framework_hook()
            .call_pre_execute(&mut store, &ctx)
            .map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Hook pre-execute failed: {e}"))
            })?;

        let gas_used = self.fuel_gas_used(&mut store, component_name);

        let events: Vec<(String, Vec<(String, String)>)> = response
            .events
            .iter()
            .map(|e| {
                (
                    e.event_type.clone(),
                    e.attributes.iter().map(|a| (a.key.clone(), a.value.clone())).collect(),
                )
            })
            .collect();
        let events_data = Self::events_to_json(&events);

        Ok(ComponentResult {
            success: response.success,
            exit_code: if response.success { 0 } else { 1 },
            data: Some(serde_json::json!({ "events": events_data })),
            error: response.error,
            stdout: serde_json::to_string(&events_data).unwrap_or_default().into_bytes(),
            stderr: Vec::new(),
            gas_used,
        })
    }

    /// Execute the post-execute hook (called after TX processing).
    pub fn execute_hook_post(
        &self,
        component_name: &str,
        height: u64,
        timestamp: u64,
        chain_id: &str,
        proposer: Option<Vec<u8>>,
        tx_count: u32,
        total_gas: u64,
    ) -> Result<ComponentResult> {
        debug!("Executing hook post-execute: {}", component_name);

        let component = self.get_component(component_name)?;
        let mut store = self.create_store(component_name)?;
        let linker = self.create_linker()?;

        let bindings = HookWorld::instantiate(&mut store, &component, &linker)
            .map_err(|e| ComponentHostError::ComponentInstantiation(e.to_string()))?;

        let ctx = crate::component_bindings::hook::exports::gridway::framework::hook::BlockContext {
            height,
            timestamp,
            chain_id: chain_id.to_string(),
            proposer,
        };

        let response = bindings
            .gridway_framework_hook()
            .call_post_execute(&mut store, &ctx, tx_count, total_gas)
            .map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Hook post-execute failed: {e}"))
            })?;

        let gas_used = self.fuel_gas_used(&mut store, component_name);

        let events: Vec<(String, Vec<(String, String)>)> = response
            .events
            .iter()
            .map(|e| {
                (
                    e.event_type.clone(),
                    e.attributes.iter().map(|a| (a.key.clone(), a.value.clone())).collect(),
                )
            })
            .collect();
        let events_data = Self::events_to_json(&events);

        Ok(ComponentResult {
            success: response.success,
            exit_code: if response.success { 0 } else { 1 },
            data: Some(serde_json::json!({ "events": events_data })),
            error: response.error,
            stdout: serde_json::to_string(&events_data).unwrap_or_default().into_bytes(),
            stderr: Vec::new(),
            gas_used,
        })
    }

    // =========================================================================
    // Validator execution (replaces ante-handler + tx-decoder)
    // =========================================================================

    /// Validate a raw transaction through the WASM validator component.
    pub fn execute_validator(
        &self,
        component_name: &str,
        height: u64,
        timestamp: u64,
        chain_id: &str,
        raw_tx: &[u8],
    ) -> Result<ComponentResult> {
        debug!("Executing validator: {}", component_name);

        let component = self.get_component(component_name)?;
        let mut store = self.create_store(component_name)?;
        let linker = self.create_linker()?;

        let bindings = ValidatorWorld::instantiate(&mut store, &component, &linker)
            .map_err(|e| ComponentHostError::ComponentInstantiation(e.to_string()))?;

        let ctx = crate::component_bindings::validator::exports::gridway::framework::validator::TxContext {
            height,
            timestamp,
            chain_id: chain_id.to_string(),
        };

        let response = bindings
            .gridway_framework_validator()
            .call_validate(&mut store, &ctx, raw_tx)
            .map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Validator execution failed: {e}"))
            })?;

        let gas_used = self.fuel_gas_used(&mut store, component_name);

        // Convert validated TX to JSON data
        let tx_data = response.tx.as_ref().map(|tx| {
            let messages: Vec<serde_json::Value> = tx
                .messages
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "type_url": m.type_url,
                        "data": m.data,
                    })
                })
                .collect();
            serde_json::json!({
                "sender": tx.sender,
                "messages": messages,
                "sequence": tx.sequence,
                "gas_limit": tx.gas_limit,
            })
        });

        let events: Vec<(String, Vec<(String, String)>)> = response
            .events
            .iter()
            .map(|e| {
                (
                    e.event_type.clone(),
                    e.attributes.iter().map(|a| (a.key.clone(), a.value.clone())).collect(),
                )
            })
            .collect();
        let events_data = Self::events_to_json(&events);

        Ok(ComponentResult {
            success: response.valid,
            exit_code: if response.valid { 0 } else { 1 },
            data: Some(serde_json::json!({
                "tx": tx_data,
                "gas_used": response.gas_used,
                "events": events_data,
            })),
            error: response.error,
            stdout: Vec::new(),
            stderr: Vec::new(),
            gas_used,
        })
    }

    // =========================================================================
    // Module execution (unchanged)
    // =========================================================================

    /// Execute a module component (e.g., bank, staking)
    pub fn execute_module(
        &self,
        module_name: &str,
        block_height: u64,
        block_time: u64,
        chain_id: &str,
        msg_type_url: &str,
        msg_data: &str,
        msg_sender: &str,
        gas_limit: u64,
    ) -> Result<ComponentResult> {
        debug!("Executing module component: {}", module_name);

        let component = self.get_component(module_name)?;

        // Create store with specific gas limit and resource limits
        let wasi = WasiCtxBuilder::new().build();
        let state = ComponentState {
            table: wasmtime_wasi::ResourceTable::new(),
            wasi,
            component_name: module_name.to_string(),
            vfs: self.vfs.clone(),
            limits: self.resource_config.to_store_limits(),
        };
        let mut store = Store::new(&self.engine, state);

        // Apply resource limiter for memory, tables, instances
        store.limiter(|state| &mut state.limits);

        store
            .set_fuel(gas_limit)
            .map_err(|e| ComponentHostError::ComponentExecution(format!("Failed to set fuel: {e}")))?;

        let linker = self.create_linker()?;

        // Instantiate the component with module-world bindings
        let bindings = crate::component_bindings::module::ModuleWorld::instantiate(
            &mut store, &component, &linker,
        )
        .map_err(|e| ComponentHostError::ComponentInstantiation(e.to_string()))?;

        // Create module context and message
        let context = crate::component_bindings::module::exports::gridway::framework::module::ModuleContext {
            block_height,
            block_time,
            chain_id: chain_id.to_string(),
            simulate: false,
        };

        let message = crate::component_bindings::module::exports::gridway::framework::module::Message {
            type_url: msg_type_url.to_string(),
            data: msg_data.to_string(),
            sender: msg_sender.to_string(),
        };

        // Execute the component
        let response = bindings
            .gridway_framework_module()
            .call_handle(&mut store, &context, &message)
            .map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Module execution failed: {e}"))
            })?;

        // Get remaining fuel for gas tracking
        let gas_used = gas_limit - store.get_fuel().unwrap_or(0);
        // Use the module-reported gas if available, otherwise use fuel-based measurement
        let final_gas_used = if response.gas_used > 0 { response.gas_used } else { gas_used };

        // Convert events to JSON for data field
        let events_data: Vec<serde_json::Value> = response
            .events
            .iter()
            .map(|event| {
                let attributes: Vec<serde_json::Value> = event
                    .attributes
                    .iter()
                    .map(|attr| {
                        serde_json::json!({
                            "key": attr.key,
                            "value": attr.value
                        })
                    })
                    .collect();
                serde_json::json!({
                    "event_type": event.event_type,
                    "attributes": attributes
                })
            })
            .collect();

        let error_stderr = if let Some(ref error) = response.error {
            error.as_bytes().to_vec()
        } else {
            Vec::new()
        };

        Ok(ComponentResult {
            success: response.success,
            exit_code: if response.success { 0 } else { 1 },
            data: Some(serde_json::json!({"events": events_data})),
            error: response.error,
            stdout: serde_json::to_string(&events_data).unwrap_or_default().as_bytes().to_vec(),
            stderr: error_stderr,
            gas_used: final_gas_used,
        })
    }

    /// Get the gas consumed from the last execution
    pub fn get_gas_consumed(&self, store: &mut Store<ComponentState>) -> u64 {
        store.get_fuel().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_host_creation() {
        let host = ComponentHost::new().unwrap();
        assert!(host.components.lock().unwrap().is_empty());
    }

    #[test]
    fn test_component_host_with_vfs() {
        let mut host = ComponentHost::new().unwrap();

        let vfs = Arc::new(crate::vfs::VirtualFilesystem::new());
        host.set_vfs(vfs.clone());

        assert!(host.vfs().is_some());
    }

    #[test]
    fn test_kvstore_host_via_vfs() {
        use crate::vfs::{Capability, VirtualFilesystem};
        use std::path::PathBuf;

        // Set up VFS with a mounted MemStore
        let vfs = Arc::new(VirtualFilesystem::new());
        let bank_store: Arc<Mutex<dyn gridway_store::KVStore>> =
            Arc::new(Mutex::new(gridway_store::MemStore::new()));
        vfs.mount_store("bank".to_string(), bank_store).unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/bank")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/bank")))
            .unwrap();

        // Create ComponentState with VFS
        let wasi = WasiCtxBuilder::new().build();
        let mut state = ComponentState {
            table: wasmtime_wasi::ResourceTable::new(),
            wasi,
            component_name: "test".to_string(),
            vfs: Some(vfs),
            limits: StoreLimitsBuilder::new().build(),
        };

        // Test open_store
        let store_resource =
            kvstore::Host::open_store(&mut state, "bank".to_string()).unwrap();
        let rep = store_resource.rep();

        // Test set
        kvstore::HostStore::set(
            &mut state,
            wasmtime::component::Resource::new_own(rep),
            b"balance_alice".to_vec(),
            b"1000".to_vec(),
        );

        // Test get
        let value = kvstore::HostStore::get(
            &mut state,
            wasmtime::component::Resource::new_own(rep),
            b"balance_alice".to_vec(),
        );
        assert_eq!(value, Some(b"1000".to_vec()));

        // Test has
        let exists = kvstore::HostStore::has(
            &mut state,
            wasmtime::component::Resource::new_own(rep),
            b"balance_alice".to_vec(),
        );
        assert!(exists);

        // Test get nonexistent key
        let value = kvstore::HostStore::get(
            &mut state,
            wasmtime::component::Resource::new_own(rep),
            b"nonexistent".to_vec(),
        );
        assert_eq!(value, None);

        // Test has nonexistent key
        let exists = kvstore::HostStore::has(
            &mut state,
            wasmtime::component::Resource::new_own(rep),
            b"nonexistent".to_vec(),
        );
        assert!(!exists);

        // Test delete
        kvstore::HostStore::delete(
            &mut state,
            wasmtime::component::Resource::new_own(rep),
            b"balance_alice".to_vec(),
        );
        let value = kvstore::HostStore::get(
            &mut state,
            wasmtime::component::Resource::new_own(rep),
            b"balance_alice".to_vec(),
        );
        assert_eq!(value, None);

        // Test drop (cleanup)
        kvstore::HostStore::drop(
            &mut state,
            wasmtime::component::Resource::new_own(rep),
        )
        .unwrap();
    }

    #[test]
    fn test_kvstore_host_range_query() {
        use crate::vfs::{Capability, VirtualFilesystem};
        use std::path::PathBuf;

        let vfs = Arc::new(VirtualFilesystem::new());
        let bank_store: Arc<Mutex<dyn gridway_store::KVStore>> =
            Arc::new(Mutex::new(gridway_store::MemStore::new()));
        vfs.mount_store("bank".to_string(), bank_store).unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/bank")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/bank")))
            .unwrap();

        let wasi = WasiCtxBuilder::new().build();
        let mut state = ComponentState {
            table: wasmtime_wasi::ResourceTable::new(),
            wasi,
            component_name: "test".to_string(),
            vfs: Some(vfs),
            limits: StoreLimitsBuilder::new().build(),
        };

        let store_resource =
            kvstore::Host::open_store(&mut state, "bank".to_string()).unwrap();
        let rep = store_resource.rep();

        // Write multiple keys
        for (key, val) in [
            ("balance_alice", "1000"),
            ("balance_bob", "2000"),
            ("balance_carol", "3000"),
            ("supply_ugridway", "6000"),
        ] {
            kvstore::HostStore::set(
                &mut state,
                wasmtime::component::Resource::new_own(rep),
                key.as_bytes().to_vec(),
                val.as_bytes().to_vec(),
            );
        }

        // Range query with prefix
        let results = kvstore::HostStore::range(
            &mut state,
            wasmtime::component::Resource::new_own(rep),
            Some(b"balance_".to_vec()),
            None,
            10,
        );
        assert_eq!(results.len(), 3, "expected 3 balance_ entries");

        // Range query with limit
        let results = kvstore::HostStore::range(
            &mut state,
            wasmtime::component::Resource::new_own(rep),
            Some(b"balance_".to_vec()),
            None,
            2,
        );
        assert_eq!(results.len(), 2, "expected 2 entries with limit=2");
    }

    #[test]
    fn test_kvstore_host_open_nonexistent_store() {
        use crate::vfs::VirtualFilesystem;

        let vfs = Arc::new(VirtualFilesystem::new());
        // No stores mounted

        let wasi = WasiCtxBuilder::new().build();
        let mut state = ComponentState {
            table: wasmtime_wasi::ResourceTable::new(),
            wasi,
            component_name: "test".to_string(),
            vfs: Some(vfs),
            limits: StoreLimitsBuilder::new().build(),
        };

        // Opening a nonexistent store should fail
        let result = kvstore::Host::open_store(&mut state, "nonexistent".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_kvstore_host_no_vfs() {
        let wasi = WasiCtxBuilder::new().build();
        let mut state = ComponentState {
            table: wasmtime_wasi::ResourceTable::new(),
            wasi,
            component_name: "test".to_string(),
            vfs: None, // No VFS
            limits: StoreLimitsBuilder::new().build(),
        };

        // Opening a store without VFS should fail
        let result = kvstore::Host::open_store(&mut state, "bank".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("VFS not available"));
    }

    // ─── Resource Limit Tests ────────────────────────────────────────────────

    #[test]
    fn test_default_resource_config() {
        let config = ResourceConfig::default();
        assert_eq!(config.max_memory_size, DEFAULT_MAX_MEMORY_SIZE);
        assert_eq!(config.max_wasm_binary_size, DEFAULT_MAX_WASM_BINARY_SIZE);
        assert_eq!(config.max_instances, DEFAULT_MAX_INSTANCES);
        assert_eq!(config.max_tables, DEFAULT_MAX_TABLES);
        assert_eq!(config.max_memories, DEFAULT_MAX_MEMORIES);
        assert_eq!(config.max_table_elements, DEFAULT_MAX_TABLE_ELEMENTS);
        assert!(config.trap_on_grow_failure);
    }

    #[test]
    fn test_custom_resource_config() {
        let config = ResourceConfig {
            max_memory_size: 128 * 1024 * 1024,
            max_wasm_binary_size: 5 * 1024 * 1024,
            max_instances: 5,
            max_tables: 5,
            max_memories: 5,
            max_table_elements: 5000,
            trap_on_grow_failure: false,
        };
        let host = ComponentHost::with_resource_config(config.clone()).unwrap();
        assert_eq!(host.resource_config().max_memory_size, 128 * 1024 * 1024);
        assert_eq!(host.resource_config().max_wasm_binary_size, 5 * 1024 * 1024);
    }

    #[test]
    fn test_component_host_rejects_oversized_binary() {
        let config = ResourceConfig {
            max_wasm_binary_size: 100, // Very small limit
            ..ResourceConfig::default()
        };
        let host = ComponentHost::with_resource_config(config).unwrap();

        // Create a binary larger than the limit
        let oversized_bytes = vec![0u8; 200];
        let info = ComponentInfo {
            name: "oversized".to_string(),
            path: std::path::PathBuf::from("/tmp/oversized.wasm"),
            component_type: ComponentType::Module,
            gas_limit: 1_000_000,
        };

        let result = host.load_component("oversized", &oversized_bytes, info);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("exceeds maximum"),
            "Expected 'exceeds maximum' in error, got: {err}"
        );
    }

    #[test]
    fn test_resource_config_to_store_limits() {
        // Verify that ResourceConfig properly creates StoreLimits
        let config = ResourceConfig::default();
        let _limits = config.to_store_limits();
        // StoreLimits doesn't expose fields, but construction should succeed
    }

    #[test]
    fn test_component_host_with_resource_config() {
        let config = ResourceConfig {
            max_memory_size: 64 * 1024 * 1024, // 64MB
            ..ResourceConfig::default()
        };
        let host = ComponentHost::with_resource_config(config).unwrap();
        assert!(host.components.lock().unwrap().is_empty());
        assert_eq!(host.resource_config().max_memory_size, 64 * 1024 * 1024);
    }
}
