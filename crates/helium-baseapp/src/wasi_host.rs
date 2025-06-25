//! WASI Runtime Host Infrastructure
//!
//! This module provides the WASI runtime host that enables dynamic loading and execution
//! of WASM modules. It serves as the foundation of the microkernel architecture by providing
//! sandboxed execution environments for blockchain modules.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::{debug, error, info};
use wasmtime::*;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder};

/// WASI Host errors
#[derive(Error, Debug)]
pub enum WasiHostError {
    /// WASM engine configuration failed
    #[error("WASM engine configuration failed: {0}")]
    EngineConfig(String),

    /// Module compilation failed
    #[error("module compilation failed: {0}")]
    ModuleCompilation(#[from] wasmtime::Error),

    /// Module instantiation failed
    #[error("module instantiation failed: {0}")]
    ModuleInstantiation(String),

    /// Module execution failed
    #[error("module execution failed: {0}")]
    ModuleExecution(String),

    /// WASM trap occurred
    #[error("WASM trap: {0}")]
    WasmTrap(#[from] wasmtime::Trap),

    /// Module panic during execution
    #[error("module panic: {0}")]
    ModulePanic(String),

    /// Out of gas/fuel error
    #[error("out of gas: {0}")]
    OutOfGas(String),

    /// Memory limit exceeded
    #[error("memory limit exceeded: {0}")]
    MemoryLimitExceeded(String),

    /// WASI context setup failed
    #[error("WASI context setup failed: {0}")]
    WasiSetup(String),

    /// Module not found
    #[error("module not found: {0}")]
    ModuleNotFound(String),

    /// Invalid module format
    #[error("invalid module format: {0}")]
    InvalidModule(String),

    /// Memory allocation error
    #[error("memory allocation error: {0}")]
    MemoryError(String),

    /// Host function error
    #[error("host function error: {0}")]
    HostFunction(String),
}

pub type Result<T> = std::result::Result<T, WasiHostError>;

/// Module lifecycle state
#[derive(Debug, Clone, PartialEq)]
pub enum ModuleState {
    /// Module is loaded but not initialized
    Loaded,
    /// Module is initialized and ready for execution
    Initialized,
    /// Module is currently executing
    Executing,
    /// Module has finished execution
    Finished,
    /// Module encountered an error
    Error(String),
}

/// WASM module wrapper containing the compiled module and its state
#[derive(Debug)]
pub struct WasmModule {
    /// Module name/identifier
    pub name: String,
    /// Compiled WASM module
    pub module: Module,
    /// Current module state
    pub state: ModuleState,
    /// Module memory limit in bytes
    pub memory_limit: u64,
    /// Module gas limit
    pub gas_limit: u64,
}

impl WasmModule {
    /// Create a new WASM module
    pub fn new(name: String, module: Module, memory_limit: u64, gas_limit: u64) -> Self {
        Self {
            name,
            module,
            state: ModuleState::Loaded,
            memory_limit,
            gas_limit,
        }
    }

    /// Check if module is ready for execution
    pub fn is_ready(&self) -> bool {
        matches!(self.state, ModuleState::Initialized)
    }
}

/// WASI Host instance information
pub struct HostInstance {
    /// Instance store
    pub store: Store<WasiCtx>,
    /// WASM instance
    pub instance: Instance,
    /// Module reference
    pub module_name: String,
}

/// A writer that captures output to a shared buffer
#[derive(Clone)]
pub struct CapturingWriter {
    buffer: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
}

impl CapturingWriter {
    pub fn new(buffer: std::sync::Arc<std::sync::Mutex<Vec<u8>>>) -> Self {
        Self { buffer }
    }
}

impl std::io::Write for CapturingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Execution result for WASI module with input/output
#[derive(Debug)]
pub struct ExecutionResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// WASI Runtime Host
///
/// The host manages WASM module lifecycle and provides sandboxed execution environments.
/// It implements the microkernel pattern where modules are dynamically loaded and executed
/// in isolated WASI environments.
pub struct WasiHost {
    /// WASM runtime engine
    engine: Engine,
    /// Loaded modules registry
    modules: Arc<Mutex<HashMap<String, WasmModule>>>,
    /// Active instances
    instances: Arc<Mutex<HashMap<String, HostInstance>>>,
    /// Default memory limit for modules (in bytes)
    default_memory_limit: u64,
    /// Default gas limit for modules
    default_gas_limit: u64,
}

impl WasiHost {
    /// Create a new WASI host with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(Config::default())
    }

    /// Create a new WASI host with custom configuration
    pub fn with_config(mut config: Config) -> Result<Self> {
        // Configure engine for security and performance
        config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        config.wasm_multi_memory(true);
        config.wasm_memory64(false); // Disable 64-bit memory for security
        config.consume_fuel(true); // Enable fuel consumption for gas metering
        config.epoch_interruption(true); // Enable epoch-based interruption

        let engine =
            Engine::new(&config).map_err(|e| WasiHostError::EngineConfig(e.to_string()))?;

        info!("WASI host initialized with secure configuration");

        Ok(Self {
            engine,
            modules: Arc::new(Mutex::new(HashMap::new())),
            instances: Arc::new(Mutex::new(HashMap::new())),
            default_memory_limit: 16 * 1024 * 1024, // 16MB default
            default_gas_limit: 1_000_000,           // 1M gas units default
        })
    }

    /// Load a WASM module from bytes
    pub fn load_module(&self, name: String, wasm_bytes: &[u8]) -> Result<()> {
        debug!("Loading WASM module: {}", name);

        // Compile the module
        let module = Module::new(&self.engine, wasm_bytes)?;

        // Validate module exports
        self.validate_module_exports(&module)?;

        // Create module wrapper
        let wasm_module = WasmModule::new(
            name.clone(),
            module,
            self.default_memory_limit,
            self.default_gas_limit,
        );

        // Store the module
        let mut modules = self.modules.lock().map_err(|e| {
            WasiHostError::ModuleCompilation(anyhow::anyhow!("Lock poisoned: {}", e))
        })?;
        modules.insert(name.clone(), wasm_module);

        info!("Successfully loaded WASM module: {}", name);
        Ok(())
    }

    /// Load a WASM module from file
    pub fn load_module_from_file(&self, name: String, path: PathBuf) -> Result<()> {
        let wasm_bytes = std::fs::read(&path).map_err(|e| {
            WasiHostError::InvalidModule(format!("Failed to read file {:?}: {}", path, e))
        })?;
        self.load_module(name, &wasm_bytes)
    }

    /// Validate a WASM module without loading it
    pub fn validate_module(&self, wasm_bytes: &[u8]) -> Result<()> {
        debug!("Validating WASM module");
        
        // Compile the module to validate it
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| WasiHostError::ModuleCompilation(e.into()))?;
        
        // Validate module exports
        self.validate_module_exports(&module)?;
        
        info!("WASM module validation successful");
        Ok(())
    }

