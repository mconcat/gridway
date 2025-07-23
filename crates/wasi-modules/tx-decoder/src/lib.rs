//! WASI Transaction Decoder Component
//!
//! This module implements transaction decoding as a WASI component using the
//! component model and WIT interfaces.

use serde::{Deserialize, Serialize};

// cargo-component generates bindings
mod bindings;

use bindings::exports::gridway::framework::tx_decoder::{DecodeRequest, DecodeResponse, Guest};

/// Decoded transaction structure (kept from old implementation)
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

struct Component;

impl Guest for Component {
    fn decode_tx(request: DecodeRequest) -> DecodeResponse {
        match decode_transaction_impl(&request) {
            Ok(decoded_json) => DecodeResponse {
                success: true,
                decoded_tx: Some(decoded_json),
                error: None,
                warnings: if request.validate {
                    validate_transaction(&request)
                } else {
                    vec![]
                },
            },
            Err(e) => DecodeResponse {
                success: false,
                decoded_tx: None,
                error: Some(e),
                warnings: vec![],
            },
        }
    }
}

fn decode_transaction_impl(req: &DecodeRequest) -> Result<String, String> {
    // Decode input based on encoding
    let raw_bytes = match req.encoding.as_str() {
        "raw" => req.tx_bytes.as_bytes().to_vec(),
        "base64" => {
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &req.tx_bytes)
                .map_err(|e| format!("Invalid base64:: {e}"))?
        }
        "hex" => hex::decode(&req.tx_bytes).map_err(|e| format!("Invalid hex:: {e}"))?,
        _ => return Err(format!("Unsupported encoding:: {}", req.encoding)),
    };

    // Calculate transaction hash
    let tx_hash = calculate_tx_hash(&raw_bytes);

    // For this example, we'll use a simplified decoding
    // In reality, this would use the actual Cosmos SDK protobuf definitions
    let decoded = decode_cosmos_tx(&raw_bytes)?;

    let result = DecodedTx {
        body: decoded.body,
        auth_info: decoded.auth_info,
        signatures: decoded.signatures,
        tx_hash,
        size_bytes: raw_bytes.len(),
    };

    // Serialize to JSON string
    serde_json::to_string(&result).map_err(|e| format!("Failed to serialize result:: {e}"))
}

fn decode_cosmos_tx(bytes: &[u8]) -> Result<DecodedTx, String> {
    // Simplified decoding - in reality would use proper protobuf
    // For now, create a mock decoded transaction

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

fn calculate_tx_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(bytes);
    hex::encode(hash).to_uppercase()
}

fn validate_transaction(req: &DecodeRequest) -> Vec<String> {
    let mut warnings = vec![];

    // Basic validation warnings
    if req.tx_bytes.is_empty() {
        warnings.push("Transaction bytes are empty".to_string());
    }

    // Add more validation as needed
    warnings
}

bindings::export!(Component with_types_in bindings);
