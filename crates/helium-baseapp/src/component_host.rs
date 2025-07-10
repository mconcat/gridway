//! WASI Component Host
//!
//! This module provides the WASI component runtime host that enables dynamic loading
//! and execution of WASM components using the component model and WIT interfaces.

use crate::component_bindings::ante_handler::AnteHandlerWorld;
use crate::component_bindings::tx_decoder::TxDecoderWorld;
use crate::component_bindings::SimpleKVStoreManager;
use crate::kvstore_resource::KVStoreResourceHost;
use hex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::{debug, error, info};
use wasmtime::component::*;
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

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

/// Component host state that implements WasiView
struct ComponentState {
    table: wasmtime_wasi::ResourceTable,
    wasi: WasiCtx,
    component_name: String,
    kvstore_manager: SimpleKVStoreManager,
    kvstore_host: KVStoreResourceHost,
}

impl WasiView for ComponentState {
    fn table(&mut self) -> &mut wasmtime_wasi::ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

// Import the generated kvstore bindings
use crate::component_bindings::ante_handler::helium::framework::kvstore;

impl kvstore::HostStore for ComponentState {
    fn get(
        &mut self,
        store_handle: wasmtime::component::Resource<kvstore::Store>,
        key: Vec<u8>,
    ) -> Option<Vec<u8>> {
        // Convert kvstore::Store to KVStoreResource
        let kvstore_resource = wasmtime::component::Resource::<
            crate::kvstore_resource::KVStoreResource,
        >::new_own(store_handle.rep());
        match self
            .kvstore_host
            .get_resource(&mut self.table, kvstore_resource)
        {
            Ok(store) => store.get(&key).unwrap_or(None),
            Err(_) => None,
        }
    }

    fn set(
        &mut self,
        store_handle: wasmtime::component::Resource<kvstore::Store>,
        key: Vec<u8>,
        value: Vec<u8>,
    ) {
        let kvstore_resource = wasmtime::component::Resource::<
            crate::kvstore_resource::KVStoreResource,
        >::new_own(store_handle.rep());
        if let Ok(store) = self
            .kvstore_host
            .get_resource(&mut self.table, kvstore_resource)
        {
            let _ = store.set(&key, &value);
        }
    }

    fn delete(
        &mut self,
        store_handle: wasmtime::component::Resource<kvstore::Store>,
        key: Vec<u8>,
    ) {
        let kvstore_resource = wasmtime::component::Resource::<
            crate::kvstore_resource::KVStoreResource,
        >::new_own(store_handle.rep());
        if let Ok(store) = self
            .kvstore_host
            .get_resource(&mut self.table, kvstore_resource)
        {
            let _ = store.delete(&key);
        }
    }

    fn has(
        &mut self,
        store_handle: wasmtime::component::Resource<kvstore::Store>,
        key: Vec<u8>,
    ) -> bool {
        let kvstore_resource = wasmtime::component::Resource::<
            crate::kvstore_resource::KVStoreResource,
        >::new_own(store_handle.rep());
        match self
            .kvstore_host
            .get_resource(&mut self.table, kvstore_resource)
        {
            Ok(store) => store.has(&key).unwrap_or(false),
            Err(_) => false,
        }
    }

