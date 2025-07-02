//! WASI BeginBlock Handler Module
//!
//! This module implements the BeginBlock ABCI handler as a WASI program that can be
//! dynamically loaded by the BaseApp. It processes block header information,
//! manages validator updates, and emits block-level events.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use thiserror::Error;

/// Error types for begin block operations
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum BeginBlockError {
    #[error("Invalid block height: current {current}, expected {expected}")]
    InvalidHeight { current: u64, expected: u64 },

    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(String),

    #[error("Validator set error: {0}")]
    ValidatorError(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Result type for begin block operations
pub type BeginBlockResult<T> = Result<T, BeginBlockError>;

/// Block context passed from the host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockContext {
    pub height: u64,
    pub time: u64,
    pub chain_id: String,
    pub proposer_address: Vec<u8>,
    pub last_block_hash: Vec<u8>,
    pub app_hash: Vec<u8>,
}

/// Validator information for updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Validator {
    pub address: Vec<u8>,
    pub power: i64,
    pub pub_key_type: String,
    pub pub_key_value: Vec<u8>,
}

/// Evidence of misbehavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub evidence_type: String,
    pub validator: Validator,
    pub height: u64,
    pub time: u64,
    pub total_voting_power: i64,
}

/// BeginBlock request data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeginBlockRequest {
    pub header: BlockContext,
    pub last_commit_info: LastCommitInfo,
    pub byzantine_validators: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastCommitInfo {
    pub round: i32,
    pub votes: Vec<VoteInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteInfo {
    pub validator: Validator,
    pub signed_last_block: bool,
}

/// BeginBlock response
#[derive(Debug, Serialize, Deserialize)]
pub struct BeginBlockResponse {
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

/// WASI BeginBlock handler implementation
pub struct WasiBeginBlockHandler {
    /// Track missed blocks for downtime slashing
    missed_blocks: std::collections::HashMap<Vec<u8>, u32>,
    /// Slash window for downtime (default: 10000 blocks)
    slash_window: u64,
    /// Minimum signed blocks percentage (default: 50%)
    min_signed_per_window: f64,
}

impl WasiBeginBlockHandler {
    pub fn new() -> Self {
        Self {
            missed_blocks: std::collections::HashMap::new(),
            slash_window: 10000,
            min_signed_per_window: 0.5,
        }
    }

    /// Main entry point for begin block handling
    pub fn handle(&mut self, req: &BeginBlockRequest) -> BeginBlockResponse {
        log::info!(
            "WASI BeginBlock: Processing block {} on chain {}",
            req.header.height,
            req.header.chain_id
        );

        let mut events = Vec::new();

        // Process block header
        events.extend(self.process_block_header(&req.header));

        // Process last commit info (track validator participation)
        events.extend(self.process_last_commit(&req.last_commit_info, req.header.height));

        // Process byzantine validators (evidence handling)
        events.extend(self.process_evidence(&req.byzantine_validators));

        // Additional block-level processing
        events.extend(self.process_epoch_transitions(&req.header));

        BeginBlockResponse { events }
    }

    fn process_block_header(&self, header: &BlockContext) -> Vec<Event> {
        // Emit new block event
        vec![Event {
            event_type: "new_block".to_string(),
            attributes: vec![
                Attribute {
                    key: "height".to_string(),
                    value: header.height.to_string(),
                },
                Attribute {
                    key: "time".to_string(),
                    value: header.time.to_string(),
                },
                Attribute {
                    key: "proposer".to_string(),
                    value: hex::encode(&header.proposer_address),
                },
                Attribute {
                    key: "app_hash".to_string(),
                    value: hex::encode(&header.app_hash),
                },
            ],
        }]
    }

    fn process_last_commit(&mut self, last_commit: &LastCommitInfo, height: u64) -> Vec<Event> {
        let mut events = vec![];
        let mut missed_validators = vec![];

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
                if *missed_count as f64 / self.slash_window as f64
                    > (1.0 - self.min_signed_per_window)
                {
                    missed_validators.push(hex::encode(&validator_addr));
                    log::warn!(
                        "Validator {} missed {missed_count} blocks in window of {}",
                        hex::encode(&validator_addr),
                        self.slash_window
                    );
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
                    Attribute {
                        key: "height".to_string(),
                        value: height.to_string(),
                    },
                    Attribute {
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
                Attribute {
                    key: "total_validators".to_string(),
                    value: total_validators.to_string(),
                },
                Attribute {
                    key: "signed_validators".to_string(),
                    value: signed_validators.to_string(),
                },
                Attribute {
                    key: "participation_rate".to_string(),
                    value: format!(
                        "{:.2}%",
                        (signed_validators as f64 / total_validators as f64) * 100.0
                    ),
                },
            ],
        });

        events
    }

    fn process_evidence(&self, evidence_list: &[Evidence]) -> Vec<Event> {
        let mut events = vec![];

        for evidence in evidence_list {
            log::warn!(
                "Processing evidence: {} for validator {} at height {}",
                evidence.evidence_type,
                hex::encode(&evidence.validator.address),
                evidence.height
            );

            events.push(Event {
                event_type: "evidence_submitted".to_string(),
                attributes: vec![
                    Attribute {
                        key: "evidence_type".to_string(),
                        value: evidence.evidence_type.clone(),
                    },
                    Attribute {
                        key: "validator".to_string(),
                        value: hex::encode(&evidence.validator.address),
                    },
                    Attribute {
                        key: "height".to_string(),
                        value: evidence.height.to_string(),
                    },
                    Attribute {
                        key: "voting_power".to_string(),
                        value: evidence.total_voting_power.to_string(),
                    },
                ],
            });

            // In a real implementation, this would trigger slashing
            // through interaction with the staking module via VFS
        }

        events
    }

    fn process_epoch_transitions(&self, header: &BlockContext) -> Vec<Event> {
        let mut events = vec![];

        // Check for daily epoch (every 86400 blocks assuming 1s blocks)
        if header.height.is_multiple_of(86400) {
            events.push(Event {
                event_type: "epoch_transition".to_string(),
                attributes: vec![
                    Attribute {
                        key: "epoch_type".to_string(),
                        value: "daily".to_string(),
                    },
                    Attribute {
                        key: "height".to_string(),
                        value: header.height.to_string(),
                    },
                ],
            });
        }

        // Check for weekly epoch
        if header.height.is_multiple_of(86400 * 7) {
            events.push(Event {
                event_type: "epoch_transition".to_string(),
                attributes: vec![
                    Attribute {
                        key: "epoch_type".to_string(),
                        value: "weekly".to_string(),
                    },
                    Attribute {
                        key: "height".to_string(),
                        value: header.height.to_string(),
                    },
                ],
            });
        }

        events
    }

    /// Clean up old missed block records outside the slash window
    fn cleanup_old_records(&mut self, current_height: u64) {
        // In a real implementation, we would track block heights
        // and clean up records older than slash_window
        if current_height.is_multiple_of(1000) {
            log::info!("Cleaning up old missed block records at height {current_height}");
            // Simplified: just clear if too many entries
            if self.missed_blocks.len() > 1000 {
                self.missed_blocks.clear();
            }
        }
    }
}

/// WASI entry point function
/// This function is called by the WASI host to process begin block
#[no_mangle]
pub extern "C" fn begin_block() -> i32 {
    // Initialize logging
    env_logger::init();

    let mut handler = WasiBeginBlockHandler::new();

    // Read input from stdin (begin block request)
    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        log::error!("Failed to read input: {e}");
        return 1;
    }

    // Parse input
    let request: BeginBlockRequest = match serde_json::from_str(&input) {
        Ok(data) => data,
        Err(e) => {
            log::error!("Failed to parse input JSON: {e}");
            return 1;
        }
    };

    // Clean up old records periodically
    handler.cleanup_old_records(request.header.height);

    // Process begin block
    let response = handler.handle(&request);

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

    0 // Success
}

