//! Host-Guest ABI & Communication Protocol
//!
//! This module defines the standardized WASI ABI for Cosmos SDK modules, providing
//! a formal communication protocol between the host (Rust runtime) and guest (WASM modules).
//!
//! The ABI follows these principles:
//! - Error codes: success=0, errors=non-zero (following Unix conventions)
//! - Memory management: Guest allocates, host validates and accesses
//! - Protobuf serialization: All complex data exchange uses protobuf
//! - Stderr capture: Detailed error messages captured from guest
//! - Capability-based security: Host functions require proper capabilities

use std::sync::{Arc, Mutex};

use helium_store::KVStore;
use helium_types::SdkError;
use thiserror::Error;
use tracing::{debug, error, info, warn};
use wasmtime::{AsContextMut, *};

use crate::capabilities::CapabilityManager;
use crate::vfs::VirtualFilesystem;

/// ABI error types
#[derive(Error, Debug)]
pub enum AbiError {
    /// Invalid memory access
    #[error("invalid memory access: {0}")]
    InvalidMemoryAccess(String),

    /// Invalid pointer or size
    #[error("invalid pointer or size: ptr={ptr}, size={size}")]
    InvalidPointer { ptr: u32, size: u32 },

    /// Protobuf serialization/deserialization failed
    #[error("protobuf error: {0}")]
    ProtobufError(String),

    /// Memory allocation failed
    #[error("memory allocation failed: {0}")]
    AllocationFailed(String),

    /// Host function not available
    #[error("host function not available: {0}")]
    FunctionNotAvailable(String),

    /// Capability check failed
    #[error("capability check failed: {0}")]
    CapabilityError(String),

    /// Store operation failed
    #[error("store operation failed: {0}")]
    StoreError(String),

    /// SDK error propagation
    #[error("SDK error: {0}")]
    SdkError(#[from] SdkError),

    /// WASM execution error
    #[error("WASM execution error: {0}")]
    ExecutionError(String),
}

pub type Result<T> = std::result::Result<T, AbiError>;

/// ABI result codes following Unix conventions
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AbiResultCode {
    /// Success
    Success = 0,
    /// Generic error
    Error = 1,
    /// Invalid argument
    InvalidArg = 2,
    /// Permission denied (capability error)
    PermissionDenied = 3,
    /// Not found
    NotFound = 4,
    /// Out of memory
    OutOfMemory = 5,
    /// Invalid operation
    InvalidOperation = 6,
    /// Serialization error
    SerializationError = 7,
    /// Store error
    StoreError = 8,
}

impl From<AbiError> for AbiResultCode {
    fn from(error: AbiError) -> Self {
        match error {
            AbiError::InvalidMemoryAccess(_) | AbiError::InvalidPointer { .. } => {
                AbiResultCode::InvalidArg
            }
            AbiError::ProtobufError(_) => AbiResultCode::SerializationError,
            AbiError::AllocationFailed(_) => AbiResultCode::OutOfMemory,
            AbiError::FunctionNotAvailable(_) | AbiError::CapabilityError(_) => {
                AbiResultCode::PermissionDenied
            }
            AbiError::StoreError(_) => AbiResultCode::StoreError,
            AbiError::SdkError(_) => AbiResultCode::Error,
            AbiError::ExecutionError(_) => AbiResultCode::InvalidOperation,
        }
    }
}

/// Memory region descriptor for host-guest data exchange
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    /// Pointer to data in WASM memory
    pub ptr: u32,
    /// Size of data in bytes  
    pub size: u32,
}

impl MemoryRegion {
    /// Create a new memory region
    pub fn new(ptr: u32, size: u32) -> Self {
        Self { ptr, size }
    }

    /// Check if the region is valid (non-null, positive size)
    pub fn is_valid(&self) -> bool {
        self.ptr > 0 && self.size > 0
    }
}

/// Capability types for host function access control
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Read access to blockchain state
    ReadState,
    /// Write access to blockchain state  
    WriteState,
    /// Send messages to other modules
    SendMessage,
    /// Access transaction data
    AccessTransaction,
    /// Emit events
    EmitEvent,
    /// Access current block information
    AccessBlock,
    /// Logging capability
    Log,
    /// Memory allocation capability
    AllocateMemory,
}

