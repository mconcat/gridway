//! WASI Transaction Decoder Module
//!
//! This module implements transaction decoding as a WASI program that can be
//! dynamically loaded by the BaseApp. It handles protobuf decoding of Cosmos SDK
//! transactions, supporting various message types and encoding formats.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use thiserror::Error;
use base64::{engine::general_purpose::STANDARD, Engine};

/// Error types for transaction decoder operations
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum TxDecodeError {
    #[error("Invalid transaction format: {0}")]
    InvalidFormat(String),

    #[error("Protobuf decode error: {0}")]
    ProtobufError(String),

    #[error("Unknown message type: {0}")]
    UnknownMessageType(String),

    #[error("Invalid message data: {0}")]
    InvalidMessageData(String),

    #[error("Unsupported encoding: {0}")]
    UnsupportedEncoding(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Result type for transaction decoder operations
pub type TxDecodeResult<T> = Result<T, TxDecodeError>;

/// Transaction decode request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodeRequest {
    /// Raw transaction bytes (could be base64 or hex encoded)
    pub tx_bytes: String,
    /// Encoding format: "raw", "base64", or "hex"
    pub encoding: String,
    /// Whether to validate the transaction structure
    pub validate: bool,
}

/// Decoded transaction structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedTx {
    pub body: TxBody,
    pub auth_info: AuthInfo,
    pub signatures: Vec<String>, // Hex encoded signatures
    pub tx_hash: String,         // Transaction hash
    pub size_bytes: usize,       // Transaction size
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxBody {
    pub messages: Vec<DecodedMessage>,
    pub memo: String,
    pub timeout_height: u64,
    pub extension_options: Vec<Any>,
    pub non_critical_extension_options: Vec<Any>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedMessage {
    pub type_url: String,
    pub value: serde_json::Value, // Decoded message as JSON
    pub raw_value: String,        // Hex encoded raw bytes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Any {
    pub type_url: String,
    pub value: String, // Hex encoded
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthInfo {
    pub signer_infos: Vec<SignerInfo>,
    pub fee: Fee,
    pub tip: Option<Tip>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignerInfo {
    pub public_key: Option<Any>,
    pub mode_info: ModeInfo,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfo {
    pub single: Option<ModeInfoSingle>,
    pub multi: Option<ModeInfoMulti>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfoSingle {
    pub mode: String, // "SIGN_MODE_DIRECT", "SIGN_MODE_TEXTUAL", etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfoMulti {
    pub bitarray: CompactBitArray,
    pub mode_infos: Vec<ModeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactBitArray {
    pub extra_bits_stored: u32,
    pub elems: String, // Hex encoded
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fee {
    pub amount: Vec<Coin>,
    pub gas_limit: u64,
    pub payer: String,
    pub granter: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coin {
    pub denom: String,
    pub amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tip {
    pub amount: Vec<Coin>,
    pub tipper: String,
}

/// Decode response
#[derive(Debug, Serialize, Deserialize)]
pub struct DecodeResponse {
    pub success: bool,
    pub decoded_tx: Option<DecodedTx>,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

/// Message type registry for decoding known message types
type MessageHandler = Box<dyn Fn(&[u8]) -> TxDecodeResult<serde_json::Value>>;

pub struct MessageRegistry {
    handlers: std::collections::HashMap<String, MessageHandler>,
}

impl Default for MessageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            handlers: std::collections::HashMap::new(),
        };

        // Register known message types
        registry.register_cosmos_messages();
        registry
    }

    fn register_cosmos_messages(&mut self) {
        // Bank messages
        self.handlers.insert(
            "/cosmos.bank.v1beta1.MsgSend".to_string(),
            Box::new(decode_msg_send),
        );

        self.handlers.insert(
            "/cosmos.bank.v1beta1.MsgMultiSend".to_string(),
            Box::new(decode_msg_multi_send),
        );

        // Staking messages
        self.handlers.insert(
            "/cosmos.staking.v1beta1.MsgDelegate".to_string(),
            Box::new(decode_msg_delegate),
        );

        self.handlers.insert(
            "/cosmos.staking.v1beta1.MsgUndelegate".to_string(),
            Box::new(decode_msg_undelegate),
        );

        // Governance messages
        self.handlers.insert(
            "/cosmos.gov.v1beta1.MsgSubmitProposal".to_string(),
            Box::new(decode_msg_submit_proposal),
        );

        self.handlers.insert(
            "/cosmos.gov.v1beta1.MsgVote".to_string(),
            Box::new(decode_msg_vote),
        );
    }

    #[allow(dead_code)]
    fn decode_message(&self, type_url: &str, value: &[u8]) -> TxDecodeResult<serde_json::Value> {
        if let Some(handler) = self.handlers.get(type_url) {
            handler(value)
        } else {
            // Unknown message type - return as generic Any
            Ok(serde_json::json!({
                "@type": type_url,
                "value": STANDARD.encode(value)
            }))
        }
    }
}

/// WASI Transaction Decoder implementation
pub struct WasiTxDecoder {
    #[allow(dead_code)]
    registry: MessageRegistry,
}

impl WasiTxDecoder {
    pub fn new() -> Self {
        Self {
            registry: MessageRegistry::new(),
        }
    }

    /// Main entry point for transaction decoding
    pub fn decode(&self, req: &DecodeRequest) -> DecodeResponse {
        log::info!(
            "WASI TxDecoder: Decoding transaction with {} encoding",
            req.encoding
        );

        match self.decode_transaction(req) {
            Ok(decoded_tx) => {
                let warnings = if req.validate {
                    self.validate_transaction(&decoded_tx)
                } else {
                    vec![]
                };

                DecodeResponse {
                    success: true,
                    decoded_tx: Some(decoded_tx),
                    error: None,
                    warnings,
                }
            }
            Err(e) => DecodeResponse {
                success: false,
                decoded_tx: None,
                error: Some(e.to_string()),
                warnings: vec![],
            },
        }
    }

    fn decode_transaction(&self, req: &DecodeRequest) -> TxDecodeResult<DecodedTx> {
        // Decode input based on encoding
        let raw_bytes = match req.encoding.as_str() {
            "raw" => req.tx_bytes.as_bytes().to_vec(),
            "base64" => STANDARD.decode(&req.tx_bytes)
                .map_err(|e| TxDecodeError::InvalidFormat(format!("Invalid base64: {e}")))?,
            "hex" => hex::decode(&req.tx_bytes)
                .map_err(|e| TxDecodeError::InvalidFormat(format!("Invalid hex: {e}")))?,
            _ => return Err(TxDecodeError::UnsupportedEncoding(req.encoding.clone())),
        };

        // Calculate transaction hash
        let tx_hash = self.calculate_tx_hash(&raw_bytes);

        // For this example, we'll use a simplified protobuf structure
        // In reality, this would use the actual Cosmos SDK protobuf definitions
        let decoded = self.decode_cosmos_tx(&raw_bytes)?;

        Ok(DecodedTx {
            body: decoded.body,
            auth_info: decoded.auth_info,
            signatures: decoded.signatures,
            tx_hash,
            size_bytes: raw_bytes.len(),
        })
    }

    fn decode_cosmos_tx(&self, bytes: &[u8]) -> TxDecodeResult<DecodedTx> {
        // Simplified decoding - in reality would use proper protobuf
        // For now, parse a JSON representation for demonstration

        // This is a placeholder implementation
        // Real implementation would decode actual protobuf bytes

        let body = TxBody {
            messages: vec![DecodedMessage {
                type_url: "/cosmos.bank.v1beta1.MsgSend".to_string(),
                value: serde_json::json!({
                    "from_address": "cosmos1...",
                    "to_address": "cosmos1...",
                    "amount": [{"denom": "uatom", "amount": "1000000"}]
                }),
                raw_value: hex::encode(&bytes[0..32.min(bytes.len())]),
            }],
            memo: "Example transaction".to_string(),
            timeout_height: 0,
            extension_options: vec![],
            non_critical_extension_options: vec![],
        };

        let auth_info = AuthInfo {
            signer_infos: vec![SignerInfo {
                public_key: Some(Any {
                    type_url: "/cosmos.crypto.secp256k1.PubKey".to_string(),
                    value: hex::encode([0u8; 33]),
                }),
                mode_info: ModeInfo {
                    single: Some(ModeInfoSingle {
                        mode: "SIGN_MODE_DIRECT".to_string(),
                    }),
                    multi: None,
                },
                sequence: 0,
            }],
            fee: Fee {
                amount: vec![Coin {
                    denom: "uatom".to_string(),
                    amount: "1000".to_string(),
                }],
                gas_limit: 200000,
                payer: String::new(),
                granter: String::new(),
            },
            tip: None,
        };

        Ok(DecodedTx {
            body,
            auth_info,
            signatures: vec![hex::encode([0u8; 64])],
            tx_hash: String::new(),
            size_bytes: bytes.len(),
        })
    }

    fn calculate_tx_hash(&self, bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(bytes);
        hex::encode(hash).to_uppercase()
    }

    fn validate_transaction(&self, tx: &DecodedTx) -> Vec<String> {
        let mut warnings = vec![];

        // Check transaction size
        if tx.size_bytes > 1_000_000 {
            warnings.push(format!(
                "Transaction size {} exceeds recommended limit",
                tx.size_bytes
            ));
        }

        // Check memo length
        if tx.body.memo.len() > 256 {
            warnings.push("Memo exceeds 256 character limit".to_string());
        }

        // Check gas limit
        if tx.auth_info.fee.gas_limit < 50000 {
            warnings.push("Gas limit may be too low for transaction execution".to_string());
        }

        // Check for empty messages
        if tx.body.messages.is_empty() {
            warnings.push("Transaction contains no messages".to_string());
        }

        // Check signature count matches signer info
        if tx.signatures.len() != tx.auth_info.signer_infos.len() {
            warnings.push("Signature count doesn't match signer info count".to_string());
        }

        warnings
    }
}

// Message decoding functions
fn decode_msg_send(_bytes: &[u8]) -> TxDecodeResult<serde_json::Value> {
    // Simplified - would use actual protobuf decoding
    Ok(serde_json::json!({
        "from_address": "cosmos1example...",
        "to_address": "cosmos1recipient...",
        "amount": [{"denom": "uatom", "amount": "1000000"}]
    }))
}

fn decode_msg_multi_send(_bytes: &[u8]) -> TxDecodeResult<serde_json::Value> {
    Ok(serde_json::json!({
        "inputs": [{"address": "cosmos1...", "coins": [{"denom": "uatom", "amount": "1000000"}]}],
        "outputs": [{"address": "cosmos1...", "coins": [{"denom": "uatom", "amount": "1000000"}]}]
    }))
}

fn decode_msg_delegate(_bytes: &[u8]) -> TxDecodeResult<serde_json::Value> {
    Ok(serde_json::json!({
        "delegator_address": "cosmos1...",
        "validator_address": "cosmosvaloper1...",
        "amount": {"denom": "uatom", "amount": "1000000"}
    }))
}

fn decode_msg_undelegate(_bytes: &[u8]) -> TxDecodeResult<serde_json::Value> {
    Ok(serde_json::json!({
        "delegator_address": "cosmos1...",
        "validator_address": "cosmosvaloper1...",
        "amount": {"denom": "uatom", "amount": "1000000"}
    }))
}

fn decode_msg_submit_proposal(_bytes: &[u8]) -> TxDecodeResult<serde_json::Value> {
    Ok(serde_json::json!({
        "content": {
            "@type": "/cosmos.gov.v1beta1.TextProposal",
            "title": "Example Proposal",
            "description": "This is an example proposal"
        },
        "initial_deposit": [{"denom": "uatom", "amount": "1000000"}],
        "proposer": "cosmos1..."
    }))
}

fn decode_msg_vote(_bytes: &[u8]) -> TxDecodeResult<serde_json::Value> {
    Ok(serde_json::json!({
        "proposal_id": "1",
        "voter": "cosmos1...",
        "option": "VOTE_OPTION_YES"
    }))
}

/// WASI entry point function
/// This function is called by the WASI host to decode transactions
#[no_mangle]
pub extern "C" fn decode_tx() -> i32 {
    // Initialize logging
    env_logger::init();

    let decoder = WasiTxDecoder::new();

    // Read input from stdin
    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        log::error!("Failed to read input: {e}");
        return 1;
    }

    // Parse decode request
    let request: DecodeRequest = match serde_json::from_str(&input) {
        Ok(data) => data,
        Err(e) => {
            log::error!("Failed to parse input JSON: {e}");
            return 1;
        }
    };

    // Decode transaction
    let response = decoder.decode(&request);

    // Write response to stdout
    match serde_json::to_string(&response) {
        Ok(output) => {
            if let Err(e) = io::stdout().write_all(output.as_bytes()) {
                log::error!("Failed to write output: {e}");
                return 1;
            }
        }
        Err(e) => {
            log::error!("Failed to serialize response: {e}");
            return 1;
        }
    }

    if response.success {
        0
    } else {
        1
    }
}

/// Alternative entry point for testing
#[no_mangle]
pub extern "C" fn _start() {
    std::process::exit(decode_tx());
}

// For non-WASI environments, provide a library interface
impl Default for WasiTxDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tx_decoder_creation() {
        let decoder = WasiTxDecoder::new();
        assert!(!decoder.registry.handlers.is_empty());
    }

    #[test]
    fn test_decode_base64_transaction() {
        let decoder = WasiTxDecoder::new();

        let request = DecodeRequest {
            tx_bytes: base64::encode("test transaction bytes"),
            encoding: "base64".to_string(),
            validate: true,
        };

        let response = decoder.decode(&request);
        assert!(response.success);
        assert!(response.decoded_tx.is_some());
    }

    #[test]
    fn test_decode_hex_transaction() {
        let decoder = WasiTxDecoder::new();

        let request = DecodeRequest {
            tx_bytes: hex::encode("test transaction bytes"),
            encoding: "hex".to_string(),
            validate: false,
        };

        let response = decoder.decode(&request);
        assert!(response.success);
        assert!(response.warnings.is_empty()); // No validation requested
    }

    #[test]
    fn test_invalid_encoding() {
        let decoder = WasiTxDecoder::new();

        let request = DecodeRequest {
            tx_bytes: "invalid".to_string(),
            encoding: "unknown".to_string(),
            validate: false,
        };

        let response = decoder.decode(&request);
        assert!(!response.success);
        assert!(response.error.is_some());
        assert!(response.error.unwrap().contains("Unsupported encoding"));
    }

    #[test]
    fn test_transaction_validation() {
        let decoder = WasiTxDecoder::new();

        let mut tx = DecodedTx {
            body: TxBody {
                messages: vec![],
                memo: "x".repeat(300), // Too long
                timeout_height: 0,
                extension_options: vec![],
                non_critical_extension_options: vec![],
            },
            auth_info: AuthInfo {
                signer_infos: vec![],
                fee: Fee {
                    amount: vec![],
                    gas_limit: 10000, // Too low
                    payer: String::new(),
                    granter: String::new(),
                },
                tip: None,
            },
            signatures: vec![],
            tx_hash: String::new(),
            size_bytes: 2_000_000, // Too large
        };

        let warnings = decoder.validate_transaction(&tx);
        assert!(warnings.len() >= 4); // Size, memo, gas, empty messages
    }
}
