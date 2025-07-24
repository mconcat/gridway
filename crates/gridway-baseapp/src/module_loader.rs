//! WASM Module Loading System
//!
//! This module provides functionality for discovering, loading, validating, and managing
//! WASM modules at runtime. It handles module lifecycle, resource management, and provides
//! hot-reloading capabilities.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::fs;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error, info, warn};
use wasmtime::{Engine, Instance, Linker, Module, Store, StoreLimits, TypedFunc};

use crate::module_router::{ModuleConfig, ModuleEvent};
use crate::capabilities::CapabilityManager;
use crate::vfs::VirtualFilesystem;
use crate::abi::{AbiContext, Capability, HostFunctions};

/// Module loader error types
#[derive(Error, Debug)]
pub enum ModuleLoaderError {
    /// File system error
    #[error("file system error:: {0}")]
    FileSystem(#[from] std::io::Error),
    
    /// WASM validation error
    #[error("WASM validation error:: {0}")]
    ValidationError(String),
    
    /// Missing required export
    #[error("missing required export:: {0}")]
    MissingExport(String),
    
    /// Module instantiation error
    #[error("instantiation error:: {0}")]
    InstantiationError(String),
    
    /// Module not found
    #[error("module not found:: {0}")]
    ModuleNotFound(String),
    
    /// Invalid module metadata
    #[error("invalid module metadata:: {0}")]
    InvalidMetadata(String),
    
    /// State migration error during reload
    #[error("state migration error:: {0}")]
    StateMigrationError(String),
    
    /// Resource limit exceeded
    #[error("resource limit exceeded:: {0}")]
    ResourceLimitExceeded(String),
    
    /// Module already loaded
    #[error("module already loaded:: {0}")]
    ModuleAlreadyLoaded(String),
}

/// Module metadata extracted from WASM
#[derive(Debug, Clone)]
pub struct ModuleMetadata {
    /// Module name (from custom section)
    pub name: String,
    /// Module version
    pub version: String,
    /// Required host functions
    pub required_imports: Vec<String>,
    /// Exported functions
    pub exports: Vec<String>,
    /// Custom metadata
    pub custom: HashMap<String, String>,
}

/// Information about a discovered module
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// File path
    pub path: PathBuf,
    /// Module metadata
    pub metadata: ModuleMetadata,
    /// File size in bytes
    pub size: u64,
    /// Last modified time
    pub modified: std::time::SystemTime,
}

/// A loaded and instantiated WASM module
pub struct LoadedModule {
    /// Module name
    pub name: String,
    /// WASM module
    pub module: Module,
    /// Module instance
    pub instance: Instance,
    /// Store with ABI context
    pub store: Store<AbiContext>,
    /// Module metadata
    pub metadata: ModuleMetadata,
    /// Creation time
    pub loaded_at: std::time::SystemTime,
}

/// Module state for hot reloading
#[derive(Debug, Clone)]
pub struct ModuleState {
    /// Module name
    pub module_name: String,
    /// Serialized state data
    pub state_data: Vec<u8>,
    /// State version
    pub version: u32,
}

/// Store limits for resource management
#[derive(Debug, Clone)]
pub struct ModuleLimits {
    /// Maximum memory in bytes
    pub memory_size: usize,
    /// Maximum table elements
    pub table_elements: u32,
    /// Maximum instances
    pub instances: u32,
    /// Maximum memories
    pub memories: u32,
}

impl Default for ModuleLimits {
    fn default() -> Self {
        Self {
            memory_size: 100 * 1024 * 1024, // 100MB
            table_elements: 10_000,
            instances: 10,
            memories: 1,
        }
    }
}

/// Inter-module message for communication
#[derive(Debug, Clone)]
pub struct InterModuleMessage {
    /// Sender module name
    pub from: String,
    /// Receiver module name
    pub to: String,
    /// Message type/endpoint
    pub endpoint: String,
    /// Message payload
    pub payload: Vec<u8>,
    /// Message ID for tracking
    pub id: u64,
    /// Timestamp
    pub timestamp: std::time::SystemTime,
}

