//! Gridway Client — TX builder library and HTTP client.
//!
//! Provides ergonomic transaction construction, signing, and submission
//! for the gridway blockchain.
//!
//! # Example
//!
//! ```rust,ignore
//! use gridway_client::{TxBuilder, Coin, GridwayClient};
//! use commonware_cryptography::Signer;
//! use commonware_cryptography::ed25519;
//!
//! # async fn example() -> Result<(), gridway_client::ClientError> {
//! let private_key = ed25519::PrivateKey::from_seed(42);
//! let tx = TxBuilder::new(private_key)
//!     .bank_send("deadbeef...", vec![Coin::new("ugridway", 1000)])
//!     .build()?;
//!
//! let client = GridwayClient::new("http://localhost:4547");
//! let result = client.submit_tx(&tx).await?;
//! println!("TX hash: {}", result.tx_hash);
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod keystore;

use commonware_codec::DecodeExt;
use commonware_cryptography::{ed25519, Signer};
use gridway_crypto::Address;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use client::GridwayClient;

// Re-export response types from client module
pub use client::{AccountResponse, BalanceResponse, StatusResponse, SubmitTxResponse};
pub use keystore::Keystore;

// ============================================================================
// Error types
// ============================================================================

/// Errors that can occur during TX building or client operations.
#[derive(Error, Debug)]
pub enum ClientError {
    #[error("no messages added to transaction")]
    NoMessages,

    #[error("invalid private key: {0}")]
    InvalidPrivateKey(String),

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("HTTP request error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("node returned error: {0}")]
    NodeError(String),

    #[error("invalid hex: {0}")]
    HexError(#[from] hex::FromHexError),

    #[error("unexpected response: {0}")]
    UnexpectedResponse(String),
}

// ============================================================================
// Coin
// ============================================================================

/// A coin with denomination and amount (amount as string for large numbers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coin {
    pub denom: String,
    pub amount: String,
}

impl Coin {
    /// Create a new Coin from a denomination and a u64 amount.
    pub fn new(denom: impl Into<String>, amount: u64) -> Self {
        Coin {
            denom: denom.into(),
            amount: amount.to_string(),
        }
    }

    /// Create a new Coin from a denomination and a string amount (for very large values).
    pub fn new_from_str(denom: impl Into<String>, amount: impl Into<String>) -> Self {
        Coin {
            denom: denom.into(),
            amount: amount.into(),
        }
    }
}

// ============================================================================
// TX Body & Signed TX types
// ============================================================================

/// Transaction body containing messages.
///
/// This matches the format expected by gridway-node.
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

/// A signed transaction ready for submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTx {
    pub body: TxBody,
    /// Hex-encoded 32-byte ed25519 public key
    pub public_key: String,
    /// Hex-encoded 64-byte ed25519 signature over canonical body JSON
    pub signature: String,
}

impl SignedTx {
    /// Serialize the signed transaction to canonical JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Serialize the signed transaction to pretty-printed JSON.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

// ============================================================================
// TxBuilder
// ============================================================================

/// Builder for constructing and signing gridway transactions.
///
/// # Example
///
/// ```rust,ignore
/// use gridway_client::{TxBuilder, Coin};
/// use commonware_cryptography::ed25519;
///
/// let private_key = ed25519::PrivateKey::from_seed(42);
/// let tx = TxBuilder::new(private_key)
///     .bank_send("deadbeef01234567890abcdef01234567890abcd", vec![Coin::new("ugridway", 1000)])
///     .build()
///     .unwrap();
///
/// let json = tx.to_json().unwrap();
/// assert!(json.contains("bank.MsgSend"));
/// ```
pub struct TxBuilder {
    private_key: ed25519::PrivateKey,
    messages: Vec<serde_json::Value>,
    chain_id: String,
    sequence: u64,
    memo: String,
}

impl TxBuilder {
    /// Create a new TxBuilder with the given private key.
    pub fn new(private_key: ed25519::PrivateKey) -> Self {
        Self {
            private_key,
            messages: Vec::new(),
            chain_id: "gridway-1".to_string(),
            sequence: 0,
            memo: String::new(),
        }
    }

    /// Create a new TxBuilder from a hex-encoded private key.
    pub fn from_hex_key(key_hex: &str) -> Result<Self, ClientError> {
        let key_bytes = hex::decode(key_hex)?;
        let private_key = ed25519::PrivateKey::decode(key_bytes.as_ref())
            .map_err(|e| ClientError::InvalidPrivateKey(format!("{:?}", e)))?;
        Ok(Self::new(private_key))
    }

    /// Set the chain ID (default: "gridway-1").
    pub fn chain_id(mut self, chain_id: impl Into<String>) -> Self {
        self.chain_id = chain_id.into();
        self
    }

    /// Set the sequence number for replay protection.
    pub fn sequence(mut self, seq: u64) -> Self {
        self.sequence = seq;
        self
    }

    /// Set a memo for the transaction.
    pub fn memo(mut self, memo: impl Into<String>) -> Self {
        self.memo = memo.into();
        self
    }

