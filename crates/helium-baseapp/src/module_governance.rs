//! Module Lifecycle Management
//!
//! This module implements governance-based module installation and upgrade functionality.
//! It provides handlers for storing WASM bytecode, installing modules, and upgrading modules
//! through governance proposals.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use helium_types::{AccAddress, SdkError, SdkMsg};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error, info, warn};

use crate::module_router::{ModuleConfig, ModuleRouter, RouterError};
use crate::vfs::VirtualFilesystem;

/// Module governance error types
#[derive(Error, Debug)]
pub enum GovernanceError {
    /// Invalid WASM bytecode
    #[error("invalid WASM bytecode: {0}")]
    InvalidWasm(String),

    /// Module not found
    #[error("module not found: {0}")]
    ModuleNotFound(String),

    /// Module already exists
    #[error("module already exists: {0}")]
    ModuleAlreadyExists(String),

    /// Invalid module configuration
    #[error("invalid module configuration: {0}")]
    InvalidConfig(String),

    /// Migration failed
    #[error("module migration failed: {0}")]
    MigrationFailed(String),

    /// Storage error
    #[error("storage error: {0}")]
    StorageError(String),

    /// Router error
    #[error("router error: {0}")]
    RouterError(#[from] RouterError),

    /// Unauthorized operation
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// Version compatibility error
    #[error("version incompatible: {0}")]
    IncompatibleVersion(String),
}

pub type Result<T> = std::result::Result<T, GovernanceError>;

/// Message to store WASM bytecode on-chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsgStoreCode {
    /// Proposal creator/authority (usually governance module)
    pub authority: String,
    /// WASM bytecode to store
    pub wasm_code: Vec<u8>,
    /// Code metadata
    pub metadata: CodeMetadata,
}