    /// Initialize a loaded module (create WASI context and instance)
    pub fn initialize_module(&self, name: &str) -> Result<()> {
        debug!("Initializing WASM module: {}", name);

        // Get module from registry
        let module = {
            let modules = self
                .modules
                .lock()
                .map_err(|e| WasiHostError::ModuleInstantiation(format!("Lock poisoned: {}", e)))?;
            modules
                .get(name)
                .ok_or_else(|| WasiHostError::ModuleNotFound(name.to_string()))?
                .module
                .clone()
        };

        // Create WASI context
        let wasi_ctx = WasiCtxBuilder::new().inherit_stdio().inherit_env().build();

        // Create store with WASI context
        let mut store = Store::new(&self.engine, wasi_ctx);

        // Set fuel for gas metering
        store
            .set_fuel(self.default_gas_limit)
            .map_err(|e| WasiHostError::ModuleInstantiation(e.to_string()))?;

        // Create basic linker for now (WASI integration to be completed)
        let mut linker = Linker::new(&self.engine);

        // Add host functions
        self.add_host_functions(&mut linker)?;

        // Instantiate the module
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| WasiHostError::ModuleInstantiation(e.to_string()))?;

        // Store the instance
        let host_instance = HostInstance {
            store,
            instance,
            module_name: name.to_string(),
        };

        let mut instances = self
            .instances
            .lock()
            .map_err(|e| WasiHostError::ModuleInstantiation(format!("Lock poisoned: {}", e)))?;
        instances.insert(name.to_string(), host_instance);

        // Update module state
        {
            let mut modules = self
                .modules
                .lock()
                .map_err(|e| WasiHostError::ModuleInstantiation(format!("Lock poisoned: {}", e)))?;
            if let Some(module) = modules.get_mut(name) {
                module.state = ModuleState::Initialized;
            }
        }

