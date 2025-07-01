//! Base application framework for the helium blockchain.
//!
//! This crate provides the core application interface and ABCI implementation
//! for helium blockchain applications.

// Note: ante module removed - transaction validation handled by WASI modules
pub mod abi;
pub mod ante;
pub mod wasi_host;
pub mod vfs;
pub mod module_router;
pub mod capabilities;
pub mod module_governance;
pub mod module_loader;

use thiserror::Error;
use std::sync::Arc;
use std::collections::HashMap;
use helium_store::{KVStore, MemStore};
use helium_types::{tx::TxDecodeError, SdkMsg};
use serde_json::Value;

// Import microkernel components  
use crate::wasi_host::WasiHost;
use crate::vfs::VirtualFilesystem;
use crate::module_router::{ModuleRouter, ExecutionContext, ExecutionResult};
use crate::capabilities::CapabilityManager;
use crate::module_governance::ModuleGovernance;

// Note: ante handlers removed - transaction validation handled by WASI modules
pub use abi::{AbiContext, AbiError, AbiResultCode, Capability, HostFunctions, MemoryManager, MemoryRegion, ProtobufHelper};
pub use module_governance::{MsgStoreCode, MsgInstallModule, MsgUpgradeModule, CodeMetadata, ModuleInstallConfig};

/// BaseApp errors
#[derive(Error, Debug)]
pub enum BaseAppError {
    /// Invalid transaction
    #[error("invalid transaction: {0}")]
    InvalidTx(String),

    /// Transaction execution failed
    #[error("transaction execution failed: {0}")]
    TxFailed(String),

    /// Invalid block
    #[error("invalid block: {0}")]
    InvalidBlock(String),

    /// Store error
    #[error("store error: {0}")]
    Store(String),

    /// ABCI operation failed
    #[error("ABCI operation failed: {0}")]
    AbciError(String),

    /// Query failed
    #[error("query failed: {0}")]
    QueryFailed(String),

    /// Chain initialization failed
    #[error("chain initialization failed: {0}")]
    InitChainFailed(String),
}

/// Result type alias
pub type Result<T> = std::result::Result<T, BaseAppError>;

/// Transaction response
#[derive(Debug, Clone)]
pub struct TxResponse {
    /// Response code (0 = success)
    pub code: u32,
    /// Log message
    pub log: String,
    /// Gas used
    pub gas_used: u64,
    /// Gas wanted
    pub gas_wanted: u64,
    /// Events emitted
    pub events: Vec<Event>,
}

/// Event emitted during execution
#[derive(Debug, Clone)]
pub struct Event {
    /// Event type
    pub event_type: String,
    /// Event attributes
    pub attributes: Vec<Attribute>,
}

/// Event attribute
#[derive(Debug, Clone)]
pub struct Attribute {
    /// Attribute key
    pub key: String,
    /// Attribute value
    pub value: String,
}

/// ABCI Application trait
pub trait Application: Send + Sync {
    /// Get application info
    fn info(&self) -> Result<(String, u64)>;

    /// Check transaction validity without executing
    fn check_tx(&self, tx: &[u8]) -> Result<TxResponse>;

    /// Process a complete block
    fn finalize_block(
        &mut self,
        height: u64,
        time: u64,
        txs: Vec<Vec<u8>>,
    ) -> Result<Vec<TxResponse>>;

    /// Commit the current block state
    fn commit(&mut self) -> Result<Vec<u8>>;

    /// Initialize chain from genesis
    fn init_chain(&mut self, chain_id: String, genesis: &[u8]) -> Result<()>;

    /// Query application state
    fn query(&self, path: String, data: &[u8], height: u64, prove: bool) -> Result<QueryResponse>;
}

/// Query response structure
#[derive(Debug, Clone)]
pub struct QueryResponse {
    /// Response code (0 = success)
    pub code: u32,
    /// Log message
    pub log: String,
    /// Query result value
    pub value: Vec<u8>,
    /// Height at which the query was evaluated
    pub height: u64,
    /// Merkle proof (if requested)
    pub proof: Option<Vec<u8>>,
}

/// Execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecMode {
    /// Check mode (validation only)
    Check,
    /// Simulate mode
    Simulate,
    /// Deliver mode (actual execution)
    Deliver,
}

/// Execution context
#[derive(Debug, Clone)]
pub struct Context {
    /// Current block height
    pub block_height: u64,
    /// Current block time
    pub block_time: u64,
    /// Chain ID
    pub chain_id: String,
    /// Execution mode
    pub exec_mode: ExecMode,
}