/// Module loading configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleLoaderConfig {
    /// Directory containing WASM modules
    pub modules_dir: PathBuf,
    /// Cache size for compiled modules
    pub cache_size: usize,
    /// Default memory limit in bytes
    pub memory_limit: usize,
    /// Default CPU time limit in milliseconds
    pub cpu_time_limit: u64,
    /// Allow hot reloading
    pub allow_hot_reload: bool,
    /// Module configurations
    pub modules: Vec<ModuleConfigEntry>,
}

/// Individual module configuration entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleConfigEntry {
    /// Module name
    pub name: String,
    /// WASM file path (relative to modules_dir)
    pub path: String,
    /// Whether to preload on startup
    pub preload: bool,
    /// Module capabilities
    pub capabilities: Vec<String>,
    /// Memory limit override
    pub memory_limit: Option<usize>,
    /// Gas limit override
    pub gas_limit: Option<u64>,
    /// IPC endpoints this module provides
    pub endpoints: Vec<String>,
    /// Message types this module handles
    pub message_types: Vec<String>,
}

impl Default for ModuleLoaderConfig {
    fn default() -> Self {
        Self {
            modules_dir: PathBuf::from("./wasm_modules"),
            cache_size: 100,
            memory_limit: 512 * 1024 * 1024, // 512MB
            cpu_time_limit: 5000, // 5 seconds
            allow_hot_reload: true,
            modules: vec![],
        }
    }
}