        info!("Successfully initialized WASM module: {}", name);
        Ok(())
    }

    /// Execute a function in a WASM module
    pub fn execute_function(
        &self,
        module_name: &str,
        function_name: &str,
        args: &[Val],
    ) -> Result<Vec<Val>> {
        debug!(
            "Executing function {} in module {}",
            function_name, module_name
        );

        // Update module state to executing
        {
            let mut modules = self
                .modules
                .lock()
                .map_err(|e| WasiHostError::ModuleExecution(format!("Lock poisoned: {}", e)))?;
            if let Some(module) = modules.get_mut(module_name) {
                if !module.is_ready() {
                    return Err(WasiHostError::ModuleExecution(format!(
                        "Module {} is not ready for execution",
                        module_name
                    )));
                }
                module.state = ModuleState::Executing;
            } else {
                return Err(WasiHostError::ModuleNotFound(module_name.to_string()));
            }
        }

        // Execute the function and handle errors properly
        let execution_result = {
            let mut instances = self
                .instances
                .lock()
                .map_err(|e| WasiHostError::ModuleExecution(format!("Lock poisoned: {}", e)))?;

            let host_instance = instances
                .get_mut(module_name)
                .ok_or_else(|| WasiHostError::ModuleNotFound(module_name.to_string()))?;

            // Get the function
            let func = host_instance
                .instance
                .get_func(&mut host_instance.store, function_name)
                .ok_or_else(|| {
                    WasiHostError::ModuleExecution(format!(
                        "Function {} not found in module {}",
                        function_name, module_name
                    ))
                })?;

            // Execute the function with comprehensive error handling
            let mut results = vec![Val::I32(0); func.ty(&host_instance.store).results().len()];

            // Attempt execution with trap handling
            match func.call(&mut host_instance.store, args, &mut results) {
                Ok(_) => {
                    // Log successful execution
                    debug!("Function execution completed successfully");
                    Ok(results)
                }
                Err(e) => {
                    // Check if this is a trap
                    if let Some(trap) = e.downcast_ref::<wasmtime::Trap>() {
                        let error = WasiHostError::WasmTrap(trap.clone());
                        self.set_module_error(module_name, error.to_string()).ok();
                        return Err(error);
                    }

                    // Check for specific error conditions
                    let error_msg = e.to_string();
                    let error = if error_msg.contains("out of fuel") || error_msg.contains("gas") {
                        WasiHostError::OutOfGas(error_msg.clone())
                    } else if error_msg.contains("memory") && error_msg.contains("limit") {
                        WasiHostError::MemoryLimitExceeded(error_msg.clone())
                    } else if error_msg.contains("panic") {
                        WasiHostError::ModulePanic(error_msg.clone())
                    } else {
                        WasiHostError::ModuleExecution(error_msg.clone())
                    };

                    // Set module to error state for serious errors
                    match error {
                        WasiHostError::ModulePanic(_) | WasiHostError::WasmTrap(_) => {
                            self.set_module_error(module_name, error.to_string()).ok();
                        }
                        _ => {} // Non-fatal errors don't change module state
                    }

                    Err(error)
                }
            }
        };

        // Update module state based on execution result
        match &execution_result {
            Ok(_) => {
                // Update module state back to initialized on success
                let mut modules = self
                    .modules
                    .lock()
                    .map_err(|e| WasiHostError::ModuleExecution(format!("Lock poisoned: {}", e)))?;
                if let Some(module) = modules.get_mut(module_name) {
                    module.state = ModuleState::Initialized;
                }
                debug!(
                    "Successfully executed function {} in module {}",
                    function_name, module_name
                );
            }
            Err(e) => {
                // Reset state to initialized for non-fatal errors
                match e {
                    WasiHostError::OutOfGas(_) | WasiHostError::MemoryLimitExceeded(_) => {
                        let mut modules = self.modules.lock().map_err(|e| {
                            WasiHostError::ModuleExecution(format!("Lock poisoned: {}", e))
                        })?;
                        if let Some(module) = modules.get_mut(module_name) {
                            module.state = ModuleState::Initialized;
                        }
                    }
                    _ => {} // Fatal errors keep module in error state
                }
                error!(
                    "Function {} execution failed in module {}: {}",
                    function_name, module_name, e
                );
            }
        }

        execution_result
    }

    /// Cleanup and remove a module
    pub fn cleanup_module(&self, name: &str) -> Result<()> {
        debug!("Cleaning up WASM module: {}", name);

        // Remove instance
        {
            let mut instances = self
                .instances
                .lock()
                .map_err(|e| WasiHostError::ModuleExecution(format!("Lock poisoned: {}", e)))?;
            instances.remove(name);
        }

        // Remove module
        {
            let mut modules = self
                .modules
                .lock()
                .map_err(|e| WasiHostError::ModuleExecution(format!("Lock poisoned: {}", e)))?;
            modules.remove(name);
        }

        info!("Successfully cleaned up WASM module: {}", name);
        Ok(())
    }

    /// Get module state
    pub fn get_module_state(&self, name: &str) -> Result<ModuleState> {
        let modules = self
            .modules
            .lock()
            .map_err(|e| WasiHostError::ModuleExecution(format!("Lock poisoned: {}", e)))?;

        modules
            .get(name)
            .map(|m| m.state.clone())
            .ok_or_else(|| WasiHostError::ModuleNotFound(name.to_string()))
    }

    /// List all loaded modules
    pub fn list_modules(&self) -> Result<Vec<String>> {
        let modules = self
            .modules
            .lock()
            .map_err(|e| WasiHostError::ModuleExecution(format!("Lock poisoned: {}", e)))?;

        Ok(modules.keys().cloned().collect())
    }

    /// Recover a module from error state by reinitializing it
    pub fn recover_module(&self, name: &str) -> Result<()> {
        debug!("Attempting to recover module: {}", name);

        // Check if module exists and is in error state
        {
            let modules = self
                .modules
                .lock()
                .map_err(|e| WasiHostError::ModuleExecution(format!("Lock poisoned: {}", e)))?;

            let module = modules
                .get(name)
                .ok_or_else(|| WasiHostError::ModuleNotFound(name.to_string()))?;

            if !matches!(module.state, ModuleState::Error(_)) {
                return Err(WasiHostError::ModuleExecution(format!(
                    "Module {} is not in error state, cannot recover",
                    name
                )));
            }
        }

        // Clean up current instance
        {
            let mut instances = self
                .instances
                .lock()
                .map_err(|e| WasiHostError::ModuleExecution(format!("Lock poisoned: {}", e)))?;
            instances.remove(name);
        }

        // Reset module state to loaded
        {
            let mut modules = self
                .modules
                .lock()
                .map_err(|e| WasiHostError::ModuleExecution(format!("Lock poisoned: {}", e)))?;
            if let Some(module) = modules.get_mut(name) {
                module.state = ModuleState::Loaded;
            }
        }

        // Reinitialize the module
        self.initialize_module(name)?;

        info!("Successfully recovered module: {}", name);
        Ok(())
    }

    /// Set module to error state
    pub fn set_module_error(&self, name: &str, error: String) -> Result<()> {
        let mut modules = self
            .modules
            .lock()
            .map_err(|e| WasiHostError::ModuleExecution(format!("Lock poisoned: {}", e)))?;

        if let Some(module) = modules.get_mut(name) {
            module.state = ModuleState::Error(error.clone());
            error!("Module {} entered error state: {}", name, error);
        }

        Ok(())
    }

    /// Validate that a module has required exports
    fn validate_module_exports(&self, module: &Module) -> Result<()> {
        let exports: Vec<_> = module.exports().collect();
        debug!(
            "Module exports: {:?}",
            exports.iter().map(|e| e.name()).collect::<Vec<_>>()
        );

        // For now, we don't enforce specific exports, but we log them for debugging
        // In the future, we might require specific functions like _start, init, etc.

        Ok(())
    }

    /// Add host functions to the linker
    fn add_host_functions(&self, linker: &mut Linker<WasiCtx>) -> Result<()> {
        // Add basic host functions for memory management and communication

        // Host function for logging from WASM modules
        linker
            .func_wrap("env", "host_log", |level: i32, ptr: i32, len: i32| {
                // In a real implementation, we would read from WASM memory
                let level_str = match level {
                    0 => "DEBUG",
                    1 => "INFO",
                    2 => "WARN",
                    3 => "ERROR",
                    _ => "UNKNOWN",
                };
                debug!("WASM Log [{}]: ptr={}, len={}", level_str, ptr, len);
            })
            .map_err(|e| WasiHostError::HostFunction(e.to_string()))?;

        // Host function for memory allocation
        linker
            .func_wrap("env", "host_alloc", |size: i32| -> i32 {
                debug!("WASM requested memory allocation: {} bytes", size);
                // Return a mock pointer - in real implementation, this would manage WASM memory
                if size > 0 && size < 1024 * 1024 {
                    // 1MB limit for safety
                    1024 // Mock allocation pointer
                } else {
                    0 // Allocation failed
                }
            })
            .map_err(|e| WasiHostError::HostFunction(e.to_string()))?;

        // Host function for memory deallocation
        linker
            .func_wrap("env", "host_free", |ptr: i32| {
                debug!("WASM freed memory at: {}", ptr);
                // In real implementation, this would free WASM memory
            })
            .map_err(|e| WasiHostError::HostFunction(e.to_string()))?;

        Ok(())
    }

    /// Execute a WASM module with input data and capture output
    pub fn execute_module_with_input(&self, wasm_bytes: &[u8], _input: &[u8]) -> Result<ExecutionResult> {
        debug!("Executing WASM module with input");
        
        // Compile the module
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| WasiHostError::ModuleCompilation(e.into()))?;
        
        // For now, use a simplified approach with inherit_stdio
        // TODO: Implement proper I/O capture using MemoryPipe when API is stable
        let wasi_ctx = WasiCtxBuilder::new()
            .inherit_stdio()
            .inherit_env()
            .build();
        
        let mut store = Store::new(&self.engine, wasi_ctx);
        
        // Set fuel for gas metering
        store.set_fuel(self.default_gas_limit)
            .map_err(|e| WasiHostError::ModuleInstantiation(e.to_string()))?;
        
        // Create WASI linker - using direct import since the API keeps changing
        let mut linker = Linker::new(&self.engine);
        // Add basic host functions for now
        self.add_host_functions(&mut linker)?;
        
        // Instantiate the module
        let instance = linker.instantiate(&mut store, &module)
            .map_err(|e| WasiHostError::ModuleExecution(e.to_string()))?;
        
        // Get the entry point function (looking for 'ante_handle' or '_start')
        let exit_code = if let Ok(func) = instance.get_typed_func::<(), i32>(&mut store, "ante_handle") {
            // Custom entry point
            func.call(&mut store, ())
                .map_err(|e| WasiHostError::ModuleExecution(e.to_string()))?
        } else if let Ok(func) = instance.get_typed_func::<(), ()>(&mut store, "_start") {
            // Standard WASI entry point
            func.call(&mut store, ())
                .map_err(|e| WasiHostError::ModuleExecution(e.to_string()))?;
            0 // Success
        } else {
            return Err(WasiHostError::ModuleExecution(
                "No entry point found (expected 'ante_handle' or '_start')".to_string()
            ));
        };
        
        // For now, return empty output - will be improved with proper I/O capture
        Ok(ExecutionResult {
            exit_code,
            stdout: vec![], // TODO: Capture from pipes
            stderr: vec![], // TODO: Capture from pipes
        })
    }
}