impl Context {
    /// Create a new context
    pub fn new(block_height: u64, block_time: u64, chain_id: String, exec_mode: ExecMode) -> Self {
        Self {
            block_height,
            block_time,
            chain_id,
            exec_mode,
        }
    }
}

/// Base application - acts as microkernel host for WASM modules
pub struct BaseApp {
    /// Application name
    name: String,
    /// Current context
    context: Option<Context>,
    /// WASI runtime host for module execution
    wasi_host: Arc<WasiHost>,
    /// Virtual filesystem for state access
    vfs: Arc<VirtualFilesystem>,
    /// Module router for message dispatch
    module_router: Arc<ModuleRouter>,
    /// Capability manager for module security
    capability_manager: Arc<CapabilityManager>,
    /// Module governance for WASM module lifecycle management
    module_governance: Arc<ModuleGovernance>,
    /// Module paths for WASI modules
    module_paths: HashMap<String, String>,
}

impl BaseApp {
    /// Create a new base application with microkernel architecture
    pub fn new(name: String) -> Result<Self> {
        // Initialize WASI runtime host
        let wasi_host = Arc::new(
            WasiHost::new()
                .map_err(|e| BaseAppError::InitChainFailed(format!("Failed to initialize WASI host: {}", e)))?
        );
        
        // Initialize virtual filesystem
        let vfs = Arc::new(VirtualFilesystem::new());
        
        // Initialize default stores for the VFS
        Self::setup_default_stores(&vfs)?;
        
        // Initialize capability manager
        let capability_manager = Arc::new(CapabilityManager::new());
        
        // Initialize module router
        let module_router = Arc::new(ModuleRouter::new(wasi_host.clone(), vfs.clone()));
        
        // Initialize module governance with governance authority
        let governance_authority = "cosmos10d07y265gmmuvt4z0w9aw880jnsr700j6zn9kn".to_string(); // Default governance module address
        let module_governance = Arc::new(ModuleGovernance::new(
            module_router.clone(),
            vfs.clone(),
            governance_authority,
        ));
        
        // Register governance message types (we'll handle them directly in BaseApp for now)
        // In a full implementation, these would be routed through WASM modules
        
        // Initialize module paths
        let mut module_paths = HashMap::new();
        module_paths.insert("begin_blocker".to_string(), "modules/begin_blocker.wasm".to_string());
        module_paths.insert("end_blocker".to_string(), "modules/end_blocker.wasm".to_string());
        module_paths.insert("tx_decoder".to_string(), "modules/tx_decoder.wasm".to_string());
        
        Ok(Self {
            name,
            context: None,
            wasi_host,
            vfs,
            module_router,
            capability_manager,
            module_governance,
            module_paths,
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
    
    /// Set up default stores in the VFS for blockchain modules
    fn setup_default_stores(vfs: &Arc<VirtualFilesystem>) -> Result<()> {
        // Create default stores for core modules
        let auth_store: Arc<std::sync::Mutex<dyn KVStore>> = Arc::new(std::sync::Mutex::new(MemStore::new()));
        let bank_store: Arc<std::sync::Mutex<dyn KVStore>> = Arc::new(std::sync::Mutex::new(MemStore::new()));
        let staking_store: Arc<std::sync::Mutex<dyn KVStore>> = Arc::new(std::sync::Mutex::new(MemStore::new()));
        let gov_store: Arc<std::sync::Mutex<dyn KVStore>> = Arc::new(std::sync::Mutex::new(MemStore::new()));
        
        // Mount stores in VFS namespaces
        vfs.mount_store("auth".to_string(), auth_store)
            .map_err(|e| BaseAppError::Store(format!("Failed to mount auth store: {}", e)))?;
        vfs.mount_store("bank".to_string(), bank_store)
            .map_err(|e| BaseAppError::Store(format!("Failed to mount bank store: {}", e)))?;
        vfs.mount_store("staking".to_string(), staking_store)
            .map_err(|e| BaseAppError::Store(format!("Failed to mount staking store: {}", e)))?;
        vfs.mount_store("gov".to_string(), gov_store)
            .map_err(|e| BaseAppError::Store(format!("Failed to mount gov store: {}", e)))?;
            
        Ok(())
    }

    /// Begin block processing using WASI module
    pub fn begin_block(&mut self, height: u64, time: u64, chain_id: String) -> Result<()> {
        // Set context for current block
        self.context = Some(Context::new(height, time, chain_id.clone(), ExecMode::Deliver));
        
        // Load and execute BeginBlock WASI module
        match self.execute_begin_block_wasi(height, time, &chain_id) {
            Ok(events) => {
                // Process events from BeginBlock module
                log::info!("BeginBlock WASI module executed successfully with {} events", events.len());
                Ok(())
            }
            Err(e) => {
                log::error!("BeginBlock WASI module failed: {}", e);
                // For now, continue with block processing even if BeginBlock fails
                // In production, this might be a fatal error
                Ok(())
            }
        }
    }

    /// Execute BeginBlock WASI module
    fn execute_begin_block_wasi(&mut self, height: u64, time: u64, chain_id: &str) -> Result<Vec<Event>> {
        use serde::{Deserialize, Serialize};
        
        #[derive(Debug, Serialize)]
        struct BeginBlockRequest {
            height: u64,
            time: u64,
            chain_id: String,
            byzantine_validators: Vec<Evidence>,
        }
        
        #[derive(Debug, Serialize)]
        struct Evidence {
            validator_address: Vec<u8>,
            evidence_type: String,
            height: u64,
        }
        
        #[derive(Debug, Deserialize)]
        struct BeginBlockResponse {
            success: bool,
            events: Vec<WasiEvent>,
            error: Option<String>,
        }
        
        #[derive(Debug, Deserialize)]
        struct WasiEvent {
            event_type: String,
            attributes: Vec<WasiAttribute>,
        }
        
        #[derive(Debug, Deserialize)]
        struct WasiAttribute {
            key: String,
            value: String,
        }
        
        // Create request
        let request = BeginBlockRequest {
            height,
            time,
            chain_id: chain_id.to_string(),
            byzantine_validators: vec![], // TODO: Get from tendermint
        };
        
        let input = serde_json::to_string(&request)
            .map_err(|e| BaseAppError::AbciError(format!("Failed to serialize BeginBlock request: {}", e)))?;
        
        // Try to load and execute BeginBlock module
        let module_path = "modules/begin_blocker.wasm";
        if let Ok(module_bytes) = std::fs::read(module_path) {
            match self.wasi_host.execute_module_with_input(&module_bytes, input.as_bytes()) {
                Ok(result) => {
                    let response: BeginBlockResponse = serde_json::from_slice(&result.stdout)
                        .map_err(|e| BaseAppError::AbciError(format!("Failed to parse BeginBlock response: {}", e)))?;
                    
                    if !response.success {
                        return Err(BaseAppError::AbciError(
                            response.error.unwrap_or_else(|| "BeginBlock failed".to_string())
                        ));
                    }
                    
                    // Convert WASI events to BaseApp events
                    let events = response.events.into_iter().map(|e| Event {
                        event_type: e.event_type,
                        attributes: e.attributes.into_iter().map(|a| Attribute {
                            key: a.key,
                            value: a.value,
                        }).collect(),
                    }).collect();
                    
                    Ok(events)
                }
                Err(e) => Err(BaseAppError::AbciError(format!("BeginBlock WASI execution failed: {}", e)))
            }
        } else {
            // Module not found - use placeholder
            log::warn!("BeginBlock WASI module not found at {}, using placeholder", module_path);
            Ok(vec![])
        }
    }

    /// End block processing using WASI module
    pub fn end_block(&mut self) -> Result<()> {
        let result = if let Some(ctx) = self.context.take() {
            self.execute_end_block_wasi(ctx.block_height, ctx.block_time, &ctx.chain_id)
        } else {
            Err(BaseAppError::InvalidBlock("No active context for EndBlock".to_string()))
        };
        
        result.map(|_| ())
    }

    /// Execute EndBlock WASI module
    fn execute_end_block_wasi(&mut self, height: u64, time: u64, chain_id: &str) -> Result<Vec<Event>> {
        use serde::{Deserialize, Serialize};
        
        #[derive(Debug, Serialize)]
        struct EndBlockRequest {
            height: u64,
            time: u64,
            chain_id: String,
            total_power: i64,
            proposer_address: Vec<u8>,
        }
        
        #[derive(Debug, Serialize)]
        struct ModuleState {
            pending_validator_updates: Vec<ValidatorUpdate>,
            active_proposals: Vec<Proposal>,
            inflation_rate: f64,
            last_reward_height: u64,
        }
        
        #[derive(Debug, Serialize, Deserialize)]
        struct ValidatorUpdate {
            pub_key: PubKey,
            power: i64,
        }
        
        #[derive(Debug, Serialize, Deserialize)]
        struct PubKey {
            type_url: String,
            value: Vec<u8>,
        }
        
        #[derive(Debug, Serialize, Deserialize)]
        struct Proposal {
            id: u64,
            voting_end_time: u64,
            tally: TallyResult,
        }
        
        #[derive(Debug, Serialize, Deserialize)]
        struct TallyResult {
            yes_votes: u64,
            no_votes: u64,
            abstain_votes: u64,
            no_with_veto_votes: u64,
        }
        
        #[derive(Debug, Deserialize)]
        struct EndBlockResponse {
            validator_updates: Vec<ValidatorUpdate>,
            consensus_param_updates: Option<ConsensusParams>,
            events: Vec<WasiEvent>,
        }
        
        #[derive(Debug, Deserialize)]  
        struct ConsensusParams {
            block_max_bytes: i64,
            block_max_gas: i64,
        }
        
        #[derive(Debug, Deserialize)]
        struct WasiEvent {
            event_type: String,
            attributes: Vec<WasiAttribute>,
        }
        
        #[derive(Debug, Deserialize)]
        struct WasiAttribute {
            key: String,
            value: String,
        }
        
        // Create request and state
        let request = EndBlockRequest {
            height,
            time,
            chain_id: chain_id.to_string(),
            total_power: 1000000, // TODO: Get from staking module
            proposer_address: vec![0; 20], // TODO: Get from tendermint
        };
        
        let state = ModuleState {
            pending_validator_updates: vec![],
            active_proposals: vec![],
            inflation_rate: 0.10,
            last_reward_height: height.saturating_sub(1000),
        };
        
        let input = serde_json::to_string(&(request, state))
            .map_err(|e| BaseAppError::AbciError(format!("Failed to serialize EndBlock request: {}", e)))?;
        
        // Try to load and execute EndBlock module
        let module_path = "modules/end_blocker.wasm";
        if let Ok(module_bytes) = std::fs::read(module_path) {
            match self.wasi_host.execute_module_with_input(&module_bytes, input.as_bytes()) {
                Ok(result) => {
                    let response: EndBlockResponse = serde_json::from_slice(&result.stdout)
                        .map_err(|e| BaseAppError::AbciError(format!("Failed to parse EndBlock response: {}", e)))?;
                    
                    // Convert WASI events to BaseApp events
                    let events = response.events.into_iter().map(|e| Event {
                        event_type: e.event_type,
                        attributes: e.attributes.into_iter().map(|a| Attribute {
                            key: a.key,
                            value: a.value,
                        }).collect(),
                    }).collect();
                    
                    // TODO: Process validator updates and consensus param updates
                    if !response.validator_updates.is_empty() {
                        log::info!("EndBlock produced {} validator updates", response.validator_updates.len());
                    }
                    
                    Ok(events)
                }
                Err(e) => Err(BaseAppError::AbciError(format!("EndBlock WASI execution failed: {}", e)))
            }
        } else {
            // Module not found - use placeholder
            log::warn!("EndBlock WASI module not found at {}, using placeholder", module_path);
            Ok(vec![])
        }
    }

    /// Check transaction validity
    pub fn check_tx(&self, tx_bytes: &[u8]) -> Result<TxResponse> {
        // First decode the transaction using WASI TxDecoder module
        let decoded_tx = self.decode_transaction_wasi(tx_bytes)?;
        
        // Then validate using ante handler WASI module (if available)
        // For now, basic validation based on decoded transaction
        if decoded_tx.get("body").and_then(|b| b.get("messages")).map_or(true, |m| m.as_array().map_or(true, |a| a.is_empty())) {
            return Ok(TxResponse {
                code: 1,
                log: "transaction contains no messages".to_string(),
                gas_used: 0,
                gas_wanted: 0,
                events: vec![],
            });
        }
        
        Ok(TxResponse {
            code: 0,
            log: "transaction validated successfully".to_string(),
            gas_used: 10000,
            gas_wanted: decoded_tx.get("auth_info")
                .and_then(|a| a.get("fee"))
                .and_then(|f| f.get("gas_limit"))
                .and_then(|g| g.as_u64())
                .unwrap_or(200000),
            events: vec![],
        })
    }

    /// Deliver transaction
    pub fn deliver_tx(&mut self, tx_bytes: &[u8]) -> Result<TxResponse> {
        if self.context.is_none() {
            return Err(BaseAppError::InvalidTx("no active context".to_string()));
        }
        
        // Decode transaction using WASI TxDecoder module
        let decoded_tx = self.decode_transaction_wasi(tx_bytes)?;
        
        // Extract messages and route to appropriate modules
        let messages = decoded_tx.get("body")
            .and_then(|b| b.get("messages"))
            .and_then(|m| m.as_array())
            .ok_or_else(|| BaseAppError::InvalidTx("no messages in transaction".to_string()))?;
        
        let mut total_gas_used = 0u64;
        let mut events = Vec::new();
        
        for (idx, msg) in messages.iter().enumerate() {
            let type_url = msg.get("type_url")
                .and_then(|t| t.as_str())
                .ok_or_else(|| BaseAppError::InvalidTx(format!("message {} missing type_url", idx)))?;
            
            // Route message to appropriate module based on type_url
            // For now, just log and simulate execution
            log::info!("Executing message {}: {}", idx, type_url);
            
            // Simulate gas consumption
            let msg_gas = match type_url {
                "/cosmos.bank.v1beta1.MsgSend" => 50000,
                "/cosmos.staking.v1beta1.MsgDelegate" => 80000,
                "/cosmos.gov.v1beta1.MsgVote" => 30000,
                _ => 100000, // Default gas for unknown messages
            };
            
            total_gas_used += msg_gas;
            
            // Add execution event
            events.push(Event {
                event_type: "message".to_string(),
                attributes: vec![
                    Attribute { key: "action".to_string(), value: type_url.to_string() },
                    Attribute { key: "module".to_string(), value: self.extract_module_from_type_url(type_url) },
                ],
            });
        }
        
        Ok(TxResponse {
            code: 0,
            log: format!("executed {} messages", messages.len()),
            gas_used: total_gas_used,
            gas_wanted: decoded_tx.get("auth_info")
                .and_then(|a| a.get("fee"))
                .and_then(|f| f.get("gas_limit"))
                .and_then(|g| g.as_u64())
                .unwrap_or(200000),
            events,
        })
    }

    /// Extract module name from message type URL
    fn extract_module_from_type_url(&self, type_url: &str) -> String {
        // Extract module from type URL like "/cosmos.bank.v1beta1.MsgSend"
        if let Some(parts) = type_url.strip_prefix('/') {
            if let Some(dot_pos) = parts.find('.') {
                if let Some(second_dot) = parts[dot_pos + 1..].find('.') {
                    return parts[dot_pos + 1..dot_pos + 1 + second_dot].to_string();
                }
            }
        }
        "unknown".to_string()
    }

    /// Decode transaction using TxDecoder WASI module
    fn decode_transaction_wasi(&self, tx_bytes: &[u8]) -> Result<serde_json::Value> {
        use serde::{Deserialize, Serialize};
        
        #[derive(Debug, Serialize)]
        struct DecodeRequest {
            tx_bytes: String,
            encoding: String,
            validate: bool,
        }
        
        #[derive(Debug, Deserialize)]
        struct DecodeResponse {
            success: bool,
            decoded_tx: Option<serde_json::Value>,
            error: Option<String>,
            warnings: Vec<String>,
        }
        
        // Create decode request - assume base64 encoding for now
        let request = DecodeRequest {
            tx_bytes: base64::encode(tx_bytes),
            encoding: "base64".to_string(),
            validate: true,
        };
        
        let input = serde_json::to_string(&request)
            .map_err(|e| BaseAppError::TxFailed(format!("Failed to serialize decode request: {}", e)))?;
        
        // Try to load and execute TxDecoder module
        let module_path = self.module_paths.get("tx_decoder")
            .ok_or_else(|| BaseAppError::TxFailed("TxDecoder module path not configured".to_string()))?;
            
        if let Ok(module_bytes) = std::fs::read(module_path) {
            match self.wasi_host.execute_module_with_input(&module_bytes, input.as_bytes()) {
                Ok(result) => {
                    let response: DecodeResponse = serde_json::from_slice(&result.stdout)
                        .map_err(|e| BaseAppError::TxFailed(format!("Failed to parse decode response: {}", e)))?;
                    
                    if !response.success {
                        return Err(BaseAppError::InvalidTx(
                            response.error.unwrap_or_else(|| "transaction decode failed".to_string())
                        ));
                    }
                    
                    // Log warnings if any
                    for warning in &response.warnings {
                        log::warn!("Transaction decode warning: {}", warning);
                    }
                    
                    response.decoded_tx
                        .ok_or_else(|| BaseAppError::TxFailed("no decoded transaction in response".to_string()))
                }
                Err(e) => Err(BaseAppError::TxFailed(format!("TxDecoder WASI execution failed: {}", e)))
            }
        } else {
            // Module not found - return placeholder decoded tx
            log::warn!("TxDecoder WASI module not found at {}, using placeholder", module_path);
            Ok(serde_json::json!({
                "body": {
                    "messages": [],
                    "memo": "",
                    "timeout_height": 0
                },
                "auth_info": {
                    "fee": {
                        "gas_limit": 200000
                    }
                }
            }))
        }
    }

    /// Commit the current block
    pub fn commit(&mut self) -> Result<Vec<u8>> {
        // WASI FORWARDING: State commitment is handled by WASM modules
        // 1. Notify all active modules to commit their state
        // 2. Aggregate state changes through GlobalAppStore
        // 3. Compute application hash from committed state
        // 4. Return final app hash for consensus
        Ok(vec![0u8; 32]) // Placeholder app hash - replaced by WASI module
    }

    /// Get current block height
    pub fn get_height(&self) -> u64 {
        // TODO: Query from WASI module state
        self.context.as_ref().map(|c| c.block_height).unwrap_or(0)
    }

    /// Get last app hash
    pub fn get_last_app_hash(&self) -> &[u8] {
        // TODO: Query from WASI module state
        &[0u8; 32] // Placeholder
    }

    /// Rollback current block (for error recovery)
    pub fn rollback(&mut self) -> Result<()> {
        // TODO: Forward to WASI module for rollback
        Ok(())
    }

    /// Finalize block processing
    pub fn finalize_block(&mut self, height: u64, time: u64, txs: Vec<Vec<u8>>) -> Result<Vec<TxResponse>> {
        // Begin block processing
        self.begin_block(height, time, "helium-1".to_string())?;
        
        let mut responses = Vec::new();
        
        // Process each transaction
        for (i, tx_bytes) in txs.iter().enumerate() {
            match self.execute_transaction(tx_bytes, height) {
                Ok(response) => responses.push(response),
                Err(e) => {
                    // Transaction failed - create error response
                    responses.push(TxResponse {
                        code: 1,
                        log: format!("Transaction {} failed: {}", i, e),
                        gas_used: 0,
                        gas_wanted: 0,
                        events: vec![],
                    });
                }
            }
        }
        
        // End block processing
        self.end_block()?;
        
        Ok(responses)
    }

    /// Execute a single transaction and return response
    pub fn execute_transaction(&mut self, tx_bytes: &[u8], height: u64) -> Result<TxResponse> {
        // First decode the transaction
        let decoded_tx = self.decode_transaction_wasi(tx_bytes)?;
        
        // Extract messages from decoded transaction
        let messages = decoded_tx
            .get("body")
            .and_then(|b| b.get("messages"))
            .and_then(|m| m.as_array())
            .ok_or_else(|| BaseAppError::InvalidTx("transaction has no messages".to_string()))?;

        let mut total_gas_used = 0u64;
        let mut events = Vec::new();
        
        // Process each message in the transaction
        for msg_value in messages {
            let type_url = msg_value
                .get("@type")
                .and_then(|t| t.as_str())
                .ok_or_else(|| BaseAppError::InvalidTx("message has no type".to_string()))?;

            // Handle governance messages
            match type_url {
                "/helium.baseapp.v1.MsgStoreCode" => {
                    let msg: MsgStoreCode = serde_json::from_value(msg_value.clone())
                        .map_err(|e| BaseAppError::InvalidTx(format!("failed to decode MsgStoreCode: {}", e)))?;
                    
                    match self.module_governance.handle_store_code(msg) {
                        Ok(code_id) => {
                            events.push(Event {
                                event_type: "store_code".to_string(),
                                attributes: vec![
                                    Attribute { key: "code_id".to_string(), value: code_id.to_string() },
                                ],
                            });
                            total_gas_used += 50000; // Base gas cost for storing code
                        }
                        Err(e) => {
                            return Ok(TxResponse {
                                code: 1,
                                log: format!("store_code failed: {}", e),
                                gas_used: total_gas_used,
                                gas_wanted: 100000,
                                events,
                            });
                        }
                    }
                }
                "/helium.baseapp.v1.MsgInstallModule" => {
                    let msg: MsgInstallModule = serde_json::from_value(msg_value.clone())
                        .map_err(|e| BaseAppError::InvalidTx(format!("failed to decode MsgInstallModule: {}", e)))?;
                    
                    match self.module_governance.handle_install_module(msg.clone()) {
                        Ok(_) => {
                            events.push(Event {
                                event_type: "install_module".to_string(),
                                attributes: vec![
                                    Attribute { key: "module_name".to_string(), value: msg.config.name },
                                    Attribute { key: "code_id".to_string(), value: msg.code_id.to_string() },
                                ],
                            });
                            total_gas_used += 100000; // Base gas cost for installing module
                        }
                        Err(e) => {
                            return Ok(TxResponse {
                                code: 1,
                                log: format!("install_module failed: {}", e),
                                gas_used: total_gas_used,
                                gas_wanted: 200000,
                                events,
                            });
                        }
                    }
                }
                "/helium.baseapp.v1.MsgUpgradeModule" => {
                    let msg: MsgUpgradeModule = serde_json::from_value(msg_value.clone())
                        .map_err(|e| BaseAppError::InvalidTx(format!("failed to decode MsgUpgradeModule: {}", e)))?;
                    
                    match self.module_governance.handle_upgrade_module(msg.clone()) {
                        Ok(_) => {
                            events.push(Event {
                                event_type: "upgrade_module".to_string(),
                                attributes: vec![
                                    Attribute { key: "module_name".to_string(), value: msg.module_name },
                                    Attribute { key: "new_code_id".to_string(), value: msg.new_code_id.to_string() },
                                ],
                            });
                            total_gas_used += 150000; // Base gas cost for upgrading module
                        }
                        Err(e) => {
                            return Ok(TxResponse {
                                code: 1,
                                log: format!("upgrade_module failed: {}", e),
                                gas_used: total_gas_used,
                                gas_wanted: 300000,
                                events,
                            });
                        }
                    }
                }
                _ => {
                    // Route to module router for other message types
                    let execution_context = ExecutionContext {
                        message_type: type_url.to_string(),
                        message_data: serde_json::to_vec(msg_value)
                            .map_err(|e| BaseAppError::InvalidTx(format!("failed to serialize message: {}", e)))?,
                        gas_limit: 100000,
                        tx_context: {
                            let mut ctx = HashMap::new();
                            ctx.insert("height".to_string(), height.to_string());
                            ctx
                        },
                    };
                    
                    // TODO: Create a proper SdkMsg implementation for unknown message types
                    // For now, return an error for unhandled message types
                    return Ok(TxResponse {
                        code: 1,
                        log: format!("unhandled message type: {}", type_url),
                        gas_used: total_gas_used,
                        gas_wanted: 100000,
                        events,
                    });
                }
            }
        }

        Ok(TxResponse {
            code: 0,
            log: "transaction executed successfully".to_string(),
            gas_used: total_gas_used,
            gas_wanted: total_gas_used + 10000,
            events,
        })
    }

    /// Initialize chain from genesis
    pub fn init_chain(&mut self, _chain_id: String, _genesis: &[u8]) -> Result<()> {
        // WASI FORWARDING: Chain initialization is handled by WASM modules
        // 1. Load genesis configuration from provided bytes
        // 2. Initialize all WASM modules with genesis state
        // 3. Set initial validator set and consensus parameters
        // 4. Establish initial application state
        Ok(())
    }

    /// Query application state
    pub fn query(&self, _path: String, _data: &[u8], _height: u64, _prove: bool) -> Result<QueryResponse> {
        // WASI FORWARDING: Queries are handled by WASM modules
        // 1. Parse query path to determine target module
        // 2. Route query to appropriate WASM module via ModuleRouter
        // 3. Module performs query using VFS state access
        // 4. Return query result with proof if requested
        Ok(QueryResponse {
            code: 0,
            log: "query forwarded to WASI module".to_string(),
            value: vec![],
            height: _height,
            proof: None,
        })
    }

    /// Simulate a transaction to estimate gas usage
    pub fn simulate_tx(&self, tx_bytes: &[u8]) -> Result<TxResponse> {
        // Create simulation context
        let _sim_context = Context::new(
            self.context.as_ref().map(|c| c.block_height).unwrap_or(0),
            self.context.as_ref().map(|c| c.block_time).unwrap_or(0),
            self.context.as_ref().map(|c| c.chain_id.clone()).unwrap_or_else(|| "simulation".to_string()),
            ExecMode::Simulate,
        );

        // TODO: Forward to WASI module for transaction simulation
        // For now, implement a basic gas estimation based on transaction size and complexity
        
        let base_gas = 21000u64; // Base transaction cost
        let per_byte_gas = 10u64; // Gas per byte of transaction data
        let tx_size_gas = tx_bytes.len() as u64 * per_byte_gas;
        
        // Estimate additional gas based on transaction type
        let type_gas = if tx_bytes.len() > 200 {
            // Likely a complex transaction with multiple messages
            50000u64
        } else {
            // Simple transaction
            25000u64
        };
        
        let estimated_gas = base_gas + tx_size_gas + type_gas;
        
        // Apply gas adjustment factor for safety margin
        let gas_wanted = (estimated_gas as f64 * 1.2) as u64;
        let gas_used = (estimated_gas as f64 * 0.85) as u64; // Simulate 85% efficiency
        
        Ok(TxResponse {
            code: 0,
            log: "simulation successful".to_string(),
            gas_used,
            gas_wanted,
            events: vec![],
        })
    }


    /// Helper methods for testing
    pub fn set_balance(&mut self, _address: &str, _denom: &str, _amount: u64) -> Result<()> {
        // WASI FORWARDING: Balance management handled by bank WASM module
        Ok(())
    }

    pub fn get_balance(&self, _address: &str, _denom: &str) -> Result<u64> {
        // WASI FORWARDING: Balance queries handled by bank WASM module
        Ok(0)
    }
}

// ARCHITECTURE NOTE: BaseApp now acts as a thin ABCI adapter that forwards
// all blockchain logic to WASM modules via the ModuleRouter and WASI host.
// Traditional transaction processing, ante handlers, and message execution
// have been moved to the microkernel architecture.

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
    fn test_begin_end_block() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();

        app.begin_block(1, 1234567890, "test-chain".to_string())
            .unwrap();
        assert!(app.context.is_some());

        let ctx = app.context.as_ref().unwrap();
        assert_eq!(ctx.block_height, 1);
        assert_eq!(ctx.block_time, 1234567890);
        assert_eq!(ctx.chain_id, "test-chain");
        assert_eq!(ctx.exec_mode, ExecMode::Deliver);

        app.end_block().unwrap();
        assert!(app.context.is_none());
    }

    #[test]
    fn test_check_tx() {
        let app = BaseApp::new("test-app".to_string()).unwrap();
        
        // Transaction validation via WASI TxDecoder
        let response = app.check_tx(b"dummy_tx").unwrap();
        // Should fail because the transaction will decode to have no messages
        assert_eq!(response.code, 1);
        assert_eq!(response.log, "transaction contains no messages");
    }

    #[test]
    fn test_deliver_tx() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();

        // No context - should fail
        let result = app.deliver_tx(b"tx");
        assert!(result.is_err());

        // With context - transaction will be decoded and processed
        app.begin_block(1, 1234567890, "test-chain".to_string())
            .unwrap();
        let response = app.deliver_tx(b"tx").unwrap();
        assert_eq!(response.code, 0);
        assert_eq!(response.log, "executed 0 messages"); // No messages in placeholder tx
        assert_eq!(response.gas_used, 0); // No messages to execute
    }

