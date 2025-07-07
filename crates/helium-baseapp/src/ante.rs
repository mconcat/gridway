//! WASI Ante Handler Bridge
//!
//! This module provides a bridge to the WASI-based ante handler module.
//! Instead of hardcoded ante handlers, it dynamically loads and executes
//! the ante handler as a WASM module following the microkernel architecture.

use crate::wasi_host::WasiHost;
use helium_proto::cometbft::abci::v1::{Event, EventAttribute, ExecTxResult as TxResponse};
use helium_types::{RawTx, SdkError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, error, info, warn};

/// Ante handler errors
#[derive(Error, Debug)]
pub enum AnteError {
    /// WASI module execution failed
    #[error("WASI module error: {0}")]
    WasiError(String),

    /// Module not found
    #[error("Ante handler module not found: {0}")]
    ModuleNotFound(String),

    /// Invalid response from WASI module
    #[error("Invalid WASI response: {0}")]
    InvalidResponse(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// SDK error
    #[error("SDK error: {0}")]
    SdkError(#[from] SdkError),
}

/// Result type for ante handlers
pub type AnteResult<T> = Result<T, AnteError>;

/// Context for ante handler execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnteContext {
    /// Block height
    pub block_height: u64,
    /// Block time
    pub block_time: u64,
    /// Chain ID
    pub chain_id: String,
    /// Gas meter for tracking gas consumption
    pub gas_used: u64,
    /// Gas limit for this transaction
    pub gas_limit: u64,
    /// Minimum gas price
    pub min_gas_price: u64,
}

impl AnteContext {
    /// Create a new ante context
    pub fn new(
        block_height: u64,
        block_time: u64,
        chain_id: String,
        gas_limit: u64,
        min_gas_price: u64,
    ) -> Self {
        Self {
            block_height,
            block_time,
            chain_id,
            gas_used: 0,
            gas_limit,
            min_gas_price,
        }
    }

    /// Consume gas and check if limit is exceeded
    pub fn consume_gas(&mut self, amount: u64) -> AnteResult<()> {
        self.gas_used += amount;
        if self.gas_used > self.gas_limit {
            return Err(AnteError::WasiError(format!(
                "Gas limit exceeded: wanted {}, limit {}",
                self.gas_used, self.gas_limit
            )));
        }
        Ok(())
    }