/// Context for ABI function execution containing capabilities and state
pub struct AbiContext {
    /// Available capabilities for the current module
    pub capabilities: Vec<Capability>,
    /// Module identifier
    pub module_id: String,
    /// Current transaction context (if any)
    pub tx_context: Option<String>,
    /// Stderr capture buffer
    pub stderr_buffer: Arc<Mutex<Vec<u8>>>,
    /// Store reference for state operations
    pub store: Option<Arc<dyn KVStore + Send + Sync>>,
    /// Virtual filesystem for WASI state access
    pub vfs: Option<Arc<VirtualFilesystem>>,
    /// Capability manager for access control
    pub capability_manager: Option<Arc<CapabilityManager>>,
}

impl AbiContext {
    /// Create a new ABI context
    pub fn new(module_id: String, capabilities: Vec<Capability>) -> Self {
        Self {
            capabilities,
            module_id,
            tx_context: None,
            stderr_buffer: Arc::new(Mutex::new(Vec::new())),
            store: None,
            vfs: None,
            capability_manager: None,
        }
    }

    /// Set the VFS instance
    pub fn set_vfs(&mut self, vfs: Arc<VirtualFilesystem>) {
        self.vfs = Some(vfs);
    }

    /// Set the capability manager
    pub fn set_capability_manager(&mut self, cap_manager: Arc<CapabilityManager>) {
        self.capability_manager = Some(cap_manager);
    }

    /// Check if the context has a specific capability
    pub fn has_capability(&self, capability: &Capability) -> bool {
        self.capabilities.contains(capability)
    }

    /// Require a capability, returning error if not available
    pub fn require_capability(&self, capability: &Capability) -> Result<()> {
        if self.has_capability(capability) {
            Ok(())
        } else {
            Err(AbiError::CapabilityError(format!(
                "Module {} missing capability {:?}",
                self.module_id, capability
            )))
        }
    }

    /// Write to stderr buffer
    pub fn write_stderr(&self, data: &[u8]) -> Result<()> {
        let mut buffer = self
            .stderr_buffer
            .lock()
            .map_err(|e| AbiError::ExecutionError(format!("Stderr lock poisoned: {}", e)))?;
        buffer.extend_from_slice(data);
        Ok(())
    }

    /// Get and clear stderr buffer contents
    pub fn take_stderr(&self) -> Result<Vec<u8>> {
        let mut buffer = self
            .stderr_buffer
            .lock()
            .map_err(|e| AbiError::ExecutionError(format!("Stderr lock poisoned: {}", e)))?;
        Ok(std::mem::take(&mut *buffer))
    }
}

/// Host-side memory manager for WASM memory access
pub struct MemoryManager {
    /// WASM memory instance
    memory: Memory,
}

impl MemoryManager {
    /// Create a new memory manager
    pub fn new(memory: Memory) -> Self {
        Self { memory }
    }

    /// Read data from WASM memory
    pub fn read_memory(
        &self,
        store: &mut impl AsContextMut,
        region: &MemoryRegion,
    ) -> Result<Vec<u8>> {
        if !region.is_valid() {
            return Err(AbiError::InvalidPointer {
                ptr: region.ptr,
                size: region.size,
            });
        }

        let data = self.memory.data(store);
        let start = region.ptr as usize;
        let end = start.saturating_add(region.size as usize);

        if end > data.len() {
            return Err(AbiError::InvalidMemoryAccess(format!(
                "Read beyond memory bounds: {}-{} > {}",
                start,
                end,
                data.len()
            )));
        }

        Ok(data[start..end].to_vec())
    }

    /// Write data to WASM memory
    pub fn write_memory(
        &self,
        store: &mut impl AsContextMut,
        region: &MemoryRegion,
        data: &[u8],
    ) -> Result<()> {
        if !region.is_valid() {
            return Err(AbiError::InvalidPointer {
                ptr: region.ptr,
                size: region.size,
            });
        }

        if data.len() != region.size as usize {
            return Err(AbiError::InvalidPointer {
                ptr: region.ptr,
                size: region.size,
            });
        }

        let memory_data = self.memory.data_mut(store);
        let start = region.ptr as usize;
        let end = start.saturating_add(region.size as usize);

        if end > memory_data.len() {
            return Err(AbiError::InvalidMemoryAccess(format!(
                "Write beyond memory bounds: {}-{} > {}",
                start,
                end,
                memory_data.len()
            )));
        }

        memory_data[start..end].copy_from_slice(data);
        Ok(())
    }