impl Default for WasiHost {
    fn default() -> Self {
        Self::new().expect("Failed to create default WASI host")
    }
}

/// Mock WASI Host for Testing
///
/// MockWasiHost simulates WASI module execution with configurable responses for host function calls.
/// This enables comprehensive testing of WASI-dependent code without requiring actual WASM compilation.
#[cfg(test)]
pub struct MockWasiHost {
    /// Stored modules with their mock state
    modules: Arc<Mutex<HashMap<String, MockWasmModule>>>,
    /// Configurable responses for host function calls
    host_function_responses: Arc<Mutex<HashMap<String, MockHostResponse>>>,
    /// Function execution responses
    function_responses: Arc<Mutex<HashMap<String, MockFunctionResponse>>>,
    /// Module execution responses
    module_responses: Arc<Mutex<HashMap<String, MockExecutionResult>>>,
}

/// Mock WASM module state
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct MockWasmModule {
    pub name: String,
    pub state: ModuleState,
    pub memory_limit: u64,
    pub gas_limit: u64,
    pub wasm_bytes: Vec<u8>,
}

/// Mock response for host function calls
#[cfg(test)]
#[derive(Debug, Clone)]
pub enum MockHostResponse {
    /// Return a fixed value
    Value(i32),
    /// Simulate an error
    Error(String),
    /// Custom response function
    Custom(fn(i32, i32, i32) -> i32),
}