/// Module communication registry
#[derive(Debug)]
pub struct ModuleRegistry {
    /// Message queue for inter-module communication
    message_queue: Arc<Mutex<VecDeque<InterModuleMessage>>>,
    /// Message ID counter
    message_counter: Arc<Mutex<u64>>,
    /// Module endpoint handlers
    endpoint_handlers: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

impl ModuleRegistry {
    /// Create a new module registry
    pub fn new() -> Self {
        Self {
            message_queue: Arc::new(Mutex::new(VecDeque::new())),
            message_counter: Arc::new(Mutex::new(0)),
            endpoint_handlers: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Send a message from one module to another
    pub fn send_message(
        &self,
        from: String,
        to: String,
        endpoint: String,
        payload: Vec<u8>,
    ) -> Result<u64, ModuleLoaderError> {
        let id = {
            let mut counter = self.message_counter.lock().unwrap();
            *counter += 1;
            *counter
        };
        
        let message = InterModuleMessage {
            from,
            to,
            endpoint,
            payload,
            id,
            timestamp: std::time::SystemTime::now(),
        };
        
        let mut queue = self.message_queue.lock().unwrap();
        queue.push_back(message);
        
        Ok(id)
    }
    
    /// Receive messages for a specific module
    pub fn receive_messages(&self, module_name: &str) -> Vec<InterModuleMessage> {
        let mut queue = self.message_queue.lock().unwrap();
        let mut messages = Vec::new();
        
        // Collect all messages for this module
        let mut i = 0;
        while i < queue.len() {
            if queue[i].to == module_name {
                if let Some(msg) = queue.remove(i) {
                    messages.push(msg);
                }
            } else {
                i += 1;
            }
        }
        
        messages
    }
    
    /// Register an endpoint handler for a module
    pub fn register_endpoint(&self, module_name: String, endpoint: String) {
        let mut handlers = self.endpoint_handlers.lock().unwrap();
        handlers.entry(endpoint).or_insert_with(Vec::new).push(module_name);
    }
    
    /// Get modules that handle a specific endpoint
    pub fn get_endpoint_handlers(&self, endpoint: &str) -> Vec<String> {
        let handlers = self.endpoint_handlers.lock().unwrap();
        handlers.get(endpoint).cloned().unwrap_or_default()
    }
}

/// WASM module loader and manager
pub struct ModuleLoader {
    /// Directory containing WASM modules
    modules_dir: PathBuf,
    /// WASM runtime engine
    engine: Engine,
    /// Loaded modules
    modules: Arc<Mutex<HashMap<String, LoadedModule>>>,
    /// Module configurations
    configs: HashMap<String, ModuleConfig>,
    /// Resource limits
    limits: ModuleLimits,
    /// Capability manager
    capability_manager: Arc<CapabilityManager>,
    /// Virtual filesystem
    vfs: Arc<VirtualFilesystem>,
    /// Module registry for inter-module communication
    registry: ModuleRegistry,
}

impl ModuleLoader {
    /// Create a new module loader
    pub fn new(
        modules_dir: PathBuf,
        capability_manager: Arc<CapabilityManager>,
        vfs: Arc<VirtualFilesystem>,
    ) -> Result<Self> {
        // Configure engine with optimizations
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(false);
        config.async_support(false);
        config.consume_fuel(true); // Enable gas metering
        config.epoch_interruption(true); // Enable interruption
        
        let engine = Engine::new(&config)?;
        
        Ok(Self {
            modules_dir,
            engine,
            modules: Arc::new(Mutex::new(HashMap::new())),
            configs: HashMap::new(),
            limits: ModuleLimits::default(),
            capability_manager,
            vfs,
            registry: ModuleRegistry::new(),
        })
    }
    
    /// Create module loader from configuration
    pub fn from_config(
        config: ModuleLoaderConfig,
        capability_manager: Arc<CapabilityManager>,
        vfs: Arc<VirtualFilesystem>,
    ) -> Result<Self, ModuleLoaderError> {
        let mut loader = Self::new(config.modules_dir.clone(), capability_manager, vfs)?;
        
        // Set memory limits from config
        loader.limits.memory_size = config.memory_limit;
        
        // Process module configurations
        for module_config in config.modules {
            let mut mod_config = ModuleConfig::new(
                module_config.name.clone(),
                config.modules_dir.join(&module_config.path),
            );
            
            // Set capabilities
            for cap in module_config.capabilities {
                mod_config = mod_config.requires_capability(cap);
            }
            
            // Set message types
            for msg_type in module_config.message_types {
                mod_config = mod_config.handles_message_type(msg_type);
            }
            
            // Set endpoints
            for endpoint in module_config.endpoints {
                mod_config = mod_config.provides_endpoint(endpoint);
            }
            
            // Set limits if overridden
            if let Some(gas_limit) = module_config.gas_limit {
                mod_config = mod_config.with_gas_limit(gas_limit);
            }
            if let Some(mem_limit) = module_config.memory_limit {
                mod_config = mod_config.with_memory_limit(mem_limit as u64);
            }
            
            loader.add_module_config(mod_config);
            
            // Preload if requested
            if module_config.preload {
                let module_path = config.modules_dir.join(&module_config.path);
                if let Err(e) = loader.load_module(&module_config.name, &module_path) {
                    error!("Failed to preload module {}: {}", module_config.name, e);
                }
            }
        }
        
        Ok(loader)
    }
    
    /// Load configuration from TOML file
    pub fn load_config(path: &Path) -> Result<ModuleLoaderConfig, ModuleLoaderError> {
        let contents = fs::read_to_string(path)
            .map_err(|e| ModuleLoaderError::FileSystem(e))?;
        
        toml::from_str(&contents)
            .map_err(|e| ModuleLoaderError::InvalidConfig(format!("Failed to parse config:: {}", e)))
    }
    
    /// Set resource limits
    pub fn set_limits(&mut self, limits: ModuleLimits) {
        self.limits = limits;
    }
    
    /// Add module configuration
    pub fn add_module_config(&mut self, config: ModuleConfig) {
        self.configs.insert(config.name.clone(), config);
    }
    
    /// Scan modules directory for WASM files
    pub fn scan_modules(&mut self) -> Result<Vec<ModuleInfo>, ModuleLoaderError> {
        let mut modules = Vec::new();
        
        info!("Scanning modules directory:: {:?}", self.modules_dir);
        
        for entry in fs::read_dir(&self.modules_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                match self.load_module_info(&path) {
                    Ok(info) => {
                        info!("Found module:: {} at {:?}", info.metadata.name, path);
                        modules.push(info);
                    }
                    Err(e) => {
                        warn!("Failed to load module info from {:?}: {}", path, e);
                    }
                }
            }
        }
        
        Ok(modules)
    }
    
    /// Load module information without instantiating
    fn load_module_info(&self, path: &Path) -> Result<ModuleInfo, ModuleLoaderError> {
        let wasm_bytes = fs::read(path)?;
        let metadata = fs::metadata(path)?;
        
        // Validate and extract metadata
        let module_metadata = self.validate_module(&wasm_bytes)?;
        
        Ok(ModuleInfo {
            path: path.to_path_buf(),
            metadata: module_metadata,
            size: metadata.len(),
            modified: metadata.modified()?,
        })
    }
    
    /// Validate WASM module and extract metadata
    pub fn validate_module(&self, wasm_bytes: &[u8]) -> Result<ModuleMetadata, ModuleLoaderError> {
        // Validate WASM format using Wasmtime's validation
        // The Module::new will validate the WASM bytes
        
        // Create module to inspect exports
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| ModuleLoaderError::ValidationError(e.to_string()))?;
        
        // Check for required exports
        let exports = module.exports(&mut Store::new(&self.engine, ()));
        let mut export_names = Vec::new();
        let mut has_required = HashMap::new();
        
        // Required functions for Cosmos SDK modules
        let required_functions = [
            "init_genesis",
            "handle_message", 
            "handle_query",
            "end_block",
        ];
        
        for export in exports {
            export_names.push(export.name().to_string());
            if required_functions.contains(&export.name()) {
                if export.ty(&mut Store::new(&self.engine, ())).func().is_some() {
                    has_required.insert(export.name().to_string(), true);
                }
            }
        }
        
        // Verify all required functions exist
        for func_name in &required_functions {
            if !has_required.contains_key(*func_name) {
                return Err(ModuleLoaderError::MissingExport(func_name.to_string()));
            }
        }
        
        // Extract module metadata from custom sections
        let metadata = self.extract_module_metadata(&module, wasm_bytes)?;
        
        Ok(metadata)
    }
    