impl SdkMsg for MsgStoreCode {
    fn type_url(&self) -> &'static str {
        "/helium.baseapp.v1.MsgStoreCode"
    }

    fn validate_basic(&self) -> std::result::Result<(), SdkError> {
        if self.authority.is_empty() {
            return Err(SdkError::InvalidRequest(
                "authority cannot be empty".to_string(),
            ));
        }
        if self.wasm_code.is_empty() {
            return Err(SdkError::InvalidRequest(
                "wasm_code cannot be empty".to_string(),
            ));
        }
        if self.metadata.name.is_empty() {
            return Err(SdkError::InvalidRequest(
                "metadata name cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    fn get_signers(&self) -> std::result::Result<Vec<AccAddress>, SdkError> {
        // Parse authority as AccAddress
        let (_hrp, addr) = AccAddress::from_bech32(&self.authority)
            .map_err(|_| SdkError::InvalidAddress(self.authority.clone()))?;
        Ok(vec![addr])
    }

    fn encode(&self) -> Vec<u8> {
        // In a real implementation, this would use protobuf encoding
        serde_json::to_vec(self).unwrap_or_default()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Message to install a new module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsgInstallModule {
    /// Proposal creator/authority
    pub authority: String,
    /// Code ID of stored WASM bytecode
    pub code_id: u64,
    /// Module configuration
    pub config: ModuleInstallConfig,
    /// Initial migration data (if needed)
    pub init_data: Option<Vec<u8>>,
}

impl SdkMsg for MsgInstallModule {
    fn type_url(&self) -> &'static str {
        "/helium.baseapp.v1.MsgInstallModule"
    }

    fn validate_basic(&self) -> std::result::Result<(), SdkError> {
        if self.authority.is_empty() {
            return Err(SdkError::InvalidRequest(
                "authority cannot be empty".to_string(),
            ));
        }
        if self.code_id == 0 {
            return Err(SdkError::InvalidRequest(
                "code_id must be greater than 0".to_string(),
            ));
        }
        if self.config.name.is_empty() {
            return Err(SdkError::InvalidRequest(
                "module name cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    fn get_signers(&self) -> std::result::Result<Vec<AccAddress>, SdkError> {
        let (_hrp, addr) = AccAddress::from_bech32(&self.authority)
            .map_err(|_| SdkError::InvalidAddress(self.authority.clone()))?;
        Ok(vec![addr])
    }

    fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Message to upgrade an existing module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsgUpgradeModule {
    /// Proposal creator/authority
    pub authority: String,
    /// Module name to upgrade
    pub module_name: String,
    /// New code ID for the upgrade
    pub new_code_id: u64,
    /// Migration data for the upgrade
    pub migration_data: Option<Vec<u8>>,
    /// Whether to force upgrade (skip compatibility checks)
    pub force: bool,
}

impl SdkMsg for MsgUpgradeModule {
    fn type_url(&self) -> &'static str {
        "/helium.baseapp.v1.MsgUpgradeModule"
    }

    fn validate_basic(&self) -> std::result::Result<(), SdkError> {
        if self.authority.is_empty() {
            return Err(SdkError::InvalidRequest(
                "authority cannot be empty".to_string(),
            ));
        }
        if self.module_name.is_empty() {
            return Err(SdkError::InvalidRequest(
                "module_name cannot be empty".to_string(),
            ));
        }
        if self.new_code_id == 0 {
            return Err(SdkError::InvalidRequest(
                "new_code_id must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }

    fn get_signers(&self) -> std::result::Result<Vec<AccAddress>, SdkError> {
        let (_hrp, addr) = AccAddress::from_bech32(&self.authority)
            .map_err(|_| SdkError::InvalidAddress(self.authority.clone()))?;
        Ok(vec![addr])
    }

    fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// WASM code metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeMetadata {
    /// Human-readable name
    pub name: String,
    /// Version string
    pub version: String,
    /// Description
    pub description: String,
    /// Repository URL
    pub repository: Option<String>,
    /// License
    pub license: Option<String>,
    /// Supported module API version
    pub api_version: String,
    /// Checksum of the WASM code
    pub checksum: String,
}

/// Module installation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInstallConfig {
    /// Module name/identifier
    pub name: String,
    /// Message types this module will handle
    pub message_types: Vec<String>,
    /// Module dependencies
    pub dependencies: Vec<String>,
    /// VFS capabilities required
    pub capabilities: Vec<String>,
    /// Gas limit for execution
    pub gas_limit: u64,
    /// Memory limit
    pub memory_limit: u64,
    /// Whether module exports handlers
    pub exports_handlers: bool,
    /// IPC endpoints provided
    pub ipc_endpoints: Vec<String>,
}

/// On-chain code registry entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCode {
    /// Unique code ID
    pub code_id: u64,
    /// WASM bytecode
    pub code: Vec<u8>,
    /// Code metadata
    pub metadata: CodeMetadata,
    /// Timestamp when stored
    pub created_at: u64,
    /// Creator authority
    pub creator: String,
}

/// On-chain module registry entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledModule {
    /// Module name
    pub name: String,
    /// Current code ID
    pub code_id: u64,
    /// Installation configuration
    pub config: ModuleInstallConfig,
    /// Installation timestamp
    pub installed_at: u64,
    /// Last upgrade timestamp
    pub upgraded_at: Option<u64>,
    /// Installation authority
    pub authority: String,
    /// Current version
    pub version: String,
}

/// Module Governance Handler
///
/// Manages the lifecycle of WASM modules through governance proposals.
pub struct ModuleGovernance {
    /// Module router for managing modules
    router: Arc<ModuleRouter>,
    /// Virtual filesystem for storage
    #[allow(dead_code)]
    vfs: Arc<VirtualFilesystem>,
    /// On-chain code registry
    code_registry: Arc<Mutex<HashMap<u64, StoredCode>>>,
    /// On-chain module registry
    module_registry: Arc<Mutex<HashMap<String, InstalledModule>>>,
    /// Next available code ID
    next_code_id: Arc<Mutex<u64>>,
    /// Governance authority (usually the governance module address)
    governance_authority: String,
}

impl ModuleGovernance {
    /// Create a new module governance handler
    pub fn new(
        router: Arc<ModuleRouter>,
        vfs: Arc<VirtualFilesystem>,
        governance_authority: String,
    ) -> Self {
        Self {
            router,
            vfs,
            code_registry: Arc::new(Mutex::new(HashMap::new())),
            module_registry: Arc::new(Mutex::new(HashMap::new())),
            next_code_id: Arc::new(Mutex::new(1)),
            governance_authority,
        }
    }

    /// Handle MsgStoreCode - store WASM bytecode on-chain
    pub fn handle_store_code(&self, msg: MsgStoreCode) -> Result<u64> {
        debug!("Handling store code request for: {}", msg.metadata.name);

        // Verify authority
        if msg.authority != self.governance_authority {
            return Err(GovernanceError::Unauthorized(format!(
                "only governance authority can store code, got: {}",
                msg.authority
            )));
        }

        // Validate WASM bytecode
        self.validate_wasm_code(&msg.wasm_code)?;

        // Calculate checksum
        let checksum = self.calculate_checksum(&msg.wasm_code);
        if checksum != msg.metadata.checksum {
            return Err(GovernanceError::InvalidWasm(
                "checksum mismatch".to_string(),
            ));
        }

        // Get next code ID
        let code_id = {
            let mut next_id = self
                .next_code_id
                .lock()
                .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;
            let id = *next_id;
            *next_id += 1;
            id
        };

        // Store the code
        let stored_code = StoredCode {
            code_id,
            code: msg.wasm_code,
            metadata: msg.metadata,
            created_at: self.current_timestamp(),
            creator: msg.authority,
        };

        {
            let mut registry = self
                .code_registry
                .lock()
                .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;
            registry.insert(code_id, stored_code);
        }

        // Persist to VFS
        self.persist_code_registry()?;

        info!("Stored WASM code with ID: {}", code_id);
        Ok(code_id)
    }

    /// Handle MsgInstallModule - install a new module
    pub fn handle_install_module(&self, msg: MsgInstallModule) -> Result<()> {
        debug!("Handling install module request: {}", msg.config.name);

        // Verify authority
        if msg.authority != self.governance_authority {
            return Err(GovernanceError::Unauthorized(format!(
                "only governance authority can install modules, got: {}",
                msg.authority
            )));
        }

        // Check if module already exists
        {
            let registry = self
                .module_registry
                .lock()
                .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;
            if registry.contains_key(&msg.config.name) {
                return Err(GovernanceError::ModuleAlreadyExists(msg.config.name));
            }
        }

        // Verify code exists
        let stored_code = {
            let registry = self
                .code_registry
                .lock()
                .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;
            registry
                .get(&msg.code_id)
                .ok_or_else(|| {
                    GovernanceError::InvalidConfig(format!("code ID {} not found", msg.code_id))
                })?
                .clone()
        };

        // Create module configuration
        let module_config = self.create_module_config(&msg.config, &stored_code)?;

        // Register with module router
        self.router.register_module(module_config)?;

        // Load the module
        self.load_module_in_router(&msg.config.name)?;

        // Run initialization if provided
        if let Some(init_data) = msg.init_data {
            self.run_module_init(&msg.config.name, &init_data)?;
        }

        // Register in module registry
        let installed_module = InstalledModule {
            name: msg.config.name.clone(),
            code_id: msg.code_id,
            config: msg.config,
            installed_at: self.current_timestamp(),
            upgraded_at: None,
            authority: msg.authority,
            version: stored_code.metadata.version,
        };

        let module_name = installed_module.name.clone();
        {
            let mut registry = self
                .module_registry
                .lock()
                .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;
            registry.insert(installed_module.name.clone(), installed_module);
        }

        // Persist to VFS
        self.persist_module_registry()?;

        info!("Successfully installed module: {}", module_name);
        Ok(())
    }

    /// Handle MsgUpgradeModule - upgrade an existing module
    pub fn handle_upgrade_module(&self, msg: MsgUpgradeModule) -> Result<()> {
        debug!("Handling upgrade module request: {}", msg.module_name);

        // Verify authority
        if msg.authority != self.governance_authority {
            return Err(GovernanceError::Unauthorized(format!(
                "only governance authority can upgrade modules, got: {}",
                msg.authority
            )));
        }

        // Get current module
        let current_module = {
            let registry = self
                .module_registry
                .lock()
                .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;
            registry
                .get(&msg.module_name)
                .ok_or_else(|| GovernanceError::ModuleNotFound(msg.module_name.clone()))?
                .clone()
        };

        // Get new code
        let new_code = {
            let registry = self
                .code_registry
                .lock()
                .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;
            registry
                .get(&msg.new_code_id)
                .ok_or_else(|| {
                    GovernanceError::InvalidConfig(format!("code ID {} not found", msg.new_code_id))
                })?
                .clone()
        };

        // Check version compatibility unless forced
        if !msg.force {
            self.check_version_compatibility(&current_module, &new_code)?;
        }

        // Run pre-upgrade migration
        if let Some(migration_data) = msg.migration_data {
            self.run_pre_upgrade_migration(&msg.module_name, &migration_data)?;
        }

        // Unload current module
        self.unload_module_from_router(&msg.module_name)?;

        // Create new module configuration
        let new_config = self.create_module_config(&current_module.config, &new_code)?;

        // Register new module
        self.router.register_module(new_config)?;

        // Load new module
        self.load_module_in_router(&msg.module_name)?;

        // Run post-upgrade migration
        self.run_post_upgrade_migration(&msg.module_name)?;

        // Update module registry
        {
            let mut registry = self
                .module_registry
                .lock()
                .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;

            if let Some(module) = registry.get_mut(&msg.module_name) {
                module.code_id = msg.new_code_id;
                module.upgraded_at = Some(self.current_timestamp());
                module.version = new_code.metadata.version;
            }
        }

        // Persist to VFS
        self.persist_module_registry()?;

        info!("Successfully upgraded module: {}", msg.module_name);
        Ok(())
    }

    /// Validate WASM bytecode
    fn validate_wasm_code(&self, code: &[u8]) -> Result<()> {
        // Basic WASM magic number check
        if code.len() < 8 {
            return Err(GovernanceError::InvalidWasm("code too short".to_string()));
        }

        let magic = &code[0..4];
        if magic != b"\x00asm" {
            return Err(GovernanceError::InvalidWasm(
                "invalid WASM magic number".to_string(),
            ));
        }

        let version = u32::from_le_bytes([code[4], code[5], code[6], code[7]]);
        if version != 1 {
            return Err(GovernanceError::InvalidWasm(format!(
                "unsupported WASM version: {version}"
            )));
        }

        // TODO: Add more comprehensive WASM validation using wasmparser
        // - Check for required exports (e.g., _start, memory)
        // - Validate imports are within allowed set
        // - Check memory/table limits
        // - Validate instruction set is safe

        Ok(())
    }

    /// Calculate SHA256 checksum of WASM code
    fn calculate_checksum(&self, code: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(code);
        hex::encode(hash)
    }

    /// Get current timestamp
    fn current_timestamp(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Create ModuleConfig from install config and stored code
    fn create_module_config(
        &self,
        config: &ModuleInstallConfig,
        stored_code: &StoredCode,
    ) -> Result<ModuleConfig> {
        // Write WASM code to temporary file
        let temp_path = self.write_temp_wasm(&stored_code.code, &config.name)?;

        let mut module_config = ModuleConfig::new(config.name.clone(), temp_path)
            .with_gas_limit(config.gas_limit)
            .with_memory_limit(config.memory_limit);

        if config.exports_handlers {
            module_config = module_config.exports_handlers();
        }

        for msg_type in &config.message_types {
            module_config = module_config.handles_message_type(msg_type.clone());
        }

        for dep in &config.dependencies {
            module_config = module_config.depends_on(dep.clone());
        }

        for cap in &config.capabilities {
            module_config = module_config.requires_capability(cap.clone());
        }

        for endpoint in &config.ipc_endpoints {
            module_config = module_config.provides_endpoint(endpoint.clone());
        }

        Ok(module_config)
    }

    /// Write WASM code to temporary file
    fn write_temp_wasm(&self, code: &[u8], module_name: &str) -> Result<PathBuf> {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("{module_name}.wasm"));

        std::fs::write(&temp_path, code).map_err(|e| {
            GovernanceError::StorageError(format!("Failed to write temp WASM: {e}"))
        })?;

        Ok(temp_path)
    }

    /// Run module initialization
    fn run_module_init(&self, module_name: &str, _init_data: &[u8]) -> Result<()> {
        // TODO: Execute module init function with init_data
        debug!("Running init for module: {}", module_name);
        Ok(())
    }

    /// Check version compatibility between current and new module
    fn check_version_compatibility(
        &self,
        current: &InstalledModule,
        new_code: &StoredCode,
    ) -> Result<()> {
        // Simple semantic version check - in production this would be more sophisticated
        let current_version = &current.version;
        let new_version = &new_code.metadata.version;

        if new_version <= current_version {
            return Err(GovernanceError::IncompatibleVersion(format!(
                "new version {new_version} is not newer than current version {current_version}"
            )));
        }

        // TODO: Add more sophisticated compatibility checks
        // - API version compatibility
        // - Breaking change detection
        // - Migration requirements

        Ok(())
    }

    /// Run pre-upgrade migration
    fn run_pre_upgrade_migration(&self, module_name: &str, _migration_data: &[u8]) -> Result<()> {
        // TODO: Execute module's pre-upgrade migration function
        debug!("Running pre-upgrade migration for module: {}", module_name);
        Ok(())
    }

    /// Run post-upgrade migration
    fn run_post_upgrade_migration(&self, module_name: &str) -> Result<()> {
        // TODO: Execute module's post-upgrade migration function
        debug!("Running post-upgrade migration for module: {}", module_name);
        Ok(())
    }

    /// Persist code registry to VFS
    fn persist_code_registry(&self) -> Result<()> {
        let registry = self
            .code_registry
            .lock()
            .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;

        let _serialized = serde_json::to_vec(&*registry)
            .map_err(|e| GovernanceError::StorageError(format!("Serialization failed: {e}")))?;

        // TODO: Write to VFS at /system/code_registry
        debug!("Persisting code registry with {} entries", registry.len());
        Ok(())
    }

    /// Persist module registry to VFS
    fn persist_module_registry(&self) -> Result<()> {
        let registry = self
            .module_registry
            .lock()
            .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;

        let _serialized = serde_json::to_vec(&*registry)
            .map_err(|e| GovernanceError::StorageError(format!("Serialization failed: {e}")))?;

        // TODO: Write to VFS at /system/module_registry
        debug!("Persisting module registry with {} entries", registry.len());
        Ok(())
    }

    /// Get installed module info
    pub fn get_module(&self, name: &str) -> Result<Option<InstalledModule>> {
        let registry = self
            .module_registry
            .lock()
            .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;
        Ok(registry.get(name).cloned())
    }

    /// Get stored code info
    pub fn get_code(&self, code_id: u64) -> Result<Option<StoredCode>> {
        let registry = self
            .code_registry
            .lock()
            .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;
        Ok(registry.get(&code_id).cloned())
    }

    /// List all installed modules
    pub fn list_modules(&self) -> Result<Vec<InstalledModule>> {
        let registry = self
            .module_registry
            .lock()
            .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;
        Ok(registry.values().cloned().collect())
    }

    /// List all stored codes
    pub fn list_codes(&self) -> Result<Vec<StoredCode>> {
        let registry = self
            .code_registry
            .lock()
            .map_err(|e| GovernanceError::StorageError(format!("Lock poisoned: {e}")))?;
        Ok(registry.values().cloned().collect())
    }

    /// Helper method to unload a module from the router
    fn unload_module_from_router(&self, _module_name: &str) -> Result<()> {
        // For now, we'll rely on the router's existing capability to unregister modules
        // In a real implementation, we'd need to add unload functionality to ModuleRouter
        warn!("Module unloading not fully implemented - registry cleanup only");
        Ok(())
    }

    /// Helper method to load a module in the router
    fn load_module_in_router(&self, _module_name: &str) -> Result<()> {
        // Use the router's internal load_module method through a workaround
        // Since we can't access private methods, we'll trigger initialization
        self.router
            .initialize()
            .map_err(GovernanceError::RouterError)?;
        Ok(())
    }
}

// Extension methods for ModuleRouter to support governance operations

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasi_host::WasiHost;
    use tempfile::TempDir;

    fn create_test_governance() -> (ModuleGovernance, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let wasi_host = Arc::new(WasiHost::new().unwrap());
        let vfs = Arc::new(VirtualFilesystem::new());
        let router = Arc::new(ModuleRouter::new(wasi_host, vfs.clone()));

        let governance = ModuleGovernance::new(router, vfs, "governance_authority".to_string());

        (governance, temp_dir)
    }

    #[test]
    fn test_store_code_validation() {
        let (governance, _temp_dir) = create_test_governance();

        // Test valid WASM
        let mut valid_code = vec![0x00, 0x61, 0x73, 0x6d]; // WASM magic number
        valid_code.extend_from_slice(&1u32.to_le_bytes()); // WASM version 1
        valid_code.resize(100, 0); // Add some dummy data

        let checksum = governance.calculate_checksum(&valid_code);

        let msg = MsgStoreCode {
            authority: "governance_authority".to_string(),
            wasm_code: valid_code,
            metadata: CodeMetadata {
                name: "test_module".to_string(),
                version: "1.0.0".to_string(),
                description: "Test module".to_string(),
                repository: None,
                license: None,
                api_version: "1.0".to_string(),
                checksum,
            },
        };

        let result = governance.handle_store_code(msg);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
    }

    #[test]
    fn test_unauthorized_store_code() {
        let (governance, _temp_dir) = create_test_governance();

        let msg = MsgStoreCode {
            authority: "unauthorized".to_string(),
            wasm_code: vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00],
            metadata: CodeMetadata {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: "Test".to_string(),
                repository: None,
                license: None,
                api_version: "1.0".to_string(),
                checksum: "invalid".to_string(),
            },
        };

        let result = governance.handle_store_code(msg);
        assert!(matches!(result, Err(GovernanceError::Unauthorized(_))));
    }

    #[test]
    fn test_invalid_wasm_code() {
        let (governance, _temp_dir) = create_test_governance();

        let msg = MsgStoreCode {
            authority: "governance_authority".to_string(),
            wasm_code: vec![0x12, 0x34, 0x56, 0x78], // Invalid magic number
            metadata: CodeMetadata {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: "Test".to_string(),
                repository: None,
                license: None,
                api_version: "1.0".to_string(),
                checksum: "invalid".to_string(),
            },
        };

        let result = governance.handle_store_code(msg);
        assert!(matches!(result, Err(GovernanceError::InvalidWasm(_))));
    }
}
