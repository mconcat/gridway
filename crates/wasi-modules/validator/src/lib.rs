//! WASI Validator Component — TX validation pipeline via WASM/VFS
//!
//! Full validation: JSON decode → ed25519 verify → sequence check via kvstore → extract messages.
//! All state access goes through the kvstore WIT interface (VFS-backed).
//!
//! Signature format matches commonware-cryptography: union_unique(namespace, body)
//! = [varint(namespace_len)][namespace][body_bytes]

mod bindings {
    wit_bindgen::generate!({
        world: "validator-world",
        path: "../../../wit",
    });
}

use bindings::exports::gridway::framework::validator::{
    Event, EventAttribute, Guest, Message, TxContext, ValidatedTx, ValidationResult,
};
use bindings::gridway::framework::kvstore;

struct ValidatorComponent;

impl Guest for ValidatorComponent {
    fn validate(ctx: TxContext, raw_tx: Vec<u8>) -> ValidationResult {
        match validate_inner(&ctx, &raw_tx) {
            Ok((vtx, gas, events)) => ValidationResult {
                valid: true,
                tx: Some(vtx),
                error: None,
                gas_used: gas,
                events,
            },
            Err((msg, gas)) => ValidationResult {
                valid: false,
                tx: None,
                error: Some(msg),
                gas_used: gas,
                events: vec![],
            },
        }
    }
}

/// Namespace for transaction signing — matches gridway_crypto::TX_NAMESPACE.
const TX_NAMESPACE: &[u8] = b"gridway-tx";

fn validate_inner(
    ctx: &TxContext,
    raw_tx: &[u8],
) -> Result<(ValidatedTx, u64, Vec<Event>), (String, u64)> {
    // ── 1. Decode JSON envelope ──────────────────────────────────────────
    let decoded: serde_json::Value =
        serde_json::from_slice(raw_tx).map_err(|e| (format!("invalid JSON: {e}"), 1000))?;

    let public_key_hex = decoded
        .get("public_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ("missing public_key field".to_string(), 1000))?;

    let signature_hex = decoded
        .get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ("missing signature field".to_string(), 1000))?;

    let body = decoded
        .get("body")
        .ok_or_else(|| ("missing body field".to_string(), 1000))?;

    // ── 2. Verify ed25519 signature ──────────────────────────────────────
    use ed25519_dalek::{Verifier, VerifyingKey, Signature};

    let pk_bytes = hex::decode(public_key_hex)
        .map_err(|e| (format!("invalid public_key hex: {e}"), 2000))?;
    if pk_bytes.len() != 32 {
        return Err(("public key must be 32 bytes".to_string(), 2000));
    }
    let pk_array: [u8; 32] = pk_bytes.clone().try_into().unwrap();
    let verifying_key = VerifyingKey::from_bytes(&pk_array)
        .map_err(|e| (format!("invalid ed25519 public key: {e}"), 2000))?;

    let sig_bytes = hex::decode(signature_hex)
        .map_err(|e| (format!("invalid signature hex: {e}"), 2000))?;
    if sig_bytes.len() != 64 {
        return Err(("signature must be 64 bytes".to_string(), 2000));
    }
    let sig_array: [u8; 64] = sig_bytes.try_into().unwrap();
    let signature = Signature::from_bytes(&sig_array);

    // Derive address: sha256(pubkey)[0..20] hex (matches gridway_crypto::Address)
    let signer_address = derive_address(&pk_bytes);

    // Canonical body JSON for signature verification
    let body_bytes = serde_json::to_string(body)
        .map_err(|e| (format!("failed to serialize body: {e}"), 2000))?;

    // Build payload matching commonware-cryptography's union_unique(namespace, msg):
    //   [varint(namespace_len)][namespace][msg]
    let sign_payload = union_unique(TX_NAMESPACE, body_bytes.as_bytes());

    verifying_key
        .verify(&sign_payload, &signature)
        .map_err(|_| ("signature verification failed".to_string(), 3000))?;

    // ── 3. Sequence check via kvstore (auth namespace) ───────────────────
    let tx_sequence = body.get("sequence").and_then(|v| v.as_u64()).unwrap_or(0);

    let auth_store = kvstore::open_store("auth")
        .map_err(|e| (format!("failed to open auth store: {e}"), 4000))?;

    let account_key = format!("account_{signer_address}");
    let account_data = auth_store
        .get(account_key.as_bytes())
        .ok_or_else(|| (format!("account not found: {signer_address}"), 4000))?;

    let account_json = String::from_utf8(account_data)
        .map_err(|e| (format!("invalid account data: {e}"), 4000))?;

    #[derive(serde::Deserialize)]
    struct Account {
        public_key: String,
        sequence: u64,
    }
    let account: Account = serde_json::from_str(&account_json)
        .map_err(|e| (format!("invalid account JSON: {e}"), 4000))?;

    if tx_sequence != account.sequence {
        return Err((
            format!(
                "sequence mismatch: expected {}, got {tx_sequence}",
                account.sequence
            ),
            4000,
        ));
    }
    if account.public_key != public_key_hex {
        return Err((
            format!("public key mismatch for address {signer_address}"),
            4000,
        ));
    }

    // ── 4. Extract messages ──────────────────────────────────────────────
    let messages_array = body
        .get("messages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ("no messages in transaction".to_string(), 5000))?;

    let mut messages = Vec::with_capacity(messages_array.len());
    for (idx, msg_value) in messages_array.iter().enumerate() {
        let type_url = msg_value
            .get("@type")
            .and_then(|t| t.as_str())
            .ok_or_else(|| (format!("message {idx} missing @type"), 5000))?
            .to_string();

        // Verify from_address matches signer
        if let Some(from_addr) = msg_value.get("from_address").and_then(|v| v.as_str()) {
            if from_addr != signer_address {
                return Err((
                    format!(
                        "message {idx}: from_address '{from_addr}' doesn't match signer '{signer_address}'"
                    ),
                    5000,
                ));
            }
        }

        let data = serde_json::to_string(msg_value)
            .map_err(|e| (format!("failed to serialize message {idx}: {e}"), 5000))?;
        messages.push(Message { type_url, data });
    }

    let gas_used = 5000 + (messages.len() as u64 * 1000);

    Ok((
        ValidatedTx {
            sender: signer_address,
            messages,
            sequence: tx_sequence,
            gas_limit: 200_000,
        },
        gas_used,
        vec![Event {
            event_type: "tx_validated".to_string(),
            attributes: vec![EventAttribute {
                key: "height".to_string(),
                value: ctx.height.to_string(),
            }],
        }],
    ))
}

/// Derive address from ed25519 public key: sha256(pubkey)[0..20] hex.
/// Matches gridway_crypto::Address::from_public_key().
fn derive_address(pubkey_bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(pubkey_bytes);
    hex::encode(&hash[..20])
}

/// Replicate commonware-utils::union_unique.
/// Format: [varint(namespace_len)][namespace][msg]
fn union_unique(namespace: &[u8], msg: &[u8]) -> Vec<u8> {
    let len = namespace.len() as u32;
    let mut buf = Vec::with_capacity(5 + namespace.len() + msg.len());
    write_varint_u32(&mut buf, len);
    buf.extend_from_slice(namespace);
    buf.extend_from_slice(msg);
    buf
}

/// Write unsigned LEB128 varint (matching commonware-codec::varint::UInt).
fn write_varint_u32(buf: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

bindings::export!(ValidatorComponent with_types_in bindings);