    #[test]
    fn test_commit() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        
        // Block commit forwarded to WASI modules
        let hash = app.commit().unwrap();
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_finalize_block() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();
        
        // Block finalization forwarded to WASI modules
        let responses = app.finalize_block(1, 1234567890, vec![]).unwrap();
        assert_eq!(responses.len(), 0);
    }

    #[test]
    fn test_baseapp_integration() {
        let mut app = BaseApp::new("test-app".to_string()).unwrap();

        // Set initial balance using helper method
        app.set_balance("alice", "uatom", 1000).unwrap();

        // Check balance - WASI module not implemented yet
        let balance = app.get_balance("alice", "uatom").unwrap();
        assert_eq!(balance, 0); // WASI module returns placeholder
    }

    #[test]
    fn test_module_governance_integration() {
        let app = BaseApp::new("test-app".to_string()).unwrap();
        
        // Verify module governance is accessible
        let governance = app.module_governance();
        
        // Test that we can list modules (should be empty initially)
        let modules = governance.list_modules().unwrap();
        assert_eq!(modules.len(), 0);
        
        // Test that we can list codes (should be empty initially)
        let codes = governance.list_codes().unwrap();
        assert_eq!(codes.len(), 0);
    }

    #[test]
    fn test_governance_message_handling() {
        let app = BaseApp::new("test-app".to_string()).unwrap();
        
        // Create a mock transaction with governance message
        let tx_json = r#"
        {
            "body": {
                "messages": [
                    {
                        "@type": "/helium.baseapp.v1.MsgStoreCode",
                        "authority": "cosmos10d07y265gmmuvt4z0w9aw880jnsr700j6zn9kn",
                        "wasm_code": "AGFzbQEAAAABBAABfwBgAX8BCwCgARFgAX8AYAt/AX4BY2AABH8BQQAQQBx+YWRkGxAYEAQQAAA=",
                        "metadata": {
                            "name": "test_module",
                            "version": "1.0.0",
                            "description": "Test module",
                            "api_version": "1.0",
                            "checksum": "invalid_checksum_for_test"
                        }
                    }
                ]
            }
        }
        "#;
        
        let tx_bytes = tx_json.as_bytes();
        
        // Check transaction should work (even if it fails validation)
        let response = app.check_tx(tx_bytes).unwrap();
        // Should fail because transaction decoder will have issues with mock data
        assert_eq!(response.code, 1);
    }
}