    fn range(
        &mut self,
        store_handle: wasmtime::component::Resource<kvstore::Store>,
        start: Option<Vec<u8>>,
        end: Option<Vec<u8>>,
        limit: u32,
    ) -> Vec<(Vec<u8>, Vec<u8>)> {
        let kvstore_resource = wasmtime::component::Resource::<
            crate::kvstore_resource::KVStoreResource,
        >::new_own(store_handle.rep());
        match self
            .kvstore_host
            .get_resource(&mut self.table, kvstore_resource)
        {
            Ok(store) => store
                .range(start.as_deref(), end.as_deref(), limit)
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    fn drop(
        &mut self,
        _rep: wasmtime::component::Resource<kvstore::Store>,
    ) -> wasmtime::Result<()> {
        // Resource cleanup is handled by the resource table
        Ok(())
    }
}

impl kvstore::Host for ComponentState {
    fn open_store(
        &mut self,
        name: String,
    ) -> std::result::Result<wasmtime::component::Resource<kvstore::Store>, String> {
        // Map the KVStoreResource to kvstore::Store resource
        match self.kvstore_host.open_store(&mut self.table, &name) {
            Ok(resource) => {
                // Convert KVStoreResource to kvstore::Store
                let store_resource =
                    wasmtime::component::Resource::<kvstore::Store>::new_own(resource.rep());
                Ok(store_resource)
            }
            Err(e) => Err(e),
        }
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
    /// KVStore manager (legacy)
    kvstore_manager: SimpleKVStoreManager,
    /// KVStore resource host for prefix-based access
    kvstore_host: KVStoreResourceHost,
}

impl ComponentHost {
    /// Create a new component host with default configuration and a base store
    pub fn new(base_store: Arc<Mutex<dyn helium_store::KVStore>>) -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.async_support(false);
        Self::with_config_and_store(config, base_store)
    }

    /// Create a new component host with custom configuration and a base store
    pub fn with_config_and_store(
        mut config: Config,
        base_store: Arc<Mutex<dyn helium_store::KVStore>>,
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

        let kvstore_host = KVStoreResourceHost::new(base_store);

        // Register default component prefixes
        // These can be overridden by calling register_component_prefix
        kvstore_host
            .register_component_prefix("ante-handler".to_string(), "/ante/".to_string())
            .map_err(ComponentHostError::ResourceError)?;
        kvstore_host
            .register_component_prefix("begin-blocker".to_string(), "/begin/".to_string())
            .map_err(ComponentHostError::ResourceError)?;
        kvstore_host
            .register_component_prefix("end-blocker".to_string(), "/end/".to_string())
            .map_err(ComponentHostError::ResourceError)?;
        kvstore_host
            .register_component_prefix("tx-decoder".to_string(), "/decoder/".to_string())
            .map_err(ComponentHostError::ResourceError)?;

        Ok(Self {
            engine,
            components: Arc::new(Mutex::new(HashMap::new())),
            component_info: Arc::new(Mutex::new(HashMap::new())),
            default_gas_limit: 10_000_000, // 10 million units
            kvstore_manager: SimpleKVStoreManager::new(),
            kvstore_host,
        })
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
            kvstore_manager: SimpleKVStoreManager::new(),
            kvstore_host: self.kvstore_host.clone(),
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
        wasmtime_wasi::add_to_linker_sync(&mut linker)
            .map_err(|e| ComponentHostError::WasiSetup(e.to_string()))?;

        // Add module-state interface

        // Add kvstore interface
        self.add_kvstore_to_linker(&mut linker)?;

        // Instantiate the component with bindings
        let bindings = AnteHandlerWorld::instantiate(&mut store, &component, &linker)
            .map_err(|e| ComponentHostError::ComponentInstantiation(e.to_string()))?;

        // Create the context
        let context = crate::component_bindings::ante_handler::exports::helium::framework::ante_handler::TxContext {
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
            .helium_framework_ante_handler()
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
            kvstore_manager: SimpleKVStoreManager::new(),
            kvstore_host: self.kvstore_host.clone(),
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
        wasmtime_wasi::add_to_linker_sync(&mut linker)
            .map_err(|e| ComponentHostError::WasiSetup(e.to_string()))?;

        // Add module-state interface

        // Add kvstore interface
        self.add_kvstore_to_linker(&mut linker)?;

        // Instantiate the component with bindings
        let bindings = TxDecoderWorld::instantiate(&mut store, &component, &linker)
            .map_err(|e| ComponentHostError::ComponentInstantiation(e.to_string()))?;

        // Create decode request using the generated types
        let request = crate::component_bindings::tx_decoder::exports::helium::framework::tx_decoder::DecodeRequest {
            tx_bytes: tx_bytes.to_string(),
            encoding: encoding.to_string(),
            validate,
        };

        // Call decode-tx function through the generated interface
        let response = bindings
            .helium_framework_tx_decoder()
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
                kvstore_manager: self.kvstore_manager.clone(),
                kvstore_host: self.kvstore_host.clone(),
            },
        );

        // Set fuel for gas limiting
        store.set_fuel(gas_limit).map_err(|e| {
            ComponentHostError::ComponentExecution(format!("Failed to set fuel: {e}"))
        })?;

        // Create linker and add WASI
        let mut linker: Linker<ComponentState> = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker_sync(&mut linker)
            .map_err(|e| ComponentHostError::WasiSetup(e.to_string()))?;

        // Add module-state interface

        // Add kvstore interface
        self.add_kvstore_to_linker(&mut linker)?;

        // Instantiate the component with bindings
        let bindings = crate::component_bindings::begin_blocker::BeginBlockerWorld::instantiate(
            &mut store, &component, &linker,
        )
        .map_err(|e| ComponentHostError::ComponentInstantiation(e.to_string()))?;

        // Create the request
        let evidence_list: Vec<crate::component_bindings::begin_blocker::exports::helium::framework::begin_blocker::Evidence> = byzantine_validators
            .into_iter()
            .map(|_| crate::component_bindings::begin_blocker::exports::helium::framework::begin_blocker::Evidence {
                validator_address: vec![],
                evidence_type: "duplicate_vote".to_string(),
                height: block_height,
            })
            .collect();

        let request = crate::component_bindings::begin_blocker::exports::helium::framework::begin_blocker::BeginBlockRequest {
            height: block_height,
            time: block_time,
            chain_id: chain_id.to_string(),
            byzantine_validators: evidence_list,
        };

        // Execute the component
        let response = bindings
            .helium_framework_begin_blocker()
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
                kvstore_manager: self.kvstore_manager.clone(),
                kvstore_host: self.kvstore_host.clone(),
            },
        );