    /// Read a null-terminated string from WASM memory
    pub fn read_string(&self, store: &mut impl AsContextMut, ptr: u32) -> Result<String> {
        let data = self.memory.data(store);
        let start = ptr as usize;

        if start >= data.len() {
            return Err(AbiError::InvalidMemoryAccess(format!(
                "String pointer {} beyond memory bounds {}",
                start,
                data.len()
            )));
        }

        // Find null terminator
        let end = data[start..]
            .iter()
            .position(|&b| b == 0)
            .map(|pos| start + pos)
            .unwrap_or(data.len());

        String::from_utf8(data[start..end].to_vec())
            .map_err(|e| AbiError::InvalidMemoryAccess(format!("Invalid UTF-8 string: {}", e)))
    }

    /// Write a string to WASM memory (null-terminated)
    pub fn write_string(&self, store: &mut impl AsContextMut, ptr: u32, s: &str) -> Result<()> {
        let data = s.as_bytes();
        let memory_data = self.memory.data_mut(store);
        let start = ptr as usize;

        if start + data.len() + 1 > memory_data.len() {
            return Err(AbiError::InvalidMemoryAccess(format!(
                "String write beyond memory bounds: {}+{} > {}",
                start,
                data.len() + 1,
                memory_data.len()
            )));
        }

        memory_data[start..start + data.len()].copy_from_slice(data);
        memory_data[start + data.len()] = 0; // Null terminator

        Ok(())
    }
}

/// Protobuf serialization helpers for host-guest data exchange
pub struct ProtobufHelper;

impl ProtobufHelper {
    /// Serialize a protobuf message to bytes
    pub fn serialize<T: prost::Message>(msg: &T) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        msg.encode(&mut buf)
            .map_err(|e| AbiError::ProtobufError(format!("Encoding failed: {}", e)))?;
        Ok(buf)
    }

    /// Deserialize bytes to a protobuf message
    pub fn deserialize<T: prost::Message + Default>(data: &[u8]) -> Result<T> {
        T::decode(data).map_err(|e| AbiError::ProtobufError(format!("Decoding failed: {}", e)))
    }

    /// Serialize and write protobuf message to WASM memory
    pub fn write_protobuf_to_memory<T: prost::Message>(
        memory_manager: &MemoryManager,
        store: &mut impl AsContextMut,
        region: &MemoryRegion,
        msg: &T,
    ) -> Result<()> {
        let data = Self::serialize(msg)?;

        if data.len() != region.size as usize {
            return Err(AbiError::ProtobufError(format!(
                "Serialized size {} doesn't match region size {}",
                data.len(),
                region.size
            )));
        }

        memory_manager.write_memory(store, region, &data)
    }

    /// Read and deserialize protobuf message from WASM memory
    pub fn read_protobuf_from_memory<T: prost::Message + Default>(
        memory_manager: &MemoryManager,
        store: &mut impl AsContextMut,
        region: &MemoryRegion,
    ) -> Result<T> {
        let data = memory_manager.read_memory(store, region)?;
        Self::deserialize(&data)
    }
}

/// Standard host functions exported to WASM modules
pub struct HostFunctions;

impl HostFunctions {
    /// Add all standard host functions to a linker
    pub fn add_to_linker(linker: &mut Linker<AbiContext>) -> Result<()> {
        Self::add_logging_functions(linker)?;
        Self::add_memory_functions(linker)?;
        Self::add_state_functions(linker)?;
        Self::add_transaction_functions(linker)?;
        Self::add_utility_functions(linker)?;
        Self::add_ipc_functions(linker)?;
        Self::add_capability_functions(linker)?;
        Ok(())
    }