/// Mock response for function execution
#[cfg(test)]
#[derive(Debug, Clone)]
pub enum MockFunctionResponse {
    /// Return fixed values
    Values(Vec<Val>),
    /// Simulate an error
    Error(WasiHostError),
    /// Custom response
    Custom(fn(&str, &[Val]) -> Result<Vec<Val>>),
}

/// Mock execution result for modules
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct MockExecutionResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[cfg(test)]
impl MockWasiHost {
    /// Create a new mock WASI host
    pub fn new() -> Self {
        Self {
            modules: Arc::new(Mutex::new(HashMap::new())),
            host_function_responses: Arc::new(Mutex::new(HashMap::new())),
            function_responses: Arc::new(Mutex::new(HashMap::new())),
            module_responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Configure mock response for a host function
    pub fn set_host_function_response(&self, function_name: &str, response: MockHostResponse) {
        let mut responses = self.host_function_responses.lock().unwrap();
        responses.insert(function_name.to_string(), response);
    }

    /// Configure mock response for function execution
    pub fn set_function_response(&self, function_name: &str, response: MockFunctionResponse) {
        let mut responses = self.function_responses.lock().unwrap();
        responses.insert(function_name.to_string(), response);
    }

    /// Configure mock response for module execution
    pub fn set_module_response(&self, module_name: &str, result: MockExecutionResult) {
        let mut responses = self.module_responses.lock().unwrap();
        responses.insert(module_name.to_string(), result);
    }

    /// Load a WASM module (mock implementation)
    pub fn load_module(&self, name: String, wasm_bytes: &[u8]) -> Result<()> {
        debug!("Mock: Loading WASM module: {}", name);

        let mock_module = MockWasmModule {
            name: name.clone(),
            state: ModuleState::Loaded,
            memory_limit: 16 * 1024 * 1024,
            gas_limit: 1_000_000,
            wasm_bytes: wasm_bytes.to_vec(),
        };

        let mut modules = self.modules.lock().unwrap();
        modules.insert(name.clone(), mock_module);

        info!("Mock: Successfully loaded WASM module: {}", name);
        Ok(())
    }

    /// Load a WASM module from file (mock implementation)
    pub fn load_module_from_file(&self, name: String, path: PathBuf) -> Result<()> {
        let wasm_bytes = std::fs::read(&path).map_err(|e| {
            WasiHostError::InvalidModule(format!("Failed to read file {:?}: {}", path, e))
        })?;
        self.load_module(name, &wasm_bytes)
    }

    /// Validate a WASM module (mock implementation)
    pub fn validate_module(&self, _wasm_bytes: &[u8]) -> Result<()> {
        debug!("Mock: Validating WASM module");
        info!("Mock: WASM module validation successful");
        Ok(())
    }

    /// Initialize a loaded module (mock implementation)
    pub fn initialize_module(&self, name: &str) -> Result<()> {
        debug!("Mock: Initializing WASM module: {}", name);

        let mut modules = self.modules.lock().unwrap();
        let module = modules
            .get_mut(name)
            .ok_or_else(|| WasiHostError::ModuleNotFound(name.to_string()))?;

        module.state = ModuleState::Initialized;

        info!("Mock: Successfully initialized WASM module: {}", name);
        Ok(())
    }

    /// Execute a function in a WASM module (mock implementation)
    pub fn execute_function(
        &self,
        module_name: &str,
        function_name: &str,
        args: &[Val],
    ) -> Result<Vec<Val>> {
        debug!(
            "Mock: Executing function {} in module {}",
            function_name, module_name
        );

        // Check if module exists and is ready
        {
            let mut modules = self.modules.lock().unwrap();
            let module = modules
                .get_mut(module_name)
                .ok_or_else(|| WasiHostError::ModuleNotFound(module_name.to_string()))?;

            if !matches!(module.state, ModuleState::Initialized) {
                return Err(WasiHostError::ModuleExecution(format!(
                    "Module {} is not ready for execution",
                    module_name
                )));
            }
            module.state = ModuleState::Executing;
        }

        // Check for configured response
        let responses = self.function_responses.lock().unwrap();
        let result = if let Some(response) = responses.get(function_name) {
            match response {
                MockFunctionResponse::Values(values) => Ok(values.clone()),
                MockFunctionResponse::Error(error) => Err(error.clone()),
                MockFunctionResponse::Custom(func) => func(function_name, args),
            }
        } else {
            // Default behavior: return success with default values
            Ok(vec![Val::I32(0)])
        };

        // Reset module state
        {
            let mut modules = self.modules.lock().unwrap();
            if let Some(module) = modules.get_mut(module_name) {
                module.state = ModuleState::Initialized;
            }
        }

        match &result {
            Ok(_) => debug!(
                "Mock: Successfully executed function {} in module {}",
                function_name, module_name
            ),
            Err(e) => error!(
                "Mock: Function {} execution failed in module {}: {}",
                function_name, module_name, e
            ),
        }

        result
    }

    /// Execute a WASM module with input data (mock implementation)
    pub fn execute_module_with_input(&self, wasm_bytes: &[u8], input: &[u8]) -> Result<ExecutionResult> {
        debug!("Mock: Executing WASM module with input");

        // Try to find a configured response based on the input or module content
        let module_hash = format!("{:x}", sha2::Sha256::digest(wasm_bytes));
        let responses = self.module_responses.lock().unwrap();
        
        if let Some(mock_result) = responses.get(&module_hash) {
            Ok(ExecutionResult {
                exit_code: mock_result.exit_code,
                stdout: mock_result.stdout.clone(),
                stderr: mock_result.stderr.clone(),
            })
        } else {
            // Default success response
            Ok(ExecutionResult {
                exit_code: 0,
                stdout: b"Mock execution successful".to_vec(),
                stderr: vec![],
            })
        }
    }

    /// Cleanup and remove a module (mock implementation)
    pub fn cleanup_module(&self, name: &str) -> Result<()> {
        debug!("Mock: Cleaning up WASM module: {}", name);

        let mut modules = self.modules.lock().unwrap();
        modules.remove(name);

        info!("Mock: Successfully cleaned up WASM module: {}", name);
        Ok(())
    }

    /// Get module state (mock implementation)
    pub fn get_module_state(&self, name: &str) -> Result<ModuleState> {
        let modules = self.modules.lock().unwrap();
        modules
            .get(name)
            .map(|m| m.state.clone())
            .ok_or_else(|| WasiHostError::ModuleNotFound(name.to_string()))
    }

    /// List all loaded modules (mock implementation)
    pub fn list_modules(&self) -> Result<Vec<String>> {
        let modules = self.modules.lock().unwrap();
        Ok(modules.keys().cloned().collect())
    }

    /// Recover a module from error state (mock implementation)
    pub fn recover_module(&self, name: &str) -> Result<()> {
        debug!("Mock: Attempting to recover module: {}", name);

        let mut modules = self.modules.lock().unwrap();
        let module = modules
            .get_mut(name)
            .ok_or_else(|| WasiHostError::ModuleNotFound(name.to_string()))?;

        if !matches!(module.state, ModuleState::Error(_)) {
            return Err(WasiHostError::ModuleExecution(format!(
                "Module {} is not in error state, cannot recover",
                name
            )));
        }

        module.state = ModuleState::Initialized;

        info!("Mock: Successfully recovered module: {}", name);
        Ok(())
    }

    /// Set module to error state (mock implementation)
    pub fn set_module_error(&self, name: &str, error: String) -> Result<()> {
        let mut modules = self.modules.lock().unwrap();
        if let Some(module) = modules.get_mut(name) {
            module.state = ModuleState::Error(error.clone());
            error!("Mock: Module {} entered error state: {}", name, error);
        }
        Ok(())
    }

    /// Mock host function for logging
    pub fn mock_host_log(&self, level: i32, ptr: i32, len: i32) -> i32 {
        if let Some(MockHostResponse::Value(val)) = self.host_function_responses.lock().unwrap().get("host_log") {
            *val
        } else {
            debug!("Mock: WASM Log [{}]: ptr={}, len={}", level, ptr, len);
            0 // Success
        }
    }

    /// Mock host function for memory allocation
    pub fn mock_host_alloc(&self, size: i32) -> i32 {
        if let Some(MockHostResponse::Value(val)) = self.host_function_responses.lock().unwrap().get("host_alloc") {
            *val
        } else {
            debug!("Mock: WASM requested memory allocation: {} bytes", size);
            if size > 0 && size < 1024 * 1024 {
                1024 // Mock allocation pointer
            } else {
                0 // Allocation failed
            }
        }
    }

    /// Mock host function for memory deallocation
    pub fn mock_host_free(&self, ptr: i32) -> i32 {
        if let Some(MockHostResponse::Value(val)) = self.host_function_responses.lock().unwrap().get("host_free") {
            *val
        } else {
            debug!("Mock: WASM freed memory at: {}", ptr);
            0 // Success
        }
    }
}

#[cfg(test)]
impl Default for MockWasiHost {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasi_host_creation() {
        let host = WasiHost::new();
        assert!(host.is_ok());

        let host = host.unwrap();
        assert_eq!(host.default_memory_limit, 16 * 1024 * 1024);
        assert_eq!(host.default_gas_limit, 1_000_000);
    }

    #[test]
    fn test_module_lifecycle() {
        let host = WasiHost::new().unwrap();

        // Initially no modules
        let modules = host.list_modules().unwrap();
        assert!(modules.is_empty());

        // Test module not found
        let state = host.get_module_state("nonexistent");
        assert!(state.is_err());
        match state.unwrap_err() {
            WasiHostError::ModuleNotFound(name) => assert_eq!(name, "nonexistent"),
            _ => panic!("Expected ModuleNotFound error"),
        }
    }

    #[test]
    fn test_wasm_module_state() {
        let engine = Engine::default();
        let module = Module::new(&engine, b"dummy").unwrap_or_else(|_| {
            // Create a minimal valid WASM module for testing
            let wasm = wat::parse_str(
                r#"
                (module
                    (func (export "test") (result i32)
                        i32.const 42
                    )
                )
            "#,
            )
            .unwrap();
            Module::new(&engine, &wasm).unwrap()
        });

        let wasm_module = WasmModule::new("test".to_string(), module, 1024, 1000);

        assert_eq!(wasm_module.name, "test");
        assert_eq!(wasm_module.state, ModuleState::Loaded);
        assert!(!wasm_module.is_ready());
        assert_eq!(wasm_module.memory_limit, 1024);
        assert_eq!(wasm_module.gas_limit, 1000);
    }

    #[test]
    fn test_mock_wasi_host_creation() {
        let mock_host = MockWasiHost::new();
        
        // Initially no modules
        let modules = mock_host.list_modules().unwrap();
        assert!(modules.is_empty());
    }

    #[test]
    fn test_mock_module_lifecycle() {
        let mock_host = MockWasiHost::new();
        
        // Create a minimal WASM module for testing
        let wasm = wat::parse_str(
            r#"
            (module
                (func (export "test") (result i32)
                    i32.const 42
                )
            )
        "#,
        ).unwrap();

        // Load module
        mock_host.load_module("test_module".to_string(), &wasm).unwrap();
        
        // Check module is loaded
        let modules = mock_host.list_modules().unwrap();
        assert_eq!(modules.len(), 1);
        assert!(modules.contains(&"test_module".to_string()));
        
        // Check initial state
        let state = mock_host.get_module_state("test_module").unwrap();
        assert_eq!(state, ModuleState::Loaded);
        
        // Initialize module
        mock_host.initialize_module("test_module").unwrap();
        
        // Check initialized state
        let state = mock_host.get_module_state("test_module").unwrap();
        assert_eq!(state, ModuleState::Initialized);
        
        // Cleanup module
        mock_host.cleanup_module("test_module").unwrap();
        
        // Check module is removed
        let modules = mock_host.list_modules().unwrap();
        assert!(modules.is_empty());
    }

    #[test]
    fn test_mock_function_execution() {
        let mock_host = MockWasiHost::new();
        
        // Create and load a test module
        let wasm = wat::parse_str(
            r#"
            (module
                (func (export "test_func") (result i32)
                    i32.const 42
                )
            )
        "#,
        ).unwrap();
        
        mock_host.load_module("test_module".to_string(), &wasm).unwrap();
        mock_host.initialize_module("test_module").unwrap();
        
        // Test default function execution
        let result = mock_host.execute_function("test_module", "test_func", &[]).unwrap();
        assert_eq!(result, vec![Val::I32(0)]); // Default mock response
        
        // Configure custom response
        mock_host.set_function_response(
            "test_func",
            MockFunctionResponse::Values(vec![Val::I32(42)]),
        );
        
        // Test custom response
        let result = mock_host.execute_function("test_module", "test_func", &[]).unwrap();
        assert_eq!(result, vec![Val::I32(42)]);
        
        // Configure error response
        mock_host.set_function_response(
            "test_func",
            MockFunctionResponse::Error(WasiHostError::ModuleExecution("Test error".to_string())),
        );
        
        // Test error response
        let result = mock_host.execute_function("test_module", "test_func", &[]);
        assert!(result.is_err());
        match result.unwrap_err() {
            WasiHostError::ModuleExecution(msg) => assert_eq!(msg, "Test error"),
            _ => panic!("Expected ModuleExecution error"),
        }
    }

    #[test]
    fn test_mock_host_functions() {
        let mock_host = MockWasiHost::new();
        
        // Test default host function behavior
        assert_eq!(mock_host.mock_host_log(1, 100, 10), 0);
        assert_eq!(mock_host.mock_host_alloc(1024), 1024);
        assert_eq!(mock_host.mock_host_free(1024), 0);
        
        // Configure custom responses
        mock_host.set_host_function_response("host_log", MockHostResponse::Value(42));
        mock_host.set_host_function_response("host_alloc", MockHostResponse::Value(2048));
        mock_host.set_host_function_response("host_free", MockHostResponse::Value(1));
        
        // Test custom responses
        assert_eq!(mock_host.mock_host_log(1, 100, 10), 42);
        assert_eq!(mock_host.mock_host_alloc(1024), 2048);
        assert_eq!(mock_host.mock_host_free(1024), 1);
    }

    #[test]
    fn test_mock_module_execution_with_input() {
        let mock_host = MockWasiHost::new();
        
        // Create a test WASM module
        let wasm = wat::parse_str(
            r#"
            (module
                (func (export "_start")
                    ;; Simple start function
                )
            )
        "#,
        ).unwrap();
        
        // Test default execution
        let result = mock_host.execute_module_with_input(&wasm, b"test input").unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, b"Mock execution successful");
        assert!(result.stderr.is_empty());
        
        // Configure custom response
        let module_hash = format!("{:x}", sha2::Sha256::digest(&wasm));
        mock_host.set_module_response(&module_hash, MockExecutionResult {
            exit_code: 42,
            stdout: b"Custom output".to_vec(),
            stderr: b"Custom error".to_vec(),
        });
        
        // Test custom response
        let result = mock_host.execute_module_with_input(&wasm, b"test input").unwrap();
        assert_eq!(result.exit_code, 42);
        assert_eq!(result.stdout, b"Custom output");
        assert_eq!(result.stderr, b"Custom error");
    }

    #[test]
    fn test_mock_module_error_recovery() {
        let mock_host = MockWasiHost::new();
        
        // Create and load a test module
        let wasm = wat::parse_str(
            r#"
            (module
                (func (export "test") (result i32)
                    i32.const 42
                )
            )
        "#,
        ).unwrap();
        
        mock_host.load_module("test_module".to_string(), &wasm).unwrap();
        mock_host.initialize_module("test_module").unwrap();
        
        // Set module to error state
        mock_host.set_module_error("test_module", "Test error".to_string()).unwrap();
        
        // Check error state
        let state = mock_host.get_module_state("test_module").unwrap();
        assert!(matches!(state, ModuleState::Error(_)));
        
        // Recover module
        mock_host.recover_module("test_module").unwrap();
        
        // Check recovered state
        let state = mock_host.get_module_state("test_module").unwrap();
        assert_eq!(state, ModuleState::Initialized);
    }

    #[test]
    fn test_mock_module_validation() {
        let mock_host = MockWasiHost::new();
        
        // Test module validation (always succeeds in mock)
        let wasm = wat::parse_str(
            r#"
            (module
                (func (export "test") (result i32)
                    i32.const 42
                )
            )
        "#,
        ).unwrap();
        
        let result = mock_host.validate_module(&wasm);
        assert!(result.is_ok());
        
        // Even invalid bytes should pass in mock
        let result = mock_host.validate_module(b"invalid wasm");
        assert!(result.is_ok());
    }
}