    /// Add a bank.MsgSend message to the transaction.
    ///
    /// The `from_address` is automatically derived from the private key.
    pub fn bank_send(mut self, to_address: impl Into<String>, amount: Vec<Coin>) -> Self {
        let from_address = self.sender_address();
        let msg = serde_json::json!({
            "@type": "bank.MsgSend",
            "from_address": from_address,
            "to_address": to_address.into(),
            "amount": amount,
        });
        self.messages.push(msg);
        self
    }

    /// Add a raw message (arbitrary JSON value) to the transaction.
    pub fn raw_message(mut self, msg: serde_json::Value) -> Self {
        self.messages.push(msg);
        self
    }

    /// Get the sender address derived from the private key.
    pub fn sender_address(&self) -> String {
        let pk = self.private_key.public_key();
        Address::from_public_key(&pk).to_hex()
    }

    /// Get the hex-encoded public key.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.private_key.public_key().as_ref())
    }

    /// Build and sign the transaction.
    pub fn build(self) -> Result<SignedTx, ClientError> {
        if self.messages.is_empty() {
            return Err(ClientError::NoMessages);
        }

        let body = TxBody {
            messages: self.messages,
            chain_id: self.chain_id,
            sequence: self.sequence,
            memo: self.memo,
        };

        // Canonical JSON serialization for signing.
        // Convert to Value first to ensure consistent field ordering (alphabetical
        // via BTreeMap), matching what the WASM validator produces when it
        // re-serializes the parsed body for signature verification.
        let body_value = serde_json::to_value(&body)?;
        let canonical_body = serde_json::to_string(&body_value)?;

        // Sign the canonical body bytes
        let signature = gridway_crypto::sign_tx_body(&self.private_key, canonical_body.as_bytes());
        let public_key = self.private_key.public_key();

        Ok(SignedTx {
            body,
            public_key: hex::encode(public_key.as_ref()),
            signature: hex::encode(signature.as_ref()),
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use commonware_codec::Encode;
    use commonware_cryptography::ed25519;

    #[test]
    fn test_coin_new() {
        let coin = Coin::new("ugridway", 1000);
        assert_eq!(coin.denom, "ugridway");
        assert_eq!(coin.amount, "1000");
    }

    #[test]
    fn test_coin_new_from_str() {
        let coin = Coin::new_from_str("ugridway", "999999999999999999");
        assert_eq!(coin.denom, "ugridway");
        assert_eq!(coin.amount, "999999999999999999");
    }

    #[test]
    fn test_coin_serialization() {
        let coin = Coin::new("ugridway", 1000);
        let json = serde_json::to_string(&coin).unwrap();
        assert!(json.contains("\"denom\":\"ugridway\""));
        assert!(json.contains("\"amount\":\"1000\""));

        // Amount must be a string in JSON
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["amount"].is_string());
    }

    #[test]
    fn test_tx_builder_bank_send() {
        let private_key = ed25519::PrivateKey::from_seed(42);
        let to_address = "deadbeef01234567890abcdef01234567890abcd";

        let tx = TxBuilder::new(private_key)
            .bank_send(to_address, vec![Coin::new("ugridway", 1000)])
            .build()
            .unwrap();

        // Verify JSON structure
        let json: serde_json::Value = serde_json::from_str(&tx.to_json().unwrap()).unwrap();
        assert!(json["body"]["messages"][0]["@type"] == "bank.MsgSend");
        assert!(json["body"]["messages"][0]["to_address"] == to_address);
        assert!(json["body"]["messages"][0]["amount"][0]["denom"] == "ugridway");
        assert!(json["body"]["messages"][0]["amount"][0]["amount"] == "1000");
        assert!(json["body"]["chain_id"] == "gridway-1");

        // Verify public key and signature are hex strings
        assert!(!tx.public_key.is_empty());
        assert!(!tx.signature.is_empty());
        assert!(hex::decode(&tx.public_key).is_ok());
        assert!(hex::decode(&tx.signature).is_ok());
    }

    #[test]
    fn test_tx_builder_from_address_auto_derived() {
        let private_key = ed25519::PrivateKey::from_seed(42);
        let expected_address = {
            let pk = private_key.public_key();
            Address::from_public_key(&pk).to_hex()
        };

        let tx = TxBuilder::new(private_key)
            .bank_send("recipient", vec![Coin::new("ugridway", 500)])
            .build()
            .unwrap();

        let json: serde_json::Value = serde_json::from_str(&tx.to_json().unwrap()).unwrap();
        assert_eq!(
            json["body"]["messages"][0]["from_address"]
                .as_str()
                .unwrap(),
            expected_address
        );
    }

    #[test]
    fn test_tx_builder_signature_verification() {
        use commonware_codec::DecodeExt;

        let private_key = ed25519::PrivateKey::from_seed(42);
        let public_key = private_key.public_key();

        let tx = TxBuilder::new(private_key)
            .bank_send("recipient", vec![Coin::new("ugridway", 100)])
            .build()
            .unwrap();

        // Re-serialize body canonically (via Value for consistent field ordering)
        let body_value = serde_json::to_value(&tx.body).unwrap();
        let canonical_body = serde_json::to_string(&body_value).unwrap();
        let sig_bytes = hex::decode(&tx.signature).unwrap();
        let signature = ed25519::Signature::decode(sig_bytes.as_ref()).unwrap();

        assert!(gridway_crypto::verify_tx_body(
            &public_key,
            canonical_body.as_bytes(),
            &signature
        ));
    }

    #[test]
    fn test_tx_builder_custom_chain_id() {
        let private_key = ed25519::PrivateKey::from_seed(1);
        let tx = TxBuilder::new(private_key)
            .chain_id("gridway-testnet")
            .bank_send("recipient", vec![Coin::new("ugridway", 1)])
            .build()
            .unwrap();

        assert_eq!(tx.body.chain_id, "gridway-testnet");
    }

    #[test]
    fn test_tx_builder_sequence_and_memo() {
        let private_key = ed25519::PrivateKey::from_seed(1);
        let tx = TxBuilder::new(private_key)
            .sequence(5)
            .memo("test memo")
            .bank_send("recipient", vec![Coin::new("ugridway", 1)])
            .build()
            .unwrap();

        assert_eq!(tx.body.sequence, 5);
        assert_eq!(tx.body.memo, "test memo");
    }

    #[test]
    fn test_tx_builder_no_messages_error() {
        let private_key = ed25519::PrivateKey::from_seed(1);
        let result = TxBuilder::new(private_key).build();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ClientError::NoMessages));
    }

