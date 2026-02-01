//! Transaction types for gridway.
//!
//! Uses a simple signed TX format with ed25519 signatures.
//! The signature covers the canonical JSON bytes of the body.

use serde::{Deserialize, Serialize};

/// A signed transaction as submitted by clients.
///
/// The `signature` is computed over the canonical JSON serialization of `body`
/// using the gridway-tx namespace with ed25519.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTx {
    pub body: TxBody,
    /// Hex-encoded 32-byte ed25519 public key
    pub public_key: String,
    /// Hex-encoded 64-byte ed25519 signature over body JSON
    pub signature: String,
}

/// Transaction body containing messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxBody {
    pub messages: Vec<serde_json::Value>,
    #[serde(default = "default_chain_id")]
    pub chain_id: String,
    #[serde(default)]
    pub sequence: u64,
    #[serde(default)]
    pub memo: String,
}

fn default_chain_id() -> String {
    "gridway-1".to_string()
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
pub type GridwayTx = SignedTx;

/// Trait for SDK messages (kept for compatibility with module router)
pub trait SdkMsg: Send + Sync {
    fn type_url(&self) -> &str;
    fn validate_basic(&self) -> std::result::Result<(), String>;
    fn encode(&self) -> Vec<u8> { vec![] }
    fn as_any(&self) -> &dyn std::any::Any where Self: Sized + 'static { self }
}

// === Legacy types kept for backward compatibility with module_router ===

/// A raw transaction (legacy format, kept for module router compat)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTx {
    pub body: LegacyTxBody,
    pub auth_info: AuthInfo,
    pub signatures: Vec<Vec<u8>>,
}

/// Legacy transaction body
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyTxBody {
    pub messages: Vec<TxMessage>,
    pub memo: String,
    pub timeout_height: u64,
}

/// A message within a transaction (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxMessage {
    pub type_url: String,
    pub value: Vec<u8>,
}

/// Authentication info for a transaction (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthInfo {
    pub signer_infos: Vec<SignerInfo>,
    pub fee: Fee,
}

/// Signer information (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignerInfo {
    pub public_key: Option<TxMessage>,
    pub mode_info: ModeInfo,
    pub sequence: u64,
}

/// Signing mode info (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfo {
    pub single: Option<ModeInfoSingle>,
}

/// Single signing mode (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfoSingle {
    pub mode: u32,
}

/// Transaction fee (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fee {
    pub amount: Vec<FeeAmount>,
    pub gas_limit: u64,
    pub payer: String,
    pub granter: String,
}

/// Fee amount in a specific denomination (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeAmount {
    pub denom: String,
    pub amount: String,
}