        // Set fuel for gas limiting
        store.set_fuel(gas_limit).map_err(|e| {
            ComponentHostError::ComponentExecution(format!("Failed to set fuel: {e}"))
        })?;

        // Create linker and add WASI
        let mut linker: Linker<ComponentState> = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker_sync(&mut linker)
            .map_err(|e| ComponentHostError::WasiSetup(e.to_string()))?;

        // Add module-state interface

        // Add kvstore interface
        self.add_kvstore_to_linker(&mut linker)?;

        // Instantiate the component with bindings
        let bindings = crate::component_bindings::end_blocker::EndBlockerWorld::instantiate(
            &mut store, &component, &linker,
        )
        .map_err(|e| ComponentHostError::ComponentInstantiation(e.to_string()))?;

        // Create the request
        let request = crate::component_bindings::end_blocker::exports::helium::framework::end_blocker::EndBlockRequest {
            height: block_height,
            chain_id: chain_id.to_string(),
        };

        // Execute the component
        let response = bindings
            .helium_framework_end_blocker()
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

    /// Mount a KVStore for component access (legacy)
    pub fn mount_kvstore(
        &self,
        name: String,
        store: Arc<Mutex<dyn helium_store::KVStore>>,
    ) -> Result<()> {
        self.kvstore_manager
            .mount_store(name, store)
            .map_err(ComponentHostError::ResourceError)
    }

    /// Register a component with its allowed KVStore prefix
    pub fn register_component_prefix(&self, component_name: &str, prefix: &str) -> Result<()> {
        self.kvstore_host
            .register_component_prefix(component_name.to_string(), prefix.to_string())
            .map_err(ComponentHostError::ResourceError)
    }

    /// Add kvstore interface to the component linker
    fn add_kvstore_to_linker(&self, linker: &mut Linker<ComponentState>) -> Result<()> {
        // Use the generated kvstore bindings
        kvstore::add_to_linker(linker, |state| state).map_err(|e| {
            ComponentHostError::ComponentInstantiation(format!(
                "Failed to add kvstore interface: {e}"
            ))
        })?;
        info!("KVStore interface added to linker");
        Ok(())
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
        let base_store = Arc::new(Mutex::new(helium_store::MemStore::new()));
        let host = ComponentHost::new(base_store).unwrap();
        assert!(host.components.lock().unwrap().is_empty());
    }
}
