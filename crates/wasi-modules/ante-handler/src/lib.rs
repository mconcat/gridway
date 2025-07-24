//! WASI Ante Handler Component
//!
//! This module implements transaction validation as a WASI component using the
//! component model and WIT interfaces.

// Removed serde imports - now using WIT-generated types
use sha2::{Digest, Sha256};
use signature::Verifier;

// Include generated bindings
mod bindings;

use bindings::exports::gridway::framework::ante_handler::{
    AnteResponse, Event, EventAttribute, Guest, TxContext,
};

/// Error types for ante handler operations
#[derive(Debug)]
pub struct AnteError {
    pub error_type: String,
    pub message: String,
}

/// Transaction data structure
#[derive(Debug, Clone)]
pub struct Transaction {
    pub body: TxBody,
    pub auth_info: AuthInfo,
    pub signatures: Vec<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct TxBody {
    pub messages: Vec<Message>,
    pub memo: String,
    pub timeout_height: u64,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub type_url: String,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct AuthInfo {
    pub signer_infos: Vec<SignerInfo>,
    pub fee: Fee,
}

#[derive(Debug, Clone)]
pub struct SignerInfo {
    pub public_key: Option<PublicKey>,
    pub sequence: u64,
    pub mode_info: ModeInfo,
}

#[derive(Debug, Clone)]
pub struct PublicKey {
    pub type_url: String,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ModeInfo {
    pub mode: u32,
}

#[derive(Debug, Clone)]
pub struct Fee {
    pub amount: Vec<Coin>,
    pub gas_limit: u64,
    pub payer: String,
    pub granter: String,
}

#[derive(Debug, Clone)]
pub struct Coin {
    pub denom: String,
    pub amount: String,
}

/// Account information for validation
#[derive(Debug, Clone)]
pub struct AccountInfo {
    pub address: String,
    pub account_number: u64,
    pub sequence: u64,
    pub public_key: Option<PublicKey>,
}

// Using WIT-generated Event and EventAttribute types instead

struct Component;

impl Guest for Component {
    fn ante_handle(context: TxContext, tx_bytes: Vec<u8>) -> AnteResponse {
        // For demo purposes, assume a min gas price of 1
        let min_gas_price = 1u64;

        // Parse transaction from bytes (in a real implementation, this would be protobuf)
        // For now, we'll create a mock transaction
        let tx = create_mock_transaction(&tx_bytes);

        let mut gas_used = 0u64;
        let mut events = Vec::new();

        // Step 1: Basic transaction validation
        gas_used += 1000; // Base validation cost
        if let Err(e) = validate_basic_tx(&tx) {
            return AnteResponse {
                success: false,
                gas_used,
                error: Some(e.message),
                events: vec![],
                priority: 0,
            };
        }

        // Step 2: Fee validation
        gas_used += 2000; // Fee validation cost
        if let Err(e) = validate_fees(min_gas_price, &tx) {
            return AnteResponse {
                success: false,
                gas_used,
                error: Some(e.message),
                events: vec![],
                priority: 0,
            };
        }

        events.push(Event {
            event_type: "fee_validation".to_string(),
            attributes: vec![EventAttribute {
                key: "gas_limit".to_string(),
                value: tx.auth_info.fee.gas_limit.to_string(),
            }],
        });

        // Step 3: Signature verification
        gas_used += 5000 * tx.signatures.len() as u64; // Per signature cost
        let accounts = match get_accounts(&tx.auth_info.signer_infos) {
            Ok(accounts) => accounts,
            Err(e) => {
                return AnteResponse {
                    success: false,
                    gas_used,
                    error: Some(e.message),
                    events: vec![],
                    priority: 0,
                };
            }
        };

        if let Err(e) = verify_signatures(&context, &tx, &accounts) {
            return AnteResponse {
                success: false,
                gas_used,
                error: Some(e.message),
                events: vec![],
                priority: 0,
            };
        }

        events.push(Event {
            event_type: "signature_verification".to_string(),
            attributes: vec![EventAttribute {
                key: "signers".to_string(),
                value: tx.signatures.len().to_string(),
            }],
        });

        // Step 4: Sequence validation
        gas_used += 1000 * tx.auth_info.signer_infos.len() as u64;
        if let Err(e) = validate_sequences(&tx, &accounts) {
            return AnteResponse {
                success: false,
                gas_used,
                error: Some(e.message),
                events: vec![],
                priority: 0,
            };
        }

        events.push(Event {
            event_type: "sequence_validation".to_string(),
            attributes: vec![EventAttribute {
                key: "accounts".to_string(),
                value: accounts.len().to_string(),
            }],
        });

        // Calculate priority based on gas price
        let priority = calculate_priority(&tx);

        AnteResponse {
            success: true,
            gas_used,
            error: None,
            events,
            priority,
        }
    }
}

fn create_mock_transaction(_tx_bytes: &[u8]) -> Transaction {
    // In a real implementation, this would deserialize from protobuf
    // For now, create a mock transaction
    Transaction {
        body: TxBody {
            messages: vec![Message {
                type_url: "/cosmos.bank.v1beta1.MsgSend".to_string(),
                value: b"mock_message".to_vec(),
            }],
            memo: "mock transaction".to_string(),
            timeout_height: 0,
        },
        auth_info: AuthInfo {
            signer_infos: vec![SignerInfo {
                public_key: Some(PublicKey {
                    type_url: "/cosmos.crypto.secp256k1.PubKey".to_string(),
                    value: vec![0; 33], // Mock compressed public key
                }),
                sequence: 0,
                mode_info: ModeInfo { mode: 1 },
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
        },
        signatures: vec![vec![0; 64]], // Mock signature
    }
}

fn validate_basic_tx(tx: &Transaction) -> Result<(), AnteError> {
    // Validate signature count matches signer info count
    if tx.signatures.len() != tx.auth_info.signer_infos.len() {
        return Err(AnteError {
            error_type: "InvalidSignature".to_string(),
            message: "signature count mismatch with signer info count".to_string(),
        });
    }

    // Validate non-empty signatures
    for sig in &tx.signatures {
        if sig.is_empty() {
            return Err(AnteError {
                error_type: "InvalidSignature".to_string(),
                message: "empty signature".to_string(),
            });
        }
    }

    Ok(())
}

fn validate_fees(min_gas_price: u64, tx: &Transaction) -> Result<(), AnteError> {
    let required_fee = tx.auth_info.fee.gas_limit * min_gas_price;

    // Calculate total fee amount
    let total_fee: u64 = tx
        .auth_info
        .fee
        .amount
        .iter()
        .map(|coin| coin.amount.parse::<u64>().unwrap_or(0))
        .sum();

    if total_fee < required_fee {
        return Err(AnteError {
            error_type: "InsufficientFees".to_string(),
            message: format!("got {total_fee}, required {required_fee}"),
        });
    }

    Ok(())
}

fn get_accounts(signer_infos: &[SignerInfo]) -> Result<Vec<AccountInfo>, AnteError> {
    let mut accounts = Vec::new();

    for signer_info in signer_infos {
        // In a real implementation, this would query the account store
        // For now, we'll create mock account data
        let account = AccountInfo {
            address: derive_address(&signer_info.public_key)?,
            account_number: 0, // Would be queried from store
            sequence: signer_info.sequence,
            public_key: signer_info.public_key.clone(),
        };
        accounts.push(account);
    }

    Ok(accounts)
}

fn derive_address(public_key: &Option<PublicKey>) -> Result<String, AnteError> {
    match public_key {
        Some(pk) => {
            // Simple address derivation for demo
            let hash = Sha256::digest(&pk.value);
            Ok(format!("cosmos1{}", hex::encode(&hash[..20])))
        }
        None => Err(AnteError {
            error_type: "InvalidSignature".to_string(),
            message: "no public key provided".to_string(),
        }),
    }
}

fn verify_signatures(
    context: &TxContext,
    tx: &Transaction,
    accounts: &[AccountInfo],
) -> Result<(), AnteError> {
    for (i, (sig_bytes, signer_info)) in tx
        .signatures
        .iter()
        .zip(tx.auth_info.signer_infos.iter())
        .enumerate()
    {
        let account = &accounts[i];

        // Validate sequence matches
        if account.sequence != signer_info.sequence {
            return Err(AnteError {
                error_type: "InvalidSequence".to_string(),
                message: format!(
                    "got {}, expected {} for account {}",
                    signer_info.sequence, account.sequence, account.address
                ),
            });
        }

        // Get public key
        let public_key = account.public_key.as_ref().ok_or_else(|| AnteError {
            error_type: "InvalidSignature".to_string(),
            message: "no public key found".to_string(),
        })?;

        // Verify signature
        verify_signature_crypto(public_key, sig_bytes, context, account)?;
    }

    Ok(())
}

fn verify_signature_crypto(
    public_key: &PublicKey,
    signature: &[u8],
    context: &TxContext,
    account: &AccountInfo,
) -> Result<(), AnteError> {
    // Create sign document
    let sign_doc = create_sign_doc(context, account);
    let sign_bytes = create_sign_bytes(&sign_doc);

    match public_key.type_url.as_str() {
        "/cosmos.crypto.secp256k1.PubKey" => {
            verify_secp256k1_signature(&public_key.value, signature, &sign_bytes)
        }
        "/cosmos.crypto.ed25519.PubKey" => {
            verify_ed25519_signature(&public_key.value, signature, &sign_bytes)
        }
        _ => Err(AnteError {
            error_type: "InvalidSignature".to_string(),
            message: format!("unsupported public key type:: {}", public_key.type_url),
        }),
    }
}

#[derive(Debug)]
struct SignDoc {
    body_bytes: Vec<u8>,
    auth_info_bytes: Vec<u8>,
    chain_id: String,
    account_number: u64,
}

fn create_sign_doc(context: &TxContext, account: &AccountInfo) -> SignDoc {
    SignDoc {
        body_bytes: b"simplified_tx_body".to_vec(), // Placeholder
        auth_info_bytes: b"simplified_auth_info".to_vec(), // Placeholder
        chain_id: context.chain_id.clone(),
        account_number: account.account_number,
    }
}

fn create_sign_bytes(sign_doc: &SignDoc) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(sign_doc.body_bytes.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&sign_doc.body_bytes);
    bytes.extend_from_slice(&(sign_doc.auth_info_bytes.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&sign_doc.auth_info_bytes);
    let chain_id_bytes = sign_doc.chain_id.as_bytes();
    bytes.extend_from_slice(&(chain_id_bytes.len() as u32).to_be_bytes());
    bytes.extend_from_slice(chain_id_bytes);
    bytes.extend_from_slice(&sign_doc.account_number.to_be_bytes());
    bytes
}

fn verify_secp256k1_signature(
    public_key: &[u8],
    signature: &[u8],
    message: &[u8],
) -> Result<(), AnteError> {
    use k256::ecdsa::{Signature, VerifyingKey};

    let message_hash = Sha256::digest(message);

    let verifying_key = VerifyingKey::from_sec1_bytes(public_key).map_err(|e| AnteError {
        error_type: "InvalidSignature".to_string(),
        message: format!("invalid secp256k1 public key:: {e}"),
    })?;

    let signature = Signature::from_bytes(signature.into()).map_err(|e| AnteError {
        error_type: "InvalidSignature".to_string(),
        message: format!("invalid secp256k1 signature:: {e}"),
    })?;

    verifying_key
        .verify(&message_hash, &signature)
        .map_err(|_| AnteError {
            error_type: "InvalidSignature".to_string(),
            message: "secp256k1 signature verification failed".to_string(),
        })?;

    Ok(())
}

fn verify_ed25519_signature(
    public_key: &[u8],
    signature: &[u8],
    message: &[u8],
) -> Result<(), AnteError> {
    use ed25519_dalek::{Signature, VerifyingKey};

    let verifying_key = VerifyingKey::from_bytes(public_key.try_into().map_err(|_| AnteError {
        error_type: "InvalidSignature".to_string(),
        message: "invalid ed25519 public key length".to_string(),
    })?)
    .map_err(|e| AnteError {
        error_type: "InvalidSignature".to_string(),
        message: format!("invalid ed25519 public key:: {e}"),
    })?;

    let signature = Signature::from_bytes(signature.try_into().map_err(|_| AnteError {
        error_type: "InvalidSignature".to_string(),
        message: "invalid ed25519 signature length".to_string(),
    })?);

    verifying_key
        .verify_strict(message, &signature)
        .map_err(|_| AnteError {
            error_type: "InvalidSignature".to_string(),
            message: "ed25519 signature verification failed".to_string(),
        })?;

    Ok(())
}

fn validate_sequences(tx: &Transaction, accounts: &[AccountInfo]) -> Result<(), AnteError> {
    for (signer_info, account) in tx.auth_info.signer_infos.iter().zip(accounts.iter()) {
        if signer_info.sequence == u64::MAX {
            return Err(AnteError {
                error_type: "InvalidSequence".to_string(),
                message: format!(
                    "got {}, expected {} for account {}",
                    signer_info.sequence, account.sequence, account.address
                ),
            });
        }
    }

    Ok(())
}

fn calculate_priority(tx: &Transaction) -> u64 {
    // Simple priority calculation based on fee amount
    tx.auth_info
        .fee
        .amount
        .iter()
        .map(|coin| coin.amount.parse::<u64>().unwrap_or(0))
        .sum()
}

bindings::export!(Component with_types_in bindings);