    /// Extract metadata from module custom sections
    fn extract_module_metadata(&self, module: &Module, _wasm_bytes: &[u8]) -> Result<ModuleMetadata, ModuleLoaderError> {
        let name = String::from("unknown");
        let version = String::from("0.1.0");
        let custom = HashMap::new();
        
        // Note: Custom section parsing would require wasmparser
        // For now, we'll use basic metadata
        
        // Get imports
        let mut required_imports = Vec::new();
        for import in module.imports() {
            required_imports.push(format!("{}::{}", import.module(), import.name()));
        }
        
        // Get exports
        let mut exports = Vec::new();
        for export in module.exports(&mut Store::new(&self.engine, ())) {
            exports.push(export.name().to_string());
        }
        
        Ok(ModuleMetadata {
            name,
            version,
            required_imports,
            exports,
            custom,
        })
    }
    
    /// Load and instantiate a module
    pub fn load_module(&mut self, module_name: &str, wasm_path: &Path) -> Result<(), ModuleLoaderError> {
        // Check if already loaded
        {
            let modules = self.modules.lock().unwrap();
            if modules.contains_key(module_name) {
                return Err(ModuleLoaderError::ModuleAlreadyLoaded(module_name.to_string()));
            }
        }
        
        info!("Loading module:: {} from {:?}", module_name, wasm_path);
        
        let wasm_bytes = fs::read(wasm_path)?;
        self.instantiate_module(module_name, wasm_bytes)
    }
    
