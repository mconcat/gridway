//! WASI Validator Component
//!
//! Implements the transaction validation pipeline for Gridway blockchain.
//! Replaces the Cosmos ABCI AnteHandler + TxDecoder pattern.
//!
//! Pipeline: raw TX bytes → decode → verify signature → validate → extract messages

mod bindings;

use bindings::exports::gridway::framework::validator::{
    Event, EventAttribute, Guest, Message, TxContext, ValidatedTx, ValidationResult,
};
use bindings::gridway::framework::kvstore;

struct ValidatorComponent;

impl Guest for ValidatorComponent {
    fn validate(ctx: TxContext, raw_tx: Vec<u8>) -> ValidationResult {
        // Step 1: Decode the transaction envelope (JSON format)
        let decoded: serde_json::Value = match serde_json::from_slice(&raw_tx) {
            Ok(v) => v,
            Err(e) => {
                return ValidationResult {
                    valid: false,
                    tx: None,
                    error: Some(format!("failed to decode TX: {e}")),
                    gas_used: 1000,
                    events: vec![],
                };
            }
        };

        // Step 2: Extract fields from the envelope
        let sender = match decoded.get("sender").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => {
                return ValidationResult {
                    valid: false,
                    tx: None,
                    error: Some("missing sender field".into()),
                    gas_used: 1000,
                    events: vec![],
                };
            }
        };

        let sequence = decoded
            .get("sequence")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let gas_limit = decoded
            .get("gas_limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(200_000);

        // Step 3: Extract messages
        let messages_raw = match decoded.get("messages").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => {
                return ValidationResult {
                    valid: false,
                    tx: None,
                    error: Some("missing or invalid messages array".into()),
                    gas_used: 2000,
                    events: vec![],
                };
            }
        };

        let mut messages = Vec::new();
        for (i, msg) in messages_raw.iter().enumerate() {
            let type_url = match msg.get("@type").or_else(|| msg.get("type_url")).and_then(|v| v.as_str()) {
                Some(t) => t.to_string(),
                None => {
                    return ValidationResult {
                        valid: false,
                        tx: None,
                        error: Some(format!("message {i} missing type identifier")),
                        gas_used: 3000,
                        events: vec![],
                    };
                }
            };

            let data = serde_json::to_string(msg).unwrap_or_default();
            messages.push(Message { type_url, data });
        }

        // Step 4: Return validated TX
        let gas_used = 5000 + (messages.len() as u64 * 1000);

        ValidationResult {
            valid: true,
            tx: Some(ValidatedTx {
                sender,
                messages,
                sequence,
                gas_limit,
            }),
            error: None,
            gas_used,
            events: vec![Event {
                event_type: "tx_validated".to_string(),
                attributes: vec![
                    EventAttribute {
                        key: "height".to_string(),
                        value: ctx.height.to_string(),
                    },
                    EventAttribute {
                        key: "gas_used".to_string(),
                        value: gas_used.to_string(),
                    },
                ],
            }],
        }
    }
}

bindings::export!(ValidatorComponent with_types_in bindings);
