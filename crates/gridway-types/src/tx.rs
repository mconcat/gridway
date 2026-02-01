//! Transaction types for gridway.
//!
//! Replaces Cosmos SDK protobuf-based transaction types with simpler
//! JSON-serializable types. The WASM modules still process transactions
//! through the same pipeline, just without protobuf encoding.

use serde::{Deserialize, Serialize};

/// A raw transaction as received from clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTx {
    pub body: TxBody,
    pub auth_info: AuthInfo,
    pub signatures: Vec<Vec<u8>>,
}

/// Transaction body containing messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxBody {
    pub messages: Vec<TxMessage>,
    pub memo: String,
    pub timeout_height: u64,
}

/// A message within a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxMessage {
    pub type_url: String,
    pub value: Vec<u8>,
}

/// Authentication info for a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthInfo {
    pub signer_infos: Vec<SignerInfo>,
    pub fee: Fee,
}

/// Signer information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignerInfo {
    pub public_key: Option<TxMessage>,
    pub mode_info: ModeInfo,
    pub sequence: u64,
}

/// Signing mode info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfo {
    pub single: Option<ModeInfoSingle>,
}

/// Single signing mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfoSingle {
    pub mode: u32,
}

/// Transaction fee
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fee {
    pub amount: Vec<FeeAmount>,
    pub gas_limit: u64,
    pub payer: String,
    pub granter: String,
}

/// Fee amount in a specific denomination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeAmount {
    pub denom: String,
    pub amount: String,
}

/// Bank send message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsgSend {
    pub from_address: String,
    pub to_address: String,
    pub amount: Vec<Coin>,
}

/// A coin amount
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coin {
    pub denom: String,
    pub amount: String,
}

/// Transaction response from execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxResponse {
    pub code: u32,
    pub data: Vec<u8>,
    pub log: String,
    pub info: String,
    pub gas_wanted: i64,
    pub gas_used: i64,
    pub events: Vec<crate::Event>,
    pub codespace: String,
}

impl TxResponse {
    /// Create a success response
    pub fn success(
        log: String,
        gas_wanted: i64,
        gas_used: i64,
        events: Vec<crate::Event>,
    ) -> Self {
        Self {
            code: 0,
            data: vec![],
            log,
            info: String::new(),
            gas_wanted,
            gas_used,
            events,
            codespace: String::new(),
        }
    }

    /// Create a failure response
    pub fn failure(code: u32, log: String, gas_wanted: i64, gas_used: i64) -> Self {
        Self {
            code,
            data: vec![],
            log,
            info: String::new(),
            gas_wanted,
            gas_used,
            events: vec![],
            codespace: String::new(),
        }
    }
}

/// Gridway transaction — the decoded form used internally
pub type GridwayTx = RawTx;

/// Trait for SDK messages (kept for compatibility with module router)
pub trait SdkMsg: Send + Sync {
    fn type_url(&self) -> &str;
    fn validate_basic(&self) -> std::result::Result<(), String>;
    fn encode(&self) -> Vec<u8> { vec![] }
    fn as_any(&self) -> &dyn std::any::Any where Self: Sized + 'static { self }
}
