//! WASI BeginBlock Handler Component
//!
//! This module implements the BeginBlock ABCI handler as a WASI component using the
//! component model and WIT interfaces.

// Removed serde imports - now using WIT-generated types
use std::collections::HashMap;

// Include generated bindings
mod bindings;

use bindings::exports::gridway::framework::begin_blocker::{
    BeginBlockRequest, BeginBlockResponse, Event, EventAttribute, Evidence, Guest,
};
use bindings::gridway::framework::kvstore;

/// Validator information for updates (still needed for internal logic)
#[derive(Debug, Clone)]
pub struct Validator {
    pub address: Vec<u8>,
    pub power: i64,
    pub pub_key_type: String,
    pub pub_key_value: Vec<u8>,
}

/// Last commit information (still needed for internal logic)
#[derive(Debug, Clone)]
pub struct LastCommitInfo {
    pub round: i32,
    pub votes: Vec<VoteInfo>,
}

#[derive(Debug, Clone)]
pub struct VoteInfo {
    pub validator: Validator,
    pub signed_last_block: bool,
}

// Using WIT-generated types instead of local event structs

struct Component {
    /// Track missed blocks for downtime slashing
    #[allow(dead_code)]
    missed_blocks: HashMap<Vec<u8>, u32>,
}

impl Component {
    fn new() -> Self {
        Self {
            missed_blocks: HashMap::new(),
        }
    }
}

impl Guest for Component {
    fn begin_block(request: BeginBlockRequest) -> BeginBlockResponse {
        let _component = Component::new();

        // Open KVStore for begin-blocker
        let store = match kvstore::open_store("begin-blocker") {
            Ok(s) => s,
            Err(e) => {
                return BeginBlockResponse {
                    success: false,
                    events: vec![],
                    error: Some(format!("Failed to open kvstore: {e}")),
                }
            }
        };

        let mut events = Vec::new();

        // Process block header
        events.extend(process_block_header(&request, &store));

        // Process byzantine validators (evidence handling)
        events.extend(process_evidence(&request.byzantine_validators));

        // Additional block-level processing
        events.extend(process_epoch_transitions(request.height));

        BeginBlockResponse {
            success: true,
            events,
            error: None,
        }
    }
}

fn process_block_header(request: &BeginBlockRequest, store: &kvstore::Store) -> Vec<Event> {
    // Get proposer address from KVStore
    let proposer_address = store.get(b"proposer_address").unwrap_or_default();

    // Emit new block event
    vec![Event {
        event_type: "new_block".to_string(),
        attributes: vec![
            EventAttribute {
                key: "height".to_string(),
                value: request.height.to_string(),
            },
            EventAttribute {
                key: "time".to_string(),
                value: request.time.to_string(),
            },
            EventAttribute {
                key: "chain_id".to_string(),
                value: request.chain_id.clone(),
            },
            EventAttribute {
                key: "proposer".to_string(),
                value: hex::encode(&proposer_address),
            },
        ],
    }]
}

impl Component {
    #[allow(dead_code)]
    fn process_last_commit(&mut self, last_commit: &LastCommitInfo, height: u64) -> Vec<Event> {
        let mut events = vec![];
        let mut missed_validators = vec![];

        // Constants for downtime slashing
        const SLASH_WINDOW: u64 = 10000;
        const MIN_SIGNED_PER_WINDOW: f64 = 0.5;

        // Track validator participation
        for vote_info in &last_commit.votes {
            let validator_addr = vote_info.validator.address.clone();

            if !vote_info.signed_last_block {
                // Increment missed blocks counter
                let missed_count = self
                    .missed_blocks
                    .entry(validator_addr.clone())
                    .and_modify(|e| *e += 1)
                    .or_insert(1);

                // Check if validator should be slashed for downtime
                if *missed_count as f64 / SLASH_WINDOW as f64 > (1.0 - MIN_SIGNED_PER_WINDOW) {
                    missed_validators.push(hex::encode(&validator_addr));
                }
            } else {
                // Reset missed blocks counter on successful sign
                self.missed_blocks.remove(&validator_addr);
            }
        }

        // Emit downtime event if any validators missed too many blocks
        if !missed_validators.is_empty() {
            events.push(Event {
                event_type: "validator_downtime".to_string(),
                attributes: vec![
                    EventAttribute {
                        key: "height".to_string(),
                        value: height.to_string(),
                    },
                    EventAttribute {
                        key: "validators".to_string(),
                        value: missed_validators.join(","),
                    },
                ],
            });
        }

        // Emit participation statistics
        let total_validators = last_commit.votes.len();
        let signed_validators = last_commit
            .votes
            .iter()
            .filter(|v| v.signed_last_block)
            .count();

        events.push(Event {
            event_type: "block_participation".to_string(),
            attributes: vec![
                EventAttribute {
                    key: "total_validators".to_string(),
                    value: total_validators.to_string(),
                },
                EventAttribute {
                    key: "signed_validators".to_string(),
                    value: signed_validators.to_string(),
                },
                EventAttribute {
                    key: "participation_rate".to_string(),
                    value: if total_validators > 0 {
                        format!(
                            "{:.2}%",
                            (signed_validators as f64 / total_validators as f64) * 100.0
                        )
                    } else {
                        "0.00%".to_string()
                    },
                },
            ],
        });

        events
    }
}

fn process_evidence(evidence_list: &[Evidence]) -> Vec<Event> {
    let mut events = vec![];

    for evidence in evidence_list {
        events.push(Event {
            event_type: "evidence_submitted".to_string(),
            attributes: vec![
                EventAttribute {
                    key: "evidence_type".to_string(),
                    value: evidence.evidence_type.clone(),
                },
                EventAttribute {
                    key: "validator".to_string(),
                    value: hex::encode(&evidence.validator_address),
                },
                EventAttribute {
                    key: "height".to_string(),
                    value: evidence.height.to_string(),
                },
            ],
        });
    }

    events
}

fn process_epoch_transitions(height: u64) -> Vec<Event> {
    let mut events = vec![];

    // Check for daily epoch (every 86400 blocks assuming 1s blocks)
    if height.is_multiple_of(86400) {
        events.push(Event {
            event_type: "epoch_transition".to_string(),
            attributes: vec![
                EventAttribute {
                    key: "epoch_type".to_string(),
                    value: "daily".to_string(),
                },
                EventAttribute {
                    key: "height".to_string(),
                    value: height.to_string(),
                },
            ],
        });
    }

    // Check for weekly epoch
    if height.is_multiple_of(86400 * 7) {
        events.push(Event {
            event_type: "epoch_transition".to_string(),
            attributes: vec![
                EventAttribute {
                    key: "epoch_type".to_string(),
                    value: "weekly".to_string(),
                },
                EventAttribute {
                    key: "height".to_string(),
                    value: height.to_string(),
                },
            ],
        });
    }

    events
}

bindings::export!(Component with_types_in bindings);