/// Alternative entry point for testing
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn _start() {
    std::process::exit(begin_block());
}

// For non-WASI environments, provide a library interface
impl Default for WasiBeginBlockHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_header() -> BlockContext {
        BlockContext {
            height: 1000,
            time: 1234567890,
            chain_id: "test-chain".to_string(),
            proposer_address: vec![1, 2, 3, 4],
            last_block_hash: vec![5, 6, 7, 8],
            app_hash: vec![9, 10, 11, 12],
        }
    }

    fn create_test_validator(address: u8, power: i64) -> Validator {
        Validator {
            address: vec![address; 20],
            power,
            pub_key_type: "/cosmos.crypto.ed25519.PubKey".to_string(),
            pub_key_value: vec![address; 32],
        }
    }

    #[test]
    fn test_begin_block_handler_creation() {
        let handler = WasiBeginBlockHandler::new();
        assert!(handler.missed_blocks.is_empty());
        assert_eq!(handler.slash_window, 10000);
        assert_eq!(handler.min_signed_per_window, 0.5);
    }

    #[test]
    fn test_process_block_header() {
        let handler = WasiBeginBlockHandler::new();
        let header = create_test_header();

        let events = handler.process_block_header(&header);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "new_block");
        assert_eq!(events[0].attributes.len(), 4);
    }

    #[test]
    fn test_process_last_commit() {
        let mut handler = WasiBeginBlockHandler::new();

        let last_commit = LastCommitInfo {
            round: 0,
            votes: vec![
                VoteInfo {
                    validator: create_test_validator(1, 100),
                    signed_last_block: true,
                },
                VoteInfo {
                    validator: create_test_validator(2, 100),
                    signed_last_block: false,
                },
                VoteInfo {
                    validator: create_test_validator(3, 100),
                    signed_last_block: true,
                },
            ],
        };

        let events = handler.process_last_commit(&last_commit, 1000);

        // Should have participation statistics event
        assert!(events.iter().any(|e| e.event_type == "block_participation"));

        // Check missed blocks tracking
        assert_eq!(handler.missed_blocks.len(), 1);
        assert!(handler.missed_blocks.contains_key(&vec![2; 20]));
    }

    #[test]
    fn test_process_evidence() {
        let handler = WasiBeginBlockHandler::new();

        let evidence = vec![Evidence {
            evidence_type: "duplicate_vote".to_string(),
            validator: create_test_validator(1, 100),
            height: 999,
            time: 1234567880,
            total_voting_power: 1000,
        }];

        let events = handler.process_evidence(&evidence);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "evidence_submitted");
        assert_eq!(events[0].attributes[0].value, "duplicate_vote");
    }

    #[test]
    fn test_epoch_transitions() {
        let handler = WasiBeginBlockHandler::new();

        // Test daily epoch
        let mut header = create_test_header();
        header.height = 86400;
        let events = handler.process_epoch_transitions(&header);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].attributes[0].value, "daily");

        // Test weekly epoch
        header.height = 86400 * 7;
        let events = handler.process_epoch_transitions(&header);
        assert_eq!(events.len(), 2); // Both daily and weekly
    }
}
