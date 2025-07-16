//! WASI Ante Handler Adapter
//!
//! This module provides a thin adapter to the WASI-based ante handler module.
//! All transaction validation logic resides in the WASI module itself.

use crate::wasi_host::WasiHost;
use helium_types::RawTx;
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
}

/// Result type for ante handlers
pub type AnteResult<T> = Result<T, AnteError>;

/// Context for ante handler execution - passed to WASI module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnteContext {
    /// Block height
    pub block_height: u64,
    /// Block time
    pub block_time: u64,
    /// Chain ID
    pub chain_id: String,
    /// Gas limit for this transaction
    pub gas_limit: u64,
    /// Minimum gas price
    pub min_gas_price: u64,
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

/// Transaction response for ante handler
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
    pub events: Vec<WasiEvent>,
}

/// WASI-based ante handler adapter
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
    pub fn handle(&mut self, ctx: &AnteContext, tx: &RawTx) -> AnteResult<TxResponse> {
        debug!(
            "WASI Ante Handler: Processing transaction for chain {}",
            ctx.chain_id
        );

        // Prepare input for WASI module - the module expects (context, transaction)
        let input = serde_json::to_string(&(ctx, tx))
            .map_err(|e| AnteError::SerializationError(e.to_string()))?;

        // Execute the ante handler module
        let response = self.execute_ante_module("default", &input)?;

        // Convert response
        Ok(TxResponse {
            code: if response.success { 0 } else { 1 },
            log: response
                .error
                .unwrap_or_else(|| "ante handler validation completed".to_string()),
            gas_used: response.gas_used,
            gas_wanted: ctx.gas_limit,
            events: response.events,
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
}

impl Default for WasiAnteHandler {
    fn default() -> Self {
        Self::new().expect("Failed to create default WASI ante handler")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helium_types::tx::{ModeInfo, ModeInfoSingle};
    use helium_types::{AuthInfo, Fee, FeeAmount, SignerInfo, TxBody, TxMessage};

    fn create_test_context() -> AnteContext {
        AnteContext {
            block_height: 100,
            block_time: 1234567890,
            chain_id: "test-chain".to_string(),
            gas_limit: 200000,
            min_gas_price: 1,
        }
    }

    #[allow(dead_code)]
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
        let ctx = create_test_context();
        assert_eq!(ctx.block_height, 100);
        assert_eq!(ctx.gas_limit, 200000);
    }

    #[test]
    fn test_wasi_ante_handler_creation() {
        let handler = WasiAnteHandler::new();
        assert!(handler.is_ok());
    }
}