    /// Instantiate a module from bytes
    pub fn instantiate_module(
        &mut self,
        module_name: &str,
        wasm_bytes: Vec<u8>,
    ) -> Result<(), ModuleLoaderError> {
        // Validate first
        let metadata = self.validate_module(&wasm_bytes)?;
        
        // Create module
        let module = Module::new(&self.engine, &wasm_bytes)
            .map_err(|e| ModuleLoaderError::InstantiationError(e.to_string()))?;
        
        // Create ABI context with appropriate capabilities
        let capabilities = self.get_module_capabilities(module_name);
        let mut abi_context = AbiContext::new(module_name.to_string(), capabilities);
        
        // Set VFS and capability manager
        abi_context.set_vfs(self.vfs.clone());
        abi_context.set_capability_manager(self.capability_manager.clone());
        
        // Create store with ABI context and limits
        let mut store = Store::new(&self.engine, abi_context);
        let limits = self.limits.clone();
        store.limiter(move |_| {
            &mut StoreLimits {
                memory_size: limits.memory_size,
                table_elements: limits.table_elements,
                instances: limits.instances as usize,
                memories: limits.memories as usize,
            }
        });
        
        // Add fuel for gas metering
        store.set_fuel(1_000_000).unwrap();
        
        // Create linker and add functions
        let linker = self.create_linker()?;
        
        // Set transaction context for the module
        store.data_mut().tx_context = Some(format!("wasi_module_{}", module_name));
        
        // Instantiate module
        let instance = linker.instantiate(&mut store, &module)
            .map_err(|e| ModuleLoaderError::InstantiationError(e.to_string()))?;
        
        // Create loaded module
        let loaded_module = LoadedModule {
            name: module_name.to_string(),
            module,
            instance,
            store,
            metadata,
            loaded_at: std::time::SystemTime::now(),
        };
        
        // Store the module
        {
            let mut modules = self.modules.lock().unwrap();
            modules.insert(module_name.to_string(), loaded_module);
        }
        
        info!("Module {} loaded successfully", module_name);
        Ok(())
    }
    
    
    /// Get capabilities for a module based on its configuration
    fn get_module_capabilities(&self, module_name: &str) -> Vec<Capability> {
        // Check if we have a specific configuration for this module
        if let Some(config) = self.configs.get(module_name) {
            // Convert string capabilities to Capability enum
            let mut capabilities = Vec::new();
            for cap_str in &config.capabilities {
                match cap_str.as_str() {
                    "read_state" => capabilities.push(Capability::ReadState),
                    "write_state" => capabilities.push(Capability::WriteState),
                    "send_message" => capabilities.push(Capability::SendMessage),
                    "access_transaction" => capabilities.push(Capability::AccessTransaction),
                    "emit_event" => capabilities.push(Capability::EmitEvent),
                    "access_block" => capabilities.push(Capability::AccessBlock),
                    "log" => capabilities.push(Capability::Log),
                    "allocate_memory" => capabilities.push(Capability::AllocateMemory),
                    _ => warn!("Unknown capability:: {}", cap_str),
                }
            }
            capabilities
        } else {
            // Default capabilities for modules
            vec![
                Capability::Log,
                Capability::AllocateMemory,
                Capability::ReadState,
                Capability::WriteState,
                Capability::EmitEvent,
            ]
        }
    }
    
    /// Create linker with host functions
    fn create_linker(&self) -> Result<Linker<AbiContext>, ModuleLoaderError> {
        let mut linker = Linker::new(&self.engine);
        
        // Add all standard host functions from ABI
        HostFunctions::add_to_linker(&mut linker)
            .map_err(|e| ModuleLoaderError::InstantiationError(format!("Failed to add host functions:: {}", e)))?;
        
        // Note: WASI functions will need to be added separately as they require WasiCtx
        // This is a limitation we'll need to work around
        
        Ok(linker)
    }
    
    /// Get a loaded module
    pub fn get_module(&self, module_name: &str) -> Option<Arc<Mutex<LoadedModule>>> {
        let modules = self.modules.lock().unwrap();
        if modules.contains_key(module_name) {
            // This is a simplification - in real implementation we'd need to handle this differently
            None
        } else {
            None
        }
    }
    
    /// Unload a module
    pub fn unload_module(&mut self, module_name: &str) -> Result<(), ModuleLoaderError> {
        let mut modules = self.modules.lock().unwrap();
        if modules.remove(module_name).is_some() {
            info!("Module {} unloaded", module_name);
            Ok(())
        } else {
            Err(ModuleLoaderError::ModuleNotFound(module_name.to_string()))
        }
    }
    
    /// Export module state for hot reloading
    pub fn export_module_state(&self, module_name: &str) -> Result<ModuleState, ModuleLoaderError> {
        let modules = self.modules.lock().unwrap();
        let _module = modules.get(module_name)
            .ok_or_else(|| ModuleLoaderError::ModuleNotFound(module_name.to_string()))?;
        
        // TODO: Implement actual state export
        // This would involve calling a special export function in the module
        // that serializes its internal state
        
        Ok(ModuleState {
            module_name: module_name.to_string(),
            state_data: vec![],
            version: 1,
        })
    }
    