    #[test]
    fn test_tx_builder_from_hex_key() {
        let private_key = ed25519::PrivateKey::from_seed(42);
        let key_hex = hex::encode(private_key.encode());

        let builder = TxBuilder::from_hex_key(&key_hex).unwrap();
        let tx = builder
            .bank_send("recipient", vec![Coin::new("ugridway", 100)])
            .build()
            .unwrap();

        assert!(!tx.signature.is_empty());
    }

    #[test]
    fn test_tx_builder_from_hex_key_invalid() {
        let result = TxBuilder::from_hex_key("not_valid_hex");
        assert!(result.is_err());
    }

    #[test]
    fn test_signed_tx_to_json() {
        let private_key = ed25519::PrivateKey::from_seed(42);
        let tx = TxBuilder::new(private_key)
            .bank_send("recipient", vec![Coin::new("ugridway", 100)])
            .build()
            .unwrap();

        let json = tx.to_json().unwrap();

        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["body"].is_object());
        assert!(parsed["public_key"].is_string());
        assert!(parsed["signature"].is_string());
    }

    #[test]
    fn test_signed_tx_roundtrip() {
        let private_key = ed25519::PrivateKey::from_seed(42);
        let tx = TxBuilder::new(private_key)
            .bank_send("recipient", vec![Coin::new("ugridway", 100)])
            .build()
            .unwrap();

        let json = tx.to_json().unwrap();
        let deserialized: SignedTx = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.public_key, tx.public_key);
        assert_eq!(deserialized.signature, tx.signature);
        assert_eq!(deserialized.body.chain_id, tx.body.chain_id);
    }

    #[test]
    fn test_client_response_parsing_submit_tx() {
        let json = r#"{"status":"submitted","tx_hash":"abc123"}"#;
        let resp: SubmitTxResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, "submitted");
        assert_eq!(resp.tx_hash, "abc123");
    }

    #[test]
    fn test_client_response_parsing_balance() {
        let json = r#"{"address":"deadbeef","denom":"ugridway","balance":5000}"#;
        let resp: BalanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.address, "deadbeef");
        assert_eq!(resp.denom, "ugridway");
        assert_eq!(resp.balance, 5000);
    }

    #[test]
    fn test_client_response_parsing_account() {
        let json = r#"{"address":"deadbeef","public_key":"aabbcc","sequence":3}"#;
        let resp: AccountResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.address, "deadbeef");
        assert_eq!(resp.public_key, "aabbcc");
        assert_eq!(resp.sequence, 3);
    }

    #[test]
    fn test_client_response_parsing_status() {
        let json = r#"{"chain_id":"gridway-1","state_root":"000000","pending_tx_count":2}"#;
        let resp: StatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.chain_id, "gridway-1");
        assert_eq!(resp.state_root, "000000");
        assert_eq!(resp.pending_tx_count, 2);
    }

    #[test]
    fn test_multiple_coins() {
        let private_key = ed25519::PrivateKey::from_seed(42);
        let tx = TxBuilder::new(private_key)
            .bank_send(
                "recipient",
                vec![Coin::new("ugridway", 1000), Coin::new("uatom", 500)],
            )
            .build()
            .unwrap();

        let json: serde_json::Value = serde_json::from_str(&tx.to_json().unwrap()).unwrap();
        let amounts = &json["body"]["messages"][0]["amount"];
        assert_eq!(amounts.as_array().unwrap().len(), 2);
    }
}
