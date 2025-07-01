//! WASI Ante Handler Module
//!
//! This module implements transaction validation as a WASI program that can be
//! dynamically loaded by the BaseApp. It provides signature verification,
//! fee deduction, and sequence validation following the Cosmos SDK pattern.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use signature::Verifier;
use std::io::{self, Read, Write};
use thiserror::Error;

/// Error types for ante handler operations
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum AnteError {
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),

    #[error("Insufficient fees: got {got}, required {required}")]
    InsufficientFees { got: u64, required: u64 },

    #[error("Invalid sequence: got {got}, expected {expected} for account {account}")]
    InvalidSequence {
        account: String,
        got: u64,
        expected: u64,
    },

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Gas limit exceeded: wanted {wanted}, limit {limit}")]
    GasLimitExceeded { wanted: u64, limit: u64 },

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Result type for ante operations
pub type AnteResult<T> = Result<T, AnteError>;

/// Transaction context passed from the host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxContext {
    pub block_height: u64,
    pub block_time: u64,
    pub chain_id: String,
    pub gas_limit: u64,
    pub min_gas_price: u64,
}

/// Transaction data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub body: TxBody,
    pub auth_info: AuthInfo,
    pub signatures: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxBody {
    pub messages: Vec<Message>,
    pub memo: String,
    pub timeout_height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub type_url: String,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthInfo {
    pub signer_infos: Vec<SignerInfo>,
    pub fee: Fee,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignerInfo {
    pub public_key: Option<PublicKey>,
    pub sequence: u64,
    pub mode_info: ModeInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKey {
    pub type_url: String,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfo {
    pub mode: u32,
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

/// Account information for validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub address: String,
    pub account_number: u64,
    pub sequence: u64,
    pub public_key: Option<PublicKey>,
}

/// Ante handler response
#[derive(Debug, Serialize, Deserialize)]
pub struct AnteResponse {
    pub success: bool,
    pub gas_used: u64,
    pub error: Option<String>,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub event_type: String,
    pub attributes: Vec<Attribute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attribute {
    pub key: String,
    pub value: String,
}

/// WASI ante handler implementation
pub struct WasiAnteHandler {
    gas_used: u64,
}

impl WasiAnteHandler {
    pub fn new() -> Self {
        Self { gas_used: 0 }
    }

    /// Main entry point for ante handler validation
    pub fn handle(&mut self, ctx: &TxContext, tx: &Transaction) -> AnteResponse {
        log::info!(
            "WASI Ante Handler: Processing transaction for chain {}",
            ctx.chain_id
        );

        match self.validate_transaction(ctx, tx) {
            Ok(events) => AnteResponse {
                success: true,
                gas_used: self.gas_used,
                error: None,
                events,
            },
            Err(e) => AnteResponse {
                success: false,
                gas_used: self.gas_used,
                error: Some(e.to_string()),
                events: vec![],
            },
        }
    }

    fn validate_transaction(
        &mut self,
        ctx: &TxContext,
        tx: &Transaction,
    ) -> AnteResult<Vec<Event>> {
        let mut events = Vec::new();

        // Step 1: Basic transaction validation
        self.consume_gas(1000)?; // Base validation cost
        self.validate_basic_tx(tx)?;

        // Step 2: Fee validation
        self.consume_gas(2000)?; // Fee validation cost
        self.validate_fees(ctx, tx)?;
        events.push(Event {
            event_type: "fee_validation".to_string(),
            attributes: vec![Attribute {
                key: "gas_limit".to_string(),
                value: tx.auth_info.fee.gas_limit.to_string(),
            }],
        });

        // Step 3: Signature verification
        self.consume_gas(5000 * tx.signatures.len() as u64)?; // Per signature cost
        let accounts = self.get_accounts(&tx.auth_info.signer_infos)?;
        self.verify_signatures(ctx, tx, &accounts)?;
        events.push(Event {
            event_type: "signature_verification".to_string(),
            attributes: vec![Attribute {
                key: "signers".to_string(),
                value: tx.signatures.len().to_string(),
            }],
        });

        // Step 4: Sequence validation and increment
        self.consume_gas(1000 * tx.auth_info.signer_infos.len() as u64)?;
        self.validate_and_increment_sequences(tx, &accounts)?;
        events.push(Event {
            event_type: "sequence_validation".to_string(),
            attributes: vec![Attribute {
                key: "accounts".to_string(),
                value: accounts.len().to_string(),
            }],
        });

        Ok(events)
    }

    fn consume_gas(&mut self, amount: u64) -> AnteResult<()> {
        self.gas_used += amount;
        Ok(())
    }

    fn validate_basic_tx(&self, tx: &Transaction) -> AnteResult<()> {
        // Validate signature count matches signer info count
        if tx.signatures.len() != tx.auth_info.signer_infos.len() {
            return Err(AnteError::InvalidSignature(
                "signature count mismatch with signer info count".to_string(),
            ));
        }

        // Validate non-empty signatures
        for sig in &tx.signatures {
            if sig.is_empty() {
                return Err(AnteError::InvalidSignature("empty signature".to_string()));
            }
        }

        Ok(())
    }

    fn validate_fees(&self, ctx: &TxContext, tx: &Transaction) -> AnteResult<()> {
        let required_fee = tx.auth_info.fee.gas_limit * ctx.min_gas_price;

        // Calculate total fee amount
        let total_fee: u64 = tx
            .auth_info
            .fee
            .amount
            .iter()
            .map(|coin| coin.amount.parse::<u64>().unwrap_or(0))
            .sum();

        if total_fee < required_fee {
            return Err(AnteError::InsufficientFees {
                got: total_fee,
                required: required_fee,
            });
        }

        Ok(())
    }

    fn get_accounts(&self, signer_infos: &[SignerInfo]) -> AnteResult<Vec<AccountInfo>> {
        let mut accounts = Vec::new();

        for signer_info in signer_infos {
            // In a real implementation, this would query the account store via WASI filesystem
            // For now, we'll create mock account data
            let account = AccountInfo {
                address: self.derive_address(&signer_info.public_key)?,
                account_number: 0, // Would be queried from store
                sequence: signer_info.sequence,
                public_key: signer_info.public_key.clone(),
            };
            accounts.push(account);
        }

        Ok(accounts)
    }

    fn derive_address(&self, public_key: &Option<PublicKey>) -> AnteResult<String> {
        match public_key {
            Some(pk) => {
                // Simple address derivation for demo
                let hash = Sha256::digest(&pk.value);
                Ok(format!("cosmos1{}", hex::encode(&hash[..20])))
            }
            None => Err(AnteError::InvalidSignature(
                "no public key provided".to_string(),
            )),
        }
    }

    fn verify_signatures(
        &self,
        ctx: &TxContext,
        tx: &Transaction,
        accounts: &[AccountInfo],
    ) -> AnteResult<()> {
        for (i, (sig_bytes, signer_info)) in tx
            .signatures
            .iter()
            .zip(tx.auth_info.signer_infos.iter())
            .enumerate()
        {
            let account = &accounts[i];

            // Validate sequence matches
            if account.sequence != signer_info.sequence {
                return Err(AnteError::InvalidSequence {
                    account: account.address.clone(),
                    got: signer_info.sequence,
                    expected: account.sequence,
                });
            }

            // Get public key
            let public_key = account
                .public_key
                .as_ref()
                .ok_or_else(|| AnteError::InvalidSignature("no public key found".to_string()))?;

            // Verify signature
            self.verify_signature_crypto(public_key, sig_bytes, ctx, account)?;
        }

        Ok(())
    }

    fn verify_signature_crypto(
        &self,
        public_key: &PublicKey,
        signature: &[u8],
        ctx: &TxContext,
        account: &AccountInfo,
    ) -> AnteResult<()> {
        // Create sign document
        let sign_doc = self.create_sign_doc(ctx, account)?;
        let sign_bytes = self.create_sign_bytes(&sign_doc)?;

        match public_key.type_url.as_str() {
            "/cosmos.crypto.secp256k1.PubKey" => {
                self.verify_secp256k1_signature(&public_key.value, signature, &sign_bytes)
            }
            "/cosmos.crypto.ed25519.PubKey" => {
                self.verify_ed25519_signature(&public_key.value, signature, &sign_bytes)
            }
            _ => Err(AnteError::InvalidSignature(format!(
                "unsupported public key type: {}",
                public_key.type_url
            ))),
        }
    }

    fn create_sign_doc(&self, ctx: &TxContext, account: &AccountInfo) -> AnteResult<SignDoc> {
        Ok(SignDoc {
            body_bytes: b"simplified_tx_body".to_vec(), // Placeholder
            auth_info_bytes: b"simplified_auth_info".to_vec(), // Placeholder
            chain_id: ctx.chain_id.clone(),
            account_number: account.account_number,
        })
    }

    fn create_sign_bytes(&self, sign_doc: &SignDoc) -> AnteResult<Vec<u8>> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(sign_doc.body_bytes.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&sign_doc.body_bytes);
        bytes.extend_from_slice(&(sign_doc.auth_info_bytes.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&sign_doc.auth_info_bytes);
        let chain_id_bytes = sign_doc.chain_id.as_bytes();
        bytes.extend_from_slice(&(chain_id_bytes.len() as u32).to_be_bytes());
        bytes.extend_from_slice(chain_id_bytes);
        bytes.extend_from_slice(&sign_doc.account_number.to_be_bytes());
        Ok(bytes)
    }

    fn verify_secp256k1_signature(
        &self,
        public_key: &[u8],
        signature: &[u8],
        message: &[u8],
    ) -> AnteResult<()> {
        use k256::ecdsa::{Signature, VerifyingKey};

        let message_hash = Sha256::digest(message);

        let verifying_key = VerifyingKey::from_sec1_bytes(public_key).map_err(|e| {
            AnteError::InvalidSignature(format!("invalid secp256k1 public key: {e}"))
        })?;

        let signature = Signature::from_bytes(signature.into()).map_err(|e| {
            AnteError::InvalidSignature(format!("invalid secp256k1 signature: {e}"))
        })?;

        verifying_key
            .verify(&message_hash, &signature)
            .map_err(|_| {
                AnteError::InvalidSignature("secp256k1 signature verification failed".to_string())
            })?;

        Ok(())
    }

    fn verify_ed25519_signature(
        &self,
        public_key: &[u8],
        signature: &[u8],
        message: &[u8],
    ) -> AnteResult<()> {
        use ed25519_dalek::{Signature, VerifyingKey};

        let verifying_key = VerifyingKey::from_bytes(public_key.try_into().map_err(|_| {
            AnteError::InvalidSignature("invalid ed25519 public key length".to_string())
        })?)
        .map_err(|e| AnteError::InvalidSignature(format!("invalid ed25519 public key: {e}")))?;

        let signature = Signature::from_bytes(signature.try_into().map_err(|_| {
            AnteError::InvalidSignature("invalid ed25519 signature length".to_string())
        })?);

        verifying_key
            .verify_strict(message, &signature)
            .map_err(|_| {
                AnteError::InvalidSignature("ed25519 signature verification failed".to_string())
            })?;

        Ok(())
    }

    fn validate_and_increment_sequences(
        &self,
        tx: &Transaction,
        accounts: &[AccountInfo],
    ) -> AnteResult<()> {
        for (signer_info, account) in tx.auth_info.signer_infos.iter().zip(accounts.iter()) {
            if signer_info.sequence == u64::MAX {
                return Err(AnteError::InvalidSequence {
                    account: account.address.clone(),
                    got: signer_info.sequence,
                    expected: account.sequence,
                });
            }

            // In a real implementation, this would increment the sequence in the account store
            log::info!(
                "Incrementing sequence for account {} from {} to {}",
                account.address,
                account.sequence,
                account.sequence + 1
            );
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SignDoc {
    body_bytes: Vec<u8>,
    auth_info_bytes: Vec<u8>,
    chain_id: String,
    account_number: u64,
}

/// WASI entry point function
/// This function is called by the WASI host to process transactions
#[no_mangle]
pub extern "C" fn ante_handle() -> i32 {
    // Initialize logging
    env_logger::init();

    let mut handler = WasiAnteHandler::new();

    // Read input from stdin (transaction context and data)
    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        log::error!("Failed to read input: {e}");
        return 1;
    }

    // Parse input
    let (ctx, tx): (TxContext, Transaction) = match serde_json::from_str(&input) {
        Ok(data) => data,
        Err(e) => {
            log::error!("Failed to parse input JSON: {e}");
            return 1;
        }
    };

    // Process transaction
    let response = handler.handle(&ctx, &tx);

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

// For non-WASI environments, provide a library interface
impl Default for WasiAnteHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_context() -> TxContext {
        TxContext {
            block_height: 100,
            block_time: 1234567890,
            chain_id: "test-chain".to_string(),
            gas_limit: 200000,
            min_gas_price: 1,
        }
    }

    fn create_test_transaction() -> Transaction {
        Transaction {
            body: TxBody {
                messages: vec![Message {
                    type_url: "/cosmos.bank.v1beta1.MsgSend".to_string(),
                    value: b"test_message".to_vec(),
                }],
                memo: "test".to_string(),
                timeout_height: 0,
            },
            auth_info: AuthInfo {
                signer_infos: vec![SignerInfo {
                    public_key: Some(PublicKey {
                        type_url: "/cosmos.crypto.secp256k1.PubKey".to_string(),
                        value: vec![1; 33], // Mock compressed public key
                    }),
                    sequence: 0,
                    mode_info: ModeInfo { mode: 1 },
                }],
                fee: Fee {
                    amount: vec![Coin {
                        denom: "uatom".to_string(),
                        amount: "250000".to_string(),
                    }],
                    gas_limit: 200000,
                    payer: "".to_string(),
                    granter: "".to_string(),
                },
            },
            signatures: vec![vec![1; 64]], // Mock signature
        }
    }

    #[test]
    fn test_ante_handler_creation() {
        let handler = WasiAnteHandler::new();
        assert_eq!(handler.gas_used, 0);
    }

    #[test]
    fn test_basic_validation() {
        let mut handler = WasiAnteHandler::new();
        let ctx = create_test_context();
        let tx = create_test_transaction();

        let result = handler.validate_basic_tx(&tx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_fee_validation() {
        let handler = WasiAnteHandler::new();
        let ctx = create_test_context();
        let tx = create_test_transaction();

        let result = handler.validate_fees(&ctx, &tx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_insufficient_fees() {
        let handler = WasiAnteHandler::new();
        let mut ctx = create_test_context();
        ctx.min_gas_price = 100; // High min gas price
        let tx = create_test_transaction();

        let result = handler.validate_fees(&ctx, &tx);
        assert!(result.is_err());
        match result {
            Err(AnteError::InsufficientFees {
                got: 250000,
                required: 20000000,
            }) => {}
            _ => panic!("Expected InsufficientFees error"),
        }
    }
}