    /// Import module state after reload
    pub fn import_module_state(&mut self, module_name: &str, state: ModuleState) -> Result<(), ModuleLoaderError> {
        let modules = self.modules.lock().unwrap();
        let _module = modules.get(module_name)
            .ok_or_else(|| ModuleLoaderError::ModuleNotFound(module_name.to_string()))?;
        
        // TODO: Implement actual state import
        // This would involve calling a special import function in the module
        // that deserializes and restores its internal state
        
        Ok(())
    }
    
    /// Reload a module (hot reload)
    pub fn reload_module(&mut self, module_name: &str) -> Result<(), ModuleLoaderError> {
        info!("Hot reloading module:: {}", module_name);
        
        // Export current state
        let state = self.export_module_state(module_name)?;
        
        // Find module path
        let module_path = self.modules_dir.join(format!("{}.wasm", module_name));
        if !module_path.exists() {
            return Err(ModuleLoaderError::ModuleNotFound(module_name.to_string()));
        }
        
        // Unload old module
        self.unload_module(module_name)?;
        
        // Load new module
        self.load_module(module_name, &module_path)?;
        
        // Import state
        self.import_module_state(module_name, state)?;
        
        info!("Module {} reloaded successfully", module_name);
        Ok(())
    }
    
    /// List all loaded modules
    pub fn list_modules(&self) -> Vec<String> {
        let modules = self.modules.lock().unwrap();
        modules.keys().cloned().collect()
    }
    
    /// Get module metadata
    pub fn get_module_metadata(&self, module_name: &str) -> Option<ModuleMetadata> {
        let modules = self.modules.lock().unwrap();
        modules.get(module_name).map(|m| m.metadata.clone())
    }
    
    /// Send inter-module message
    pub fn send_message(
        &self,
        from: String,
        to: String,
        endpoint: String,
        payload: Vec<u8>,
    ) -> Result<u64, ModuleLoaderError> {
        self.registry.send_message(from, to, endpoint, payload)
    }
    
    /// Receive messages for a module
    pub fn receive_messages(&self, module_name: &str) -> Vec<InterModuleMessage> {
        self.registry.receive_messages(module_name)
    }
    
    /// Register an endpoint for a module
    pub fn register_endpoint(&self, module_name: String, endpoint: String) {
        self.registry.register_endpoint(module_name, endpoint);
    }
    