    /// Add logging functions
    fn add_logging_functions(linker: &mut Linker<AbiContext>) -> Result<()> {
        // host_log(level: i32, ptr: u32, len: u32) -> i32
        linker
            .func_wrap(
                "env",
                "host_log",
                |mut caller: Caller<'_, AbiContext>, level: i32, ptr: u32, len: u32| -> i32 {
                    // Check logging capability and get module ID before mutable borrow
                    let (has_capability, module_id) = {
                        let context = caller.data();
                        let has_cap = context.has_capability(&Capability::Log);
                        (has_cap, context.module_id.clone())
                    };

                    if !has_capability {
                        error!("Log capability check failed: missing Log capability");
                        return AbiResultCode::PermissionDenied as i32;
                    }

                    // Get memory and read log message
                    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                        Some(memory) => memory,
                        None => {
                            error!("Failed to get WASM memory for logging");
                            return AbiResultCode::InvalidOperation as i32;
                        }
                    };

                    let memory_manager = MemoryManager::new(memory);
                    let region = MemoryRegion::new(ptr, len);

                    let message = match memory_manager.read_memory(&mut caller, &region) {
                        Ok(data) => String::from_utf8_lossy(&data).to_string(),
                        Err(e) => {
                            error!("Failed to read log message from WASM memory: {}", e);
                            return AbiResultCode::InvalidArg as i32;
                        }
                    };

                    // Log message with appropriate level
                    match level {
                        0 => debug!("[WASM:{}] {}", module_id, message),
                        1 => info!("[WASM:{}] {}", module_id, message),
                        2 => warn!("[WASM:{}] {}", module_id, message),
                        3 => error!("[WASM:{}] {}", module_id, message),
                        _ => debug!("[WASM:{}] UNKNOWN({}): {}", module_id, level, message),
                    }

                    AbiResultCode::Success as i32
                },
            )
            .map_err(|e| AbiError::ExecutionError(e.to_string()))?;
        Ok(())
    }

    /// Add memory management functions
    fn add_memory_functions(linker: &mut Linker<AbiContext>) -> Result<()> {
        // host_alloc(size: u32) -> u32 (returns pointer, 0 on failure)
        linker
            .func_wrap(
                "env",
                "host_alloc",
                |caller: Caller<'_, AbiContext>, size: u32| -> u32 {
                    let context = caller.data();

                    // Check memory allocation capability
                    if let Err(e) = context.require_capability(&Capability::AllocateMemory) {
                        error!("Memory allocation capability check failed: {}", e);
                        return 0;
                    }

                    // Basic size validation
                    if size == 0 || size > 1024 * 1024 {
                        // 1MB limit for safety
                        error!("Invalid memory allocation size: {}", size);
                        return 0;
                    }

                    debug!("WASM module {} allocated {} bytes", context.module_id, size);

                    // In a real implementation, this would manage actual WASM memory
                    // For now, return a mock pointer > 0 to indicate success
                    1024 + size // Mock allocation
                },
            )
            .map_err(|e| AbiError::ExecutionError(e.to_string()))?;

        // host_free(ptr: u32) -> i32
        linker
            .func_wrap(
                "env",
                "host_free",
                |caller: Caller<'_, AbiContext>, ptr: u32| -> i32 {
                    let context = caller.data();

                    // Check memory allocation capability
                    if let Err(e) = context.require_capability(&Capability::AllocateMemory) {
                        error!("Memory free capability check failed: {}", e);
                        return AbiResultCode::PermissionDenied as i32;
                    }

                    if ptr == 0 {
                        error!("Attempted to free null pointer");
                        return AbiResultCode::InvalidArg as i32;
                    }

                    debug!("WASM module {} freed memory at {}", context.module_id, ptr);

                    // In a real implementation, this would free actual WASM memory
                    AbiResultCode::Success as i32
                },
            )
            .map_err(|e| AbiError::ExecutionError(e.to_string()))?;

        Ok(())
    }

    /// Add state access functions
    fn add_state_functions(linker: &mut Linker<AbiContext>) -> Result<()> {
        // host_state_get(key_ptr: u32, key_len: u32, value_ptr: u32, value_len_ptr: u32) -> i32
        linker
            .func_wrap(
                "env",
                "host_state_get",
                |mut caller: Caller<'_, AbiContext>,
                 key_ptr: u32,
                 key_len: u32,
                 value_ptr: u32,
                 value_len_ptr: u32|
                 -> i32 {
                    // Get context references before any mutable borrow
                    let (module_id, has_vfs) = {
                        let context = caller.data();
                        (context.module_id.clone(), context.vfs.is_some())
                    };

                    if !has_vfs {
                        error!("No VFS available in context for module {}", module_id);
                        return AbiResultCode::InvalidOperation as i32;
                    }

                    // Get memory and read key from WASM memory
                    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                        Some(memory) => memory,
                        None => {
                            error!("Failed to get WASM memory for state access");
                            return AbiResultCode::InvalidOperation as i32;
                        }
                    };

                    let memory_manager = MemoryManager::new(memory);
                    let key_region = MemoryRegion::new(key_ptr, key_len);

                    let key_bytes = match memory_manager.read_memory(&mut caller, &key_region) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Failed to read key from WASM memory: {}", e);
                            return AbiResultCode::InvalidArg as i32;
                        }
                    };

                    // Convert key to path format: /state/{module_id}/{key}
                    let key_str = String::from_utf8_lossy(&key_bytes);
                    let path = format!("/state/{}/{}", module_id, key_str);

                    // Get VFS reference and perform read
                    let vfs = caller.data().vfs.as_ref().unwrap().clone();

                    // Open file for reading through VFS
                    let fd = match vfs.open(std::path::Path::new(&path), false) {
                        Ok(fd) => fd,
                        Err(e) => {
                            debug!("VFS file not found for key '{}': {}", key_str, e);
                            return AbiResultCode::NotFound as i32;
                        }
                    };

                    // Read file content
                    let mut buffer = Vec::new();
                    let mut chunk = vec![0u8; 4096];
                    loop {
                        match vfs.read(fd, &mut chunk) {
                            Ok(0) => break, // EOF
                            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                            Err(e) => {
                                error!("Failed to read from VFS: {}", e);
                                let _ = vfs.close(fd);
                                return AbiResultCode::StoreError as i32;
                            }
                        }
                    }

                    // Close the file
                    if let Err(e) = vfs.close(fd) {
                        error!("Failed to close VFS file: {}", e);
                    }

                    // Write result to WASM memory
                    let value_region = MemoryRegion::new(value_ptr, buffer.len() as u32);
                    let value_len_region = MemoryRegion::new(value_len_ptr, 4); // u32 size

                    // Get fresh memory reference after VFS operations
                    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                        Some(memory) => memory,
                        None => {
                            error!("Failed to get WASM memory for writing result");
                            return AbiResultCode::InvalidOperation as i32;
                        }
                    };

                    let memory_manager = MemoryManager::new(memory);

                    // Write the value
                    if let Err(e) = memory_manager.write_memory(&mut caller, &value_region, &buffer)
                    {
                        error!("Failed to write value to WASM memory: {}", e);
                        return AbiResultCode::InvalidArg as i32;
                    }

                    // Write the length
                    let len_bytes = (buffer.len() as u32).to_le_bytes();
                    if let Err(e) =
                        memory_manager.write_memory(&mut caller, &value_len_region, &len_bytes)
                    {
                        error!("Failed to write value length to WASM memory: {}", e);
                        return AbiResultCode::InvalidArg as i32;
                    }

                    debug!(
                        "WASM module {} successfully read state key '{}' ({} bytes)",
                        module_id,
                        key_str,
                        buffer.len()
                    );

                    AbiResultCode::Success as i32
                },
            )
            .map_err(|e| AbiError::ExecutionError(e.to_string()))?;
        // host_state_set(key_ptr: u32, key_len: u32, value_ptr: u32, value_len: u32) -> i32
        linker
            .func_wrap(
                "env",
                "host_state_set",
                |mut caller: Caller<'_, AbiContext>,
                 key_ptr: u32,
                 key_len: u32,
                 value_ptr: u32,
                 value_len: u32|
                 -> i32 {
                    // Get context references before any mutable borrow
                    let (module_id, has_vfs) = {
                        let context = caller.data();
                        (context.module_id.clone(), context.vfs.is_some())
                    };

                    if !has_vfs {
                        error!("No VFS available in context for module {}", module_id);
                        return AbiResultCode::InvalidOperation as i32;
                    }

                    // Get memory and read key/value from WASM memory
                    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                        Some(memory) => memory,
                        None => {
                            error!("Failed to get WASM memory for state write");
                            return AbiResultCode::InvalidOperation as i32;
                        }
                    };

                    let memory_manager = MemoryManager::new(memory);
                    let key_region = MemoryRegion::new(key_ptr, key_len);
                    let value_region = MemoryRegion::new(value_ptr, value_len);

                    let key_bytes = match memory_manager.read_memory(&mut caller, &key_region) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Failed to read key from WASM memory: {}", e);
                            return AbiResultCode::InvalidArg as i32;
                        }
                    };

                    let value_bytes = match memory_manager.read_memory(&mut caller, &value_region) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Failed to read value from WASM memory: {}", e);
                            return AbiResultCode::InvalidArg as i32;
                        }
                    };

                    // Convert key to path format: /state/{module_id}/{key}
                    let key_str = String::from_utf8_lossy(&key_bytes);
                    let path = format!("/state/{}/{}", module_id, key_str);

                    // Get VFS reference
                    let vfs = caller.data().vfs.as_ref().unwrap().clone();

                    // Open file for writing through VFS (create if not exists)
                    let fd = match vfs.open(std::path::Path::new(&path), true) {
                        Ok(fd) => fd,
                        Err(_) => {
                            // Try to create the file
                            match vfs.create(std::path::Path::new(&path)) {
                                Ok(fd) => fd,
                                Err(e) => {
                                    error!(
                                        "Failed to create VFS file for key '{}': {}",
                                        key_str, e
                                    );
                                    return AbiResultCode::StoreError as i32;
                                }
                            }
                        }
                    };

                    // Write value to file
                    match vfs.write(fd, &value_bytes) {
                        Ok(written) => {
                            if written != value_bytes.len() {
                                error!(
                                    "Partial write: expected {} bytes, wrote {}",
                                    value_bytes.len(),
                                    written
                                );
                                let _ = vfs.close(fd);
                                return AbiResultCode::StoreError as i32;
                            }
                        }
                        Err(e) => {
                            error!("Failed to write to VFS: {}", e);
                            let _ = vfs.close(fd);
                            return AbiResultCode::StoreError as i32;
                        }
                    }

                    // Close the file (commits the write)
                    if let Err(e) = vfs.close(fd) {
                        error!("Failed to close VFS file: {}", e);
                        return AbiResultCode::StoreError as i32;
                    }

                    debug!(
                        "WASM module {} successfully wrote state key '{}' ({} bytes)",
                        module_id,
                        key_str,
                        value_bytes.len()
                    );

                    AbiResultCode::Success as i32
                },
            )
            .map_err(|e| AbiError::ExecutionError(e.to_string()))?;

        Ok(())
    }

    /// Add transaction functions
    fn add_transaction_functions(linker: &mut Linker<AbiContext>) -> Result<()> {
        // host_get_tx_data(ptr: u32, len_ptr: u32) -> i32
        linker
            .func_wrap(
                "env",
                "host_get_tx_data",
                |caller: Caller<'_, AbiContext>, _ptr: u32, _len_ptr: u32| -> i32 {
                    let context = caller.data();

                    // Check transaction access capability
                    if let Err(e) = context.require_capability(&Capability::AccessTransaction) {
                        error!("Transaction access capability check failed: {}", e);
                        return AbiResultCode::PermissionDenied as i32;
                    }

                    debug!(
                        "WASM module {} requesting transaction data",
                        context.module_id
                    );

                    // Implementation would serialize current transaction data and write to WASM memory
                    AbiResultCode::Success as i32
                },
            )
            .map_err(|e| AbiError::ExecutionError(e.to_string()))?;

        Ok(())
    }

    /// Add utility functions
    fn add_utility_functions(linker: &mut Linker<AbiContext>) -> Result<()> {
        // host_emit_event(event_ptr: u32, event_len: u32) -> i32
        linker
            .func_wrap(
                "env",
                "host_emit_event",
                |caller: Caller<'_, AbiContext>, event_ptr: u32, event_len: u32| -> i32 {
                    let context = caller.data();

                    // Check emit event capability
                    if let Err(e) = context.require_capability(&Capability::EmitEvent) {
                        error!("Emit event capability check failed: {}", e);
                        return AbiResultCode::PermissionDenied as i32;
                    }

                    debug!(
                        "WASM module {} emitting event: ptr={}, len={}",
                        context.module_id, event_ptr, event_len
                    );

                    // Implementation would read event data from WASM memory and emit it
                    AbiResultCode::Success as i32
                },
            )
            .map_err(|e| AbiError::ExecutionError(e.to_string()))?;

        // host_abort(message_ptr: u32, message_len: u32) -> i32 (does not return)
        linker
            .func_wrap(
                "env",
                "host_abort",
                |mut caller: Caller<'_, AbiContext>, message_ptr: u32, message_len: u32| -> i32 {
                    // Get module ID before mutable borrow
                    let module_id = caller.data().module_id.clone();

                    // Get memory and read abort message
                    if let Some(memory) = caller.get_export("memory").and_then(|e| e.into_memory())
                    {
                        let memory_manager = MemoryManager::new(memory);
                        let region = MemoryRegion::new(message_ptr, message_len);

                        if let Ok(message_bytes) = memory_manager.read_memory(&mut caller, &region)
                        {
                            let message = String::from_utf8_lossy(&message_bytes);
                            error!("WASM module {} aborted: {}", module_id, message);

                            // Write to stderr buffer after releasing the mutable borrow
                            let context = caller.data();
                            if let Err(e) = context.write_stderr(&message_bytes) {
                                error!("Failed to write abort message to stderr: {}", e);
                            }
                        } else {
                            error!(
                                "WASM module {} aborted with invalid message pointer",
                                module_id
                            );
                        }
                    }

                    // This should trigger a trap to halt execution
                    AbiResultCode::Error as i32
                },
            )
            .map_err(|e| AbiError::ExecutionError(e.to_string()))?;
        Ok(())
    }

    /// Add IPC (Inter-Process Communication) functions for module-to-module communication
    fn add_ipc_functions(linker: &mut Linker<AbiContext>) -> Result<()> {
        // host_ipc_send(module_ptr: u32, module_len: u32, msg_ptr: u32, msg_len: u32) -> i32
        linker
            .func_wrap(
                "env",
                "host_ipc_send",
                |mut caller: Caller<'_, AbiContext>,
                 module_ptr: u32,
                 module_len: u32,
                 msg_ptr: u32,
                 msg_len: u32|
                 -> i32 {
                    // Get context references
                    let (sender_module, has_cap_manager) = {
                        let context = caller.data();
                        (
                            context.module_id.clone(),
                            context.capability_manager.is_some(),
                        )
                    };

                    if !has_cap_manager {
                        error!("No capability manager available for IPC");
                        return AbiResultCode::InvalidOperation as i32;
                    }

                    // Get memory and read target module name and message
                    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                        Some(memory) => memory,
                        None => {
                            error!("Failed to get WASM memory for IPC send");
                            return AbiResultCode::InvalidOperation as i32;
                        }
                    };

                    let memory_manager = MemoryManager::new(memory);
                    let module_region = MemoryRegion::new(module_ptr, module_len);
                    let msg_region = MemoryRegion::new(msg_ptr, msg_len);

                    let target_module_bytes =
                        match memory_manager.read_memory(&mut caller, &module_region) {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                error!("Failed to read target module name from WASM memory: {}", e);
                                return AbiResultCode::InvalidArg as i32;
                            }
                        };

                    let msg_bytes = match memory_manager.read_memory(&mut caller, &msg_region) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Failed to read message from WASM memory: {}", e);
                            return AbiResultCode::InvalidArg as i32;
                        }
                    };

                    let target_module = String::from_utf8_lossy(&target_module_bytes);

                    // Check if sender has permission to send messages to target
                    let cap_manager = caller.data().capability_manager.as_ref().unwrap().clone();

                    let send_cap =
                        crate::capabilities::CapabilityType::SendMessage(target_module.to_string());
                    match cap_manager.require_capability(&sender_module, &send_cap) {
                        Ok(_) => {}
                        Err(e) => {
                            error!(
                                "Module {} lacks capability to send messages to {}: {}",
                                sender_module, target_module, e
                            );
                            return AbiResultCode::PermissionDenied as i32;
                        }
                    }

                    // TODO: Actually implement IPC message queue or routing mechanism
                    // For now, just log the message
                    info!(
                        "IPC: {} -> {}: {} bytes",
                        sender_module,
                        target_module,
                        msg_bytes.len()
                    );

                    AbiResultCode::Success as i32
                },
            )
            .map_err(|e| AbiError::ExecutionError(e.to_string()))?;

        // host_ipc_receive(buffer_ptr: u32, buffer_len: u32, actual_len_ptr: u32) -> i32
        linker
            .func_wrap(
                "env",
                "host_ipc_receive",
                |mut caller: Caller<'_, AbiContext>,
                 _buffer_ptr: u32,
                 _buffer_len: u32,
                 actual_len_ptr: u32|
                 -> i32 {
                    let module_id = caller.data().module_id.clone();

                    // TODO: Implement actual IPC message queue
                    // For now, return no messages
                    debug!("Module {} checking for IPC messages", module_id);

                    // Write 0 to actual_len_ptr to indicate no messages
                    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                        Some(memory) => memory,
                        None => {
                            error!("Failed to get WASM memory for IPC receive");
                            return AbiResultCode::InvalidOperation as i32;
                        }
                    };

                    let memory_manager = MemoryManager::new(memory);
                    let len_region = MemoryRegion::new(actual_len_ptr, 4);
                    let zero_bytes = 0u32.to_le_bytes();

                    if let Err(e) =
                        memory_manager.write_memory(&mut caller, &len_region, &zero_bytes)
                    {
                        error!("Failed to write message length: {}", e);
                        return AbiResultCode::InvalidArg as i32;
                    }

                    AbiResultCode::Success as i32
                },
            )
            .map_err(|e| AbiError::ExecutionError(e.to_string()))?;

        Ok(())
    }

    /// Add capability checking function
    fn add_capability_functions(linker: &mut Linker<AbiContext>) -> Result<()> {
        // host_capability_check(cap_ptr: u32, cap_len: u32) -> i32
        linker
            .func_wrap(
                "env",
                "host_capability_check",
                |mut caller: Caller<'_, AbiContext>, cap_ptr: u32, cap_len: u32| -> i32 {
                    // Get context references
                    let (module_id, has_cap_manager) = {
                        let context = caller.data();
                        (
                            context.module_id.clone(),
                            context.capability_manager.is_some(),
                        )
                    };

                    if !has_cap_manager {
                        error!("No capability manager available");
                        return AbiResultCode::InvalidOperation as i32;
                    }

                    // Get memory and read capability string
                    let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                        Some(memory) => memory,
                        None => {
                            error!("Failed to get WASM memory for capability check");
                            return AbiResultCode::InvalidOperation as i32;
                        }
                    };

                    let memory_manager = MemoryManager::new(memory);
                    let cap_region = MemoryRegion::new(cap_ptr, cap_len);

                    let cap_bytes = match memory_manager.read_memory(&mut caller, &cap_region) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("Failed to read capability string from WASM memory: {}", e);
                            return AbiResultCode::InvalidArg as i32;
                        }
                    };

                    let cap_str = String::from_utf8_lossy(&cap_bytes);

                    // Parse capability string
                    let cap_manager = caller.data().capability_manager.as_ref().unwrap().clone();

                    let capability =
                        match crate::capabilities::CapabilityType::from_string(&cap_str) {
                            Ok(cap) => cap,
                            Err(e) => {
                                error!("Invalid capability format '{}': {}", cap_str, e);
                                return AbiResultCode::InvalidArg as i32;
                            }
                        };

                    // Check if module has the capability
                    match cap_manager.has_capability(&module_id, &capability) {
                        Ok(true) => {
                            debug!("Module {} has capability: {}", module_id, cap_str);
                            AbiResultCode::Success as i32
                        }
                        Ok(false) => {
                            debug!("Module {} lacks capability: {}", module_id, cap_str);
                            AbiResultCode::PermissionDenied as i32
                        }
                        Err(e) => {
                            error!("Error checking capability: {}", e);
                            AbiResultCode::Error as i32
                        }
                    }
                },
            )
            .map_err(|e| AbiError::ExecutionError(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
#[path = "abi_tests.rs"]
mod tests;

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_memory_region() {
        let region = MemoryRegion::new(100, 50);
        assert!(region.is_valid());
        assert_eq!(region.ptr, 100);
        assert_eq!(region.size, 50);

        let invalid_region = MemoryRegion::new(0, 0);
        assert!(!invalid_region.is_valid());
    }

    #[test]
    fn test_abi_result_codes() {
        assert_eq!(AbiResultCode::Success as i32, 0);
        assert_eq!(AbiResultCode::Error as i32, 1);
        assert_eq!(AbiResultCode::PermissionDenied as i32, 3);
    }

    #[test]
    fn test_error_conversion() {
        let error = AbiError::InvalidPointer { ptr: 100, size: 0 };
        let code: AbiResultCode = error.into();
        assert_eq!(code, AbiResultCode::InvalidArg);
    }

    #[test]
    fn test_abi_context_capabilities() {
        let context = AbiContext::new(
            "test_module".to_string(),
            vec![Capability::ReadState, Capability::Log],
        );

        assert!(context.has_capability(&Capability::ReadState));
        assert!(context.has_capability(&Capability::Log));
        assert!(!context.has_capability(&Capability::WriteState));

        assert!(context.require_capability(&Capability::ReadState).is_ok());
        assert!(context.require_capability(&Capability::WriteState).is_err());
    }

    #[test]
    fn test_stderr_capture() {
        let context = AbiContext::new("test".to_string(), vec![]);

        let test_data = b"test error message";
        context.write_stderr(test_data).unwrap();

        let captured = context.take_stderr().unwrap();
        assert_eq!(captured, test_data);

        // Buffer should be empty after taking
        let empty = context.take_stderr().unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_protobuf_helper() {
        // Test with a simple message struct
        #[derive(prost::Message)]
        struct TestMessage {
            #[prost(string, tag = "1")]
            content: String,
            #[prost(int32, tag = "2")]
            value: i32,
        }

        let msg = TestMessage {
            content: "test".to_string(),
            value: 42,
        };

        let serialized = ProtobufHelper::serialize(&msg).unwrap();
        let deserialized: TestMessage = ProtobufHelper::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.content, "test");
        assert_eq!(deserialized.value, 42);
    }
}
