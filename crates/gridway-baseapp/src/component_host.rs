//! WASI Component Host
//!
//! This module provides the WASI component runtime host that enables dynamic loading
//! and execution of WASM components using the component model and WIT interfaces.

use crate::component_bindings::ante_handler::AnteHandlerWorld;
use crate::component_bindings::tx_decoder::TxDecoderWorld;
use crate::vfs::VirtualFilesystem;
// VFS-backed kvstore interface — bridging WASI modules to JMT state
use crate::component_bindings::ante_handler::gridway::framework::kvstore;
use hex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::{debug, error, info};
use wasmtime::component::*;
use wasmtime::{Config, Engine, Store};
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

/// Component metadata
#[derive(Clone, Debug)]
pub struct ComponentInfo {
    /// Component name
    pub name: String,
    /// Component path
    pub path: PathBuf,
    /// Component type (ante-handler, tx-decoder, etc.)
    pub component_type: ComponentType,
    /// Gas limit for execution
    pub gas_limit: u64,
}

/// Component types
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentType {
    AnteHandler,
    BeginBlocker,
    EndBlocker,
    TxDecoder,
    Module, // Generic application module
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
        tracing::info!(
            key = String::from_utf8_lossy(&key).as_ref(),
            value = String::from_utf8_lossy(&value).as_ref(),
            rep = store_handle.rep(),
            "kvstore::set called from WASM"
        );
        if let Some(vfs) = self.vfs.as_ref() {
            let vfs_handle =
                wasmtime::component::Resource::<VfsStoreHandle>::new_own(store_handle.rep());
            match self.table.get(&vfs_handle) {
                Ok(handle) => {
                    let namespace = handle.namespace.clone();
                    tracing::info!(namespace = %namespace, "kvstore::set writing to VFS");
                    if let Err(e) = vfs.write_key(&namespace, &key, &value) {
                        tracing::error!("kvstore set failed for {namespace}:: {e}");
                    } else {
                        tracing::info!(namespace = %namespace, "kvstore::set SUCCESS");
                    }
                }
                Err(e) => {
                    tracing::error!(rep = store_handle.rep(), "kvstore::set table lookup failed: {e}");
                }
            }
            // Prevent drop from removing the resource
            std::mem::forget(vfs_handle);
        } else {
            tracing::error!("kvstore::set — VFS not available!");
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
        tracing::info!(namespace = %name, rep = resource.rep(), "kvstore::open_store created handle");
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
    /// VFS reference for state access bridging.
    /// When set, WASI components can access JMT-backed state through
    /// the kvstore WIT interface via VFS namespace stores.
    vfs: Option<Arc<VirtualFilesystem>>,
}

impl ComponentHost {
    /// Create a new component host with default configuration
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.async_support(false);
        Self::with_config(config)
    }

    /// Create a new component host with custom configuration
    pub fn with_config(
        mut config: Config,
    ) -> Result<Self> {
        // Ensure component model is enabled
        config.wasm_component_model(true);

        // Configure engine for security and performance
        config.wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Enable);
        config.wasm_multi_memory(true);
        config.wasm_memory64(false); // Disable 64-bit memory for security
        config.consume_fuel(true); // Enable fuel metering for gas tracking

        let engine =
            Engine::new(&config).map_err(|e| ComponentHostError::EngineConfig(e.to_string()))?;

        info!("Component host initialized with secure configuration");

        Ok(Self {
            engine,
            components: Arc::new(Mutex::new(HashMap::new())),
            component_info: Arc::new(Mutex::new(HashMap::new())),
            default_gas_limit: 10_000_000, // 10 million units
            vfs: None,
        })
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
        debug!("Loading component: {}", name);

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

    /// Execute an ante-handler component
    #[allow(clippy::too_many_arguments)]
    pub fn execute_ante_handler(
        &self,
        component_name: &str,
        block_height: u64,
        block_time: u64,
        chain_id: &str,
        gas_limit: u64,
        sequence: u64,
        tx_bytes: Vec<u8>,
    ) -> Result<ComponentResult> {
        debug!("Executing ante-handler component: {}", component_name);

        // Get the component
        let component = {
            let components = self.components.lock().map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Lock poisoned: {e}"))
            })?;
            components
                .get(component_name)
                .ok_or_else(|| ComponentHostError::ComponentNotFound(component_name.to_string()))?
                .clone()
        };

        // Create WASI context
        let wasi = WasiCtxBuilder::new().build();

        let state = ComponentState {
            table: wasmtime_wasi::ResourceTable::new(),
            wasi,
            component_name: component_name.to_string(),
            vfs: self.vfs.clone(),
        };

        let mut store = Store::new(&self.engine, state);

        // Set fuel limit
        let component_gas_limit = {
            let info = self.component_info.lock().map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Lock poisoned: {e}"))
            })?;
            info.get(component_name)
                .map(|i| i.gas_limit)
                .unwrap_or(self.default_gas_limit)
        };
        store
            .set_fuel(component_gas_limit)
            .map_err(|e| ComponentHostError::ComponentExecution(e.to_string()))?;

        // Create linker and add WASI
        let mut linker: Linker<ComponentState> = Linker::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
            .map_err(|e| ComponentHostError::WasiSetup(e.to_string()))?;

        // Add VFS-backed kvstore interface
        kvstore::add_to_linker::<ComponentState, wasmtime::component::HasSelf<ComponentState>>(&mut linker, |state| state)
            .map_err(|e| ComponentHostError::ComponentInstantiation(format!("Failed to add kvstore: {e}")))?;

        // Instantiate the component with bindings
        let bindings = AnteHandlerWorld::instantiate(&mut store, &component, &linker)
            .map_err(|e| ComponentHostError::ComponentInstantiation(e.to_string()))?;

        // Create the context
        let context = crate::component_bindings::ante_handler::exports::gridway::framework::ante_handler::TxContext {
            block_height,
            block_time,
            chain_id: chain_id.to_string(),
            gas_limit,
            sequence,
            simulate: false,
            is_check_tx: false,
            is_recheck: false,
        };

        // Execute the component
        let response = bindings
            .gridway_framework_ante_handler()
            .call_ante_handle(&mut store, &context, &tx_bytes)
            .map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Component execution failed: {e}"))
            })?;

        // Get remaining fuel for gas tracking
        let gas_used = component_gas_limit - store.get_fuel().unwrap_or(0);

        // Convert events to JSON for stdout
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
        let events_json = serde_json::to_string(&events_data).unwrap_or_default();

        let error_stderr = if let Some(ref error) = response.error {
            error.as_bytes().to_vec()
        } else {
            Vec::new()
        };

        Ok(ComponentResult {
            success: response.success,
            exit_code: if response.success { 0 } else { 1 },
            data: Some(serde_json::json!({
                "gas_used": response.gas_used,
                "priority": response.priority,
                "events": events_data
            })),
            error: response.error,
            stdout: events_json.as_bytes().to_vec(),
            stderr: error_stderr,
            gas_used,
        })
    }

    /// Execute a tx-decoder component
    pub fn execute_tx_decoder(
        &self,
        component_name: &str,
        tx_bytes: &str,
        encoding: &str,
        validate: bool,
    ) -> Result<ComponentResult> {
        debug!("Executing tx-decoder component: {}", component_name);

        // Get the component
        let component = {
            let components = self.components.lock().map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Lock poisoned: {e}"))
            })?;
            components
                .get(component_name)
                .ok_or_else(|| ComponentHostError::ComponentNotFound(component_name.to_string()))?
                .clone()
        };

        // Create WASI context
        let wasi = WasiCtxBuilder::new().build();

        let state = ComponentState {
            table: wasmtime_wasi::ResourceTable::new(),
            wasi,
            component_name: component_name.to_string(),
            vfs: self.vfs.clone(),
        };

        let mut store = Store::new(&self.engine, state);

        // Set fuel limit
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

        // Create linker and add WASI
        let mut linker: Linker<ComponentState> = Linker::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
            .map_err(|e| ComponentHostError::WasiSetup(e.to_string()))?;

        // Add VFS-backed kvstore interface
        kvstore::add_to_linker::<ComponentState, wasmtime::component::HasSelf<ComponentState>>(&mut linker, |state| state)
            .map_err(|e| ComponentHostError::ComponentInstantiation(format!("Failed to add kvstore: {e}")))?;

        // Instantiate the component with bindings
        let bindings = TxDecoderWorld::instantiate(&mut store, &component, &linker)
            .map_err(|e| ComponentHostError::ComponentInstantiation(e.to_string()))?;

        // Create decode request using the generated types
        let request = crate::component_bindings::tx_decoder::exports::gridway::framework::tx_decoder::DecodeRequest {
            tx_bytes: tx_bytes.to_string(),
            encoding: encoding.to_string(),
            validate,
        };

        // Call decode-tx function through the generated interface
        let response = bindings
            .gridway_framework_tx_decoder()
            .call_decode_tx(&mut store, &request)
            .map_err(|e| ComponentHostError::ComponentExecution(e.to_string()))?;

        // Get gas consumed
        let gas_used = self.get_gas_consumed(&mut store);

        // Convert response to ComponentResult
        let stdout_data = response.decoded_tx.clone().unwrap_or_default();
        let data = response
            .decoded_tx
            .and_then(|s| serde_json::from_str(&s).ok());

        Ok(ComponentResult {
            success: response.success,
            exit_code: if response.success { 0 } else { 1 },
            data,
            error: response.error.clone(),
            stdout: stdout_data.as_bytes().to_vec(),
            stderr: response.error.unwrap_or_default().as_bytes().to_vec(),
            gas_used,
        })
    }

    /// Execute a begin-blocker component
    pub fn execute_begin_blocker(
        &self,
        block_height: u64,
        block_time: u64,
        chain_id: &str,
        gas_limit: u64,
        byzantine_validators: Vec<String>,
    ) -> Result<ComponentResult> {
        debug!("Executing begin-blocker component");

        // Get the component (assume "begin-blocker" as the component name)
        let component = {
            let components = self.components.lock().map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Lock poisoned: {e}"))
            })?;
            components
                .get("begin-blocker")
                .ok_or_else(|| ComponentHostError::ComponentNotFound("begin-blocker".to_string()))?
                .clone()
        };

        // Create store
        let mut store = Store::new(
            &self.engine,
            ComponentState {
                table: wasmtime_wasi::ResourceTable::new(),
                wasi: WasiCtxBuilder::new().inherit_stdio().build(),
                component_name: "begin-blocker".to_string(),
                vfs: self.vfs.clone(),
            },
        );

        // Set fuel for gas limiting
        store.set_fuel(gas_limit).map_err(|e| {
            ComponentHostError::ComponentExecution(format!("Failed to set fuel: {e}"))
        })?;

        // Create linker and add WASI
        let mut linker: Linker<ComponentState> = Linker::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
            .map_err(|e| ComponentHostError::WasiSetup(e.to_string()))?;

        // Add VFS-backed kvstore interface
        kvstore::add_to_linker::<ComponentState, wasmtime::component::HasSelf<ComponentState>>(&mut linker, |state| state)
            .map_err(|e| ComponentHostError::ComponentInstantiation(format!("Failed to add kvstore: {e}")))?;

        // Instantiate the component with bindings
        let bindings = crate::component_bindings::begin_blocker::BeginBlockerWorld::instantiate(
            &mut store, &component, &linker,
        )
        .map_err(|e| ComponentHostError::ComponentInstantiation(e.to_string()))?;

        // Create the request
        let evidence_list: Vec<crate::component_bindings::begin_blocker::exports::gridway::framework::begin_blocker::Evidence> = byzantine_validators
            .into_iter()
            .map(|_| crate::component_bindings::begin_blocker::exports::gridway::framework::begin_blocker::Evidence {
                validator_address: vec![],
                evidence_type: "duplicate_vote".to_string(),
                height: block_height,
            })
            .collect();

        let request = crate::component_bindings::begin_blocker::exports::gridway::framework::begin_blocker::BeginBlockRequest {
            height: block_height,
            time: block_time,
            chain_id: chain_id.to_string(),
            byzantine_validators: evidence_list,
        };

        // Execute the component
        let response = bindings
            .gridway_framework_begin_blocker()
            .call_begin_block(&mut store, &request)
            .map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Component execution failed: {e}"))
            })?;

        // Get remaining fuel for gas tracking
        let gas_used = gas_limit - store.get_fuel().unwrap_or(0);

        // Convert events to JSON for stdout
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
        let events_json = serde_json::to_string(&events_data).unwrap_or_default();

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
            stdout: events_json.as_bytes().to_vec(),
            stderr: error_stderr,
            gas_used,
        })
    }

    /// Execute an end-blocker component  
    pub fn execute_end_blocker(
        &self,
        block_height: u64,
        _block_time: u64,
        chain_id: &str,
        gas_limit: u64,
    ) -> Result<ComponentResult> {
        debug!("Executing end-blocker component");

        // Get the component (assume "end-blocker" as the component name)
        let component = {
            let components = self.components.lock().map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Lock poisoned: {e}"))
            })?;
            components
                .get("end-blocker")
                .ok_or_else(|| ComponentHostError::ComponentNotFound("end-blocker".to_string()))?
                .clone()
        };

        // Create store
        let mut store = Store::new(
            &self.engine,
            ComponentState {
                table: wasmtime_wasi::ResourceTable::new(),
                wasi: WasiCtxBuilder::new().inherit_stdio().build(),
                component_name: "end-blocker".to_string(),
                vfs: self.vfs.clone(),
            },
        );

        // Set fuel for gas limiting
        store.set_fuel(gas_limit).map_err(|e| {
            ComponentHostError::ComponentExecution(format!("Failed to set fuel: {e}"))
        })?;

        // Create linker and add WASI
        let mut linker: Linker<ComponentState> = Linker::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
            .map_err(|e| ComponentHostError::WasiSetup(e.to_string()))?;

        // Add VFS-backed kvstore interface
        kvstore::add_to_linker::<ComponentState, wasmtime::component::HasSelf<ComponentState>>(&mut linker, |state| state)
            .map_err(|e| ComponentHostError::ComponentInstantiation(format!("Failed to add kvstore: {e}")))?;

        // Instantiate the component with bindings
        let bindings = crate::component_bindings::end_blocker::EndBlockerWorld::instantiate(
            &mut store, &component, &linker,
        )
        .map_err(|e| ComponentHostError::ComponentInstantiation(e.to_string()))?;

        // Create the request
        let request = crate::component_bindings::end_blocker::exports::gridway::framework::end_blocker::EndBlockRequest {
            height: block_height,
            chain_id: chain_id.to_string(),
        };

        // Execute the component
        let response = bindings
            .gridway_framework_end_blocker()
            .call_end_block(&mut store, &request)
            .map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Component execution failed: {e}"))
            })?;

        // Get remaining fuel for gas tracking
        let gas_used = gas_limit - store.get_fuel().unwrap_or(0);

        // Convert events and validator updates to JSON for stdout
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
        let validator_updates_data: Vec<serde_json::Value> = response
            .validator_updates
            .iter()
            .map(|update| {
                serde_json::json!({
                    "pub_key": {
                        "type_url": update.pub_key.key_type,
                        "value": hex::encode(&update.pub_key.value)
                    },
                    "power": update.power
                })
            })
            .collect();
        let output_data = serde_json::json!({
            "events": events_data,
            "validator_updates": validator_updates_data
        });
        let output_json = serde_json::to_string(&output_data).unwrap_or_default();

        let error_stderr = if let Some(ref error) = response.error {
            error.as_bytes().to_vec()
        } else {
            Vec::new()
        };

        Ok(ComponentResult {
            success: response.success,
            exit_code: if response.success { 0 } else { 1 },
            data: Some(output_data),
            error: response.error,
            stdout: output_json.as_bytes().to_vec(),
            stderr: error_stderr,
            gas_used,
        })
    }

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

        // Get the component by module name
        let component = {
            let components = self.components.lock().map_err(|e| {
                ComponentHostError::ComponentExecution(format!("Lock poisoned: {e}"))
            })?;
            components
                .get(module_name)
                .ok_or_else(|| ComponentHostError::ComponentNotFound(module_name.to_string()))?
                .clone()
        };

        // Create store
        let mut store = Store::new(
            &self.engine,
            ComponentState {
                table: wasmtime_wasi::ResourceTable::new(),
                wasi: WasiCtxBuilder::new().inherit_stdio().build(),
                component_name: module_name.to_string(),
                vfs: self.vfs.clone(),
            },
        );

        // Set fuel for gas limiting
        store.set_fuel(gas_limit).map_err(|e| {
            ComponentHostError::ComponentExecution(format!("Failed to set fuel: {e}"))
        })?;

        // Create linker and add WASI
        let mut linker: Linker<ComponentState> = Linker::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
            .map_err(|e| ComponentHostError::WasiSetup(e.to_string()))?;

        // Add VFS-backed kvstore interface
        kvstore::add_to_linker::<ComponentState, wasmtime::component::HasSelf<ComponentState>>(&mut linker, |state| state)
            .map_err(|e| ComponentHostError::ComponentInstantiation(format!("Failed to add kvstore: {e}")))?;

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

// Component interface bindings would go here
// For now, we're using a simplified approach

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
        };

        // Opening a store without VFS should fail
        let result = kvstore::Host::open_store(&mut state, "bank".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("VFS not available"));
    }
}