    /// Execute a module function
    pub fn call_module_function(
        &self,
        module_name: &str,
        function_name: &str,
        args: &[u8],
    ) -> Result<Vec<u8>, ModuleLoaderError> {
        let modules = self.modules.lock().unwrap();
        let _module = modules.get(module_name)
            .ok_or_else(|| ModuleLoaderError::ModuleNotFound(module_name.to_string()))?;
        
        // TODO: Actually call the function through the instance
        // This would involve:
        // 1. Getting the function export from the instance
        // 2. Preparing the arguments in WASM memory
        // 3. Calling the function
        // 4. Reading the result from WASM memory
        
        warn!("Module function execution not fully implemented yet");
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::io::Write;
    
    /// Create a minimal valid WASM module for testing
    fn create_test_wasm() -> Vec<u8> {
        // Minimal WASM module with required exports
        wat::parse_str(r#"
            (module
                (memory 1)
                (export "memory" (memory 0))
                
                ;; Required functions for Cosmos SDK modules
                (func $init_genesis (export "init_genesis") (param i32 i32) (result i32)
                    i32.const 0
                )
                
                (func $handle_message (export "handle_message") (param i32 i32) (result i32)
                    i32.const 0
                )
                
                (func $handle_query (export "handle_query") (param i32 i32) (result i32)
                    i32.const 0
                )
                
                (func $end_block (export "end_block") (param i32 i32) (result i32)
                    i32.const 0
                )
            )
        "#).unwrap()
    }
    
    #[test]
    fn test_module_loader_creation() {
        let temp_dir = TempDir::new().unwrap();
        let cap_manager = Arc::new(CapabilityManager::new());
        let vfs = Arc::new(VirtualFilesystem::new());
        
        let loader = ModuleLoader::new(
            temp_dir.path().to_path_buf(),
            cap_manager,
            vfs,
        );
        
        assert!(loader.is_ok());
    }
    
    #[test]
    fn test_scan_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let cap_manager = Arc::new(CapabilityManager::new());
        let vfs = Arc::new(VirtualFilesystem::new());
        
        let mut loader = ModuleLoader::new(
            temp_dir.path().to_path_buf(),
            cap_manager,
            vfs,
        ).unwrap();
        
        let modules = loader.scan_modules().unwrap();
        assert_eq!(modules.len(), 0);
    }
    
    #[test]
    fn test_module_validation() {
        let temp_dir = TempDir::new().unwrap();
        let cap_manager = Arc::new(CapabilityManager::new());
        let vfs = Arc::new(VirtualFilesystem::new());
        
        let loader = ModuleLoader::new(
            temp_dir.path().to_path_buf(),
            cap_manager,
            vfs,
        ).unwrap();
        
        // Test with valid WASM
        let valid_wasm = create_test_wasm();
        let result = loader.validate_module(&valid_wasm);
        assert!(result.is_ok());
        
        let metadata = result.unwrap();
        assert!(metadata.exports.contains(&"init_genesis".to_string()));
        assert!(metadata.exports.contains(&"handle_message".to_string()));
        assert!(metadata.exports.contains(&"handle_query".to_string()));
        assert!(metadata.exports.contains(&"end_block".to_string()));
        
        // Test with invalid WASM
        let invalid_wasm = vec![0, 1, 2, 3];
        let result = loader.validate_module(&invalid_wasm);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_module_config() {
        let config = ModuleConfig::new("test_module".to_string(), PathBuf::from("test.wasm"))
            .handles_message_type("/cosmos.bank.v1beta1.MsgSend".to_string())
            .depends_on("auth".to_string())
            .requires_capability("read_state".to_string())
            .with_gas_limit(500000)
            .with_memory_limit(32 * 1024 * 1024)
            .exports_handlers()
            .provides_endpoint("transfer".to_string());
        
        assert_eq!(config.name, "test_module");
        assert_eq!(config.message_types.len(), 1);
        assert_eq!(config.dependencies.len(), 1);
        assert_eq!(config.capabilities.len(), 1);
        assert_eq!(config.gas_limit, 500000);
        assert_eq!(config.memory_limit, 32 * 1024 * 1024);
        assert!(config.exports_handlers);
        assert_eq!(config.ipc_endpoints.len(), 1);
    }
    
    #[test]
    fn test_module_registry() {
        let registry = ModuleRegistry::new();
        
        // Test message sending
        let msg_id = registry.send_message(
            "module_a".to_string(),
            "module_b".to_string(),
            "transfer".to_string(),
            vec![1, 2, 3, 4],
        ).unwrap();
        
        assert_eq!(msg_id, 1);
        
        // Test message receiving
        let messages = registry.receive_messages("module_b");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].from, "module_a");
        assert_eq!(messages[0].to, "module_b");
        assert_eq!(messages[0].endpoint, "transfer");
        assert_eq!(messages[0].payload, vec![1, 2, 3, 4]);
        
        // Verify message was removed from queue
        let messages = registry.receive_messages("module_b");
        assert_eq!(messages.len(), 0);
        
        // Test endpoint registration
        registry.register_endpoint("bank".to_string(), "transfer".to_string());
        registry.register_endpoint("staking".to_string(), "transfer".to_string());
        
        let handlers = registry.get_endpoint_handlers("transfer");
        assert_eq!(handlers.len(), 2);
        assert!(handlers.contains(&"bank".to_string()));
        assert!(handlers.contains(&"staking".to_string()));
    }
    