    /// Get remaining gas
    pub fn remaining_gas(&self) -> u64 {
        self.gas_limit.saturating_sub(self.gas_used)
    }
}

/// WASI ante handler response
#[derive(Debug, Serialize, Deserialize)]
pub struct WasiAnteResponse {
    pub success: bool,
    pub gas_used: u64,
    pub error: Option<String>,
    pub events: Vec<WasiEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasiEvent {
    pub event_type: String,
    pub attributes: Vec<WasiAttribute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasiAttribute {
    pub key: String,
    pub value: String,
}

/// WASI-based ante handler that loads and executes WASM modules
pub struct WasiAnteHandler {
    /// WASI host for executing modules
    wasi_host: WasiHost,
    /// Module cache for loaded ante handlers
    module_cache: HashMap<String, Vec<u8>>,
}

impl WasiAnteHandler {
    /// Create a new WASI ante handler
    pub fn new() -> AnteResult<Self> {
        let wasi_host = WasiHost::new().map_err(|e| AnteError::WasiError(e.to_string()))?;

        Ok(Self {
            wasi_host,
            module_cache: HashMap::new(),
        })
    }

    /// Load ante handler module from filesystem
    pub fn load_module(&mut self, module_name: &str, module_path: &str) -> AnteResult<()> {
        info!(
            "Loading WASI ante handler module: {} from {}",
            module_name, module_path
        );

        let module_bytes = std::fs::read(module_path).map_err(|e| {
            AnteError::ModuleNotFound(format!("Failed to read module {module_path}: {e}"))
        })?;

        // Validate WASM module
        self.wasi_host
            .validate_module(&module_bytes)
            .map_err(|e| AnteError::WasiError(format!("Invalid WASM module: {e}")))?;

        self.module_cache
            .insert(module_name.to_string(), module_bytes);
        info!("Successfully loaded ante handler module: {}", module_name);

        Ok(())
    }

    /// Execute ante handler for transaction validation
    pub fn handle(&mut self, ctx: &mut AnteContext, tx: &RawTx) -> AnteResult<TxResponse> {
        debug!(
            "WASI Ante Handler: Processing transaction for chain {}",
            ctx.chain_id
        );

        // Convert RawTx to WASI format
        let wasi_tx = self.convert_tx_to_wasi(tx)?;
        let wasi_ctx = self.convert_context_to_wasi(ctx)?;

        // Prepare input for WASI module
        let input = serde_json::to_string(&(wasi_ctx, wasi_tx))
            .map_err(|e| AnteError::SerializationError(e.to_string()))?;

        // Execute the ante handler module
        let response = self.execute_ante_module("default", &input)?;

        // Update context with gas used
        ctx.gas_used += response.gas_used;

        // Convert WASI events to proto events
        let events = response
            .events
            .into_iter()
            .map(|e| Event {
                r#type: e.event_type,
                attributes: e
                    .attributes
                    .into_iter()
                    .map(|a| EventAttribute {
                        key: a.key,
                        value: a.value,
                        index: true,
                    })
                    .collect(),
            })
            .collect();

        // Convert response
        Ok(TxResponse {
            code: if response.success { 0 } else { 1 },
            data: vec![],
            log: response
                .error
                .unwrap_or_else(|| "ante handler validation completed".to_string()),
            info: String::new(),
            gas_wanted: ctx.gas_limit as i64,
            gas_used: response.gas_used as i64,
            events,
            codespace: String::new(),
        })
    }

    fn execute_ante_module(
        &mut self,
        module_name: &str,
        input: &str,
    ) -> AnteResult<WasiAnteResponse> {
        // Get module bytes from cache
        let module_bytes = self.module_cache.get(module_name).ok_or_else(|| {
            AnteError::ModuleNotFound(format!("Module not loaded: {module_name}"))
        })?;

        // Execute WASI module
        let result = self
            .wasi_host
            .execute_module_with_input(module_bytes, input.as_bytes())
            .map_err(|e| AnteError::WasiError(format!("WASI execution failed: {e}")))?;

        // Parse response
        let response: WasiAnteResponse = serde_json::from_slice(&result.stdout).map_err(|e| {
            AnteError::InvalidResponse(format!("Failed to parse WASI response: {e}"))
        })?;

        if !result.stderr.is_empty() {
            warn!(
                "WASI ante handler stderr: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }

        Ok(response)
    }

    fn convert_tx_to_wasi(&self, tx: &RawTx) -> AnteResult<serde_json::Value> {
        // Convert RawTx to the WASI Transaction format
        // This is a simplified conversion - in a real implementation,
        // you would need proper protobuf deserialization

        let wasi_tx = serde_json::json!({
            "body": {
                "messages": tx.body.messages.iter().map(|msg| {
                    serde_json::json!({
                        "type_url": msg.type_url,
                        "value": msg.value
                    })
                }).collect::<Vec<_>>(),
                "memo": tx.body.memo,
                "timeout_height": tx.body.timeout_height
            },
            "auth_info": {
                "signer_infos": tx.auth_info.signer_infos.iter().map(|si| {
                    serde_json::json!({
                        "public_key": si.public_key.as_ref().map(|pk| {
                            serde_json::json!({
                                "type_url": pk.type_url,
                                "value": pk.value
                            })
                        }),
                        "sequence": si.sequence,
                        "mode_info": {
                            "mode": si.mode_info.single.as_ref().map(|s| s.mode).unwrap_or(1)
                        }
                    })
                }).collect::<Vec<_>>(),
                "fee": {
                    "amount": tx.auth_info.fee.amount.iter().map(|coin| {
                        serde_json::json!({
                            "denom": coin.denom,
                            "amount": coin.amount
                        })
                    }).collect::<Vec<_>>(),
                    "gas_limit": tx.auth_info.fee.gas_limit,
                    "payer": tx.auth_info.fee.payer,
                    "granter": tx.auth_info.fee.granter
                }
            },
            "signatures": tx.signatures
        });

        Ok(wasi_tx)
    }

    fn convert_context_to_wasi(&self, ctx: &AnteContext) -> AnteResult<serde_json::Value> {
        Ok(serde_json::json!({
            "block_height": ctx.block_height,
            "block_time": ctx.block_time,
            "chain_id": ctx.chain_id,
            "gas_limit": ctx.gas_limit,
            "min_gas_price": ctx.min_gas_price
        }))
    }

    /// Get gas consumption for this handler
    pub fn gas_cost(&self) -> u64 {
        1000 // Base gas cost for WASI module loading
    }
}

impl Default for WasiAnteHandler {
    fn default() -> Self {
        Self::new().expect("Failed to create default WASI ante handler")
    }
}

/// Ante handler chain for composing multiple WASI modules
pub struct WasiAnteHandlerChain {
    /// List of WASI ante handler modules to execute in order
    handlers: Vec<WasiAnteHandler>,
}

impl WasiAnteHandlerChain {
    /// Create a new ante handler chain
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Add a WASI handler to the chain
    pub fn add_handler(mut self, handler: WasiAnteHandler) -> Self {
        self.handlers.push(handler);
        self
    }

    /// Create a default ante handler chain with standard WASI modules
    pub fn default_chain() -> AnteResult<Self> {
        let mut handler = WasiAnteHandler::new()?;

        // Try to load the default ante handler module
        if let Err(e) = handler.load_module("default", "modules/ante_handler.wasm") {
            warn!(
                "Failed to load default ante handler module: {}. Using placeholder.",
                e
            );
        }

        Ok(Self::new().add_handler(handler))
    }

    /// Execute all WASI handlers in the chain
    pub fn handle(&mut self, ctx: &mut AnteContext, tx: &RawTx) -> AnteResult<TxResponse> {
        let mut combined_response = TxResponse {
            code: 0,
            data: vec![],
            log: String::new(),
            info: String::new(),
            gas_wanted: ctx.gas_limit as i64,
            gas_used: 0,
            events: Vec::new(),
            codespace: String::new(),
        };

        for handler in &mut self.handlers {
            let response = handler.handle(ctx, tx)?;

            if response.code != 0 {
                return Ok(response); // Return first error
            }

            combined_response.gas_used += response.gas_used;
            combined_response.events.extend(response.events);

            if !response.log.is_empty() {
                if !combined_response.log.is_empty() {
                    combined_response.log.push_str("; ");
                }
                combined_response.log.push_str(&response.log);
            }
        }

        Ok(combined_response)
    }

    /// Get total gas cost for all handlers
    pub fn total_gas_cost(&self) -> u64 {
        self.handlers.iter().map(|h| h.gas_cost()).sum()
    }
}

impl Default for WasiAnteHandlerChain {
    fn default() -> Self {
        Self::default_chain().unwrap_or_else(|e| {
            error!("Failed to create default WASI ante handler chain: {}", e);
            Self::new()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helium_types::tx::{ModeInfo, ModeInfoSingle};
    use helium_types::{AuthInfo, Fee, FeeAmount, SignerInfo, TxBody, TxMessage};

    fn create_test_context() -> AnteContext {
        AnteContext::new(100, 1234567890, "test-chain".to_string(), 200000, 1)
    }

    fn create_test_tx() -> RawTx {
        RawTx {
            body: TxBody {
                messages: vec![TxMessage {
                    type_url: "/cosmos.bank.v1beta1.MsgSend".to_string(),
                    value: b"test_message".to_vec(),
                }],
                memo: "test".to_string(),
                timeout_height: 0,
            },
            auth_info: AuthInfo {
                signer_infos: vec![SignerInfo {
                    public_key: Some(TxMessage {
                        type_url: "/cosmos.crypto.secp256k1.PubKey".to_string(),
                        value: vec![1; 33],
                    }),
                    mode_info: ModeInfo {
                        single: Some(ModeInfoSingle { mode: 1 }),
                    },
                    sequence: 0,
                }],
                fee: Fee {
                    amount: vec![FeeAmount {
                        denom: "uatom".to_string(),
                        amount: "250000".to_string(),
                    }],
                    gas_limit: 200000,
                    payer: "".to_string(),
                    granter: "".to_string(),
                },
            },
            signatures: vec![vec![1; 64]],
        }
    }

    #[test]
    fn test_ante_context() {
        let mut ctx = create_test_context();

        assert_eq!(ctx.block_height, 100);
        assert_eq!(ctx.gas_used, 0);
        assert_eq!(ctx.remaining_gas(), 200000);

        // Test gas consumption
        ctx.consume_gas(1000).unwrap();
        assert_eq!(ctx.gas_used, 1000);
        assert_eq!(ctx.remaining_gas(), 199000);
    }

    #[test]
    fn test_wasi_ante_handler_creation() {
        let handler = WasiAnteHandler::new();
        assert!(handler.is_ok());
    }

    #[test]
    fn test_wasi_ante_handler_chain() {
        let chain = WasiAnteHandlerChain::new();
        assert_eq!(chain.handlers.len(), 0);
    }

    #[test]
    fn test_tx_conversion() {
        let handler = WasiAnteHandler::new().unwrap();
        let tx = create_test_tx();

        let wasi_tx = handler.convert_tx_to_wasi(&tx);
        assert!(wasi_tx.is_ok());

        let converted = wasi_tx.unwrap();
        assert!(converted["body"]["messages"].is_array());
        assert!(converted["auth_info"]["signer_infos"].is_array());
    }

    #[test]
    fn test_context_conversion() {
        let handler = WasiAnteHandler::new().unwrap();
        let ctx = create_test_context();

        let wasi_ctx = handler.convert_context_to_wasi(&ctx);
        assert!(wasi_ctx.is_ok());

        let converted = wasi_ctx.unwrap();
        assert_eq!(converted["block_height"], 100);
        assert_eq!(converted["chain_id"], "test-chain");
    }
}