    #[test]
    fn test_module_loader_config() {
        let config = ModuleLoaderConfig {
            modules_dir: PathBuf::from("/test/modules"),
            cache_size: 50,
            memory_limit: 256 * 1024 * 1024,
            cpu_time_limit: 3000,
            allow_hot_reload: false,
            modules: vec![
                ModuleConfigEntry {
                    name: "auth".to_string(),
                    path: "auth.wasm".to_string(),
                    preload: true,
                    capabilities: vec!["read_state".to_string(), "write_state".to_string()],
                    memory_limit: Some(64 * 1024 * 1024),
                    gas_limit: Some(500000),
                    endpoints: vec!["validate".to_string()],
                    message_types: vec!["/cosmos.auth.v1beta1.MsgUpdateParams".to_string()],
                },
            ],
        };
        
        // Serialize to TOML
        let toml_str = toml::to_string(&config).unwrap();
        
        // Deserialize back
        let parsed: ModuleLoaderConfig = toml::from_str(&toml_str).unwrap();
        
        assert_eq!(parsed.modules_dir, PathBuf::from("/test/modules"));
        assert_eq!(parsed.cache_size, 50);
        assert_eq!(parsed.modules.len(), 1);
        assert_eq!(parsed.modules[0].name, "auth");
    }
    
    #[test]
    fn test_scan_modules_with_wasm_files() {
        let temp_dir = TempDir::new().unwrap();
        let cap_manager = Arc::new(CapabilityManager::new());
        let vfs = Arc::new(VirtualFilesystem::new());
        
        // Create test WASM files
        let wasm_data = create_test_wasm();
        
        let module1_path = temp_dir.path().join("module1.wasm");
        let module2_path = temp_dir.path().join("module2.wasm");
        let non_wasm_path = temp_dir.path().join("not_a_module.txt");
        
        std::fs::write(&module1_path, &wasm_data).unwrap();
        std::fs::write(&module2_path, &wasm_data).unwrap();
        std::fs::write(&non_wasm_path, b"not wasm").unwrap();
        
        let mut loader = ModuleLoader::new(
            temp_dir.path().to_path_buf(),
            cap_manager,
            vfs,
        ).unwrap();
        
        let modules = loader.scan_modules().unwrap();
        assert_eq!(modules.len(), 2);
        
        // Verify module info
        for module in modules {
            assert!(module.path.to_str().unwrap().ends_with(".wasm"));
            assert_eq!(module.size, wasm_data.len() as u64);
            assert!(module.metadata.exports.contains(&"init_genesis".to_string()));
        }
    }
    
    #[test]
    fn test_module_limits() {
        let limits = ModuleLimits {
            memory_size: 50 * 1024 * 1024,
            table_elements: 5000,
            instances: 5,
            memories: 2,
        };
        
        let temp_dir = TempDir::new().unwrap();
        let cap_manager = Arc::new(CapabilityManager::new());
        let vfs = Arc::new(VirtualFilesystem::new());
        
        let mut loader = ModuleLoader::new(
            temp_dir.path().to_path_buf(),
            cap_manager,
            vfs,
        ).unwrap();
        
        loader.set_limits(limits.clone());
        assert_eq!(loader.limits.memory_size, 50 * 1024 * 1024);
        assert_eq!(loader.limits.table_elements, 5000);
    }
    
    #[test]
    fn test_module_capabilities_mapping() {
        let temp_dir = TempDir::new().unwrap();
        let cap_manager = Arc::new(CapabilityManager::new());
        let vfs = Arc::new(VirtualFilesystem::new());
        
        let mut loader = ModuleLoader::new(
            temp_dir.path().to_path_buf(),
            cap_manager,
            vfs,
        ).unwrap();
        
        // Add a module config with specific capabilities
        let config = ModuleConfig::new("test".to_string(), PathBuf::from("test.wasm"))
            .requires_capability("read_state".to_string())
            .requires_capability("write_state".to_string())
            .requires_capability("emit_event".to_string());
        
        loader.add_module_config(config);
        
        let capabilities = loader.get_module_capabilities("test");
        assert_eq!(capabilities.len(), 3);
        assert!(capabilities.contains(&Capability::ReadState));
        assert!(capabilities.contains(&Capability::WriteState));
        assert!(capabilities.contains(&Capability::EmitEvent));
        
        // Test default capabilities for unknown module
        let default_caps = loader.get_module_capabilities("unknown");
        assert!(default_caps.contains(&Capability::Log));
        assert!(default_caps.contains(&Capability::AllocateMemory));
    }
}