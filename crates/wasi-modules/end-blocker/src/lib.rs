//! WASI EndBlock Handler Module
//!
//! This module implements the EndBlock ABCI handler as a WASI program that can be
//! dynamically loaded by the BaseApp. It handles end-of-block processing including
//! validator set updates, reward distribution triggers, and governance tallying.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::collections::HashMap;
use thiserror::Error;

/// Error types for end block operations
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum EndBlockError {
    #[error("Validator update error: {0}")]
    ValidatorUpdateError(String),
    
    #[error("Reward distribution error: {0}")]
    RewardError(String),
    
    #[error("Governance tally error: {0}")]
    GovernanceError(String),
    
    #[error("IO error: {0}")]
    IoError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Result type for end block operations
pub type EndBlockResult<T> = Result<T, EndBlockError>;

/// EndBlock request data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndBlockRequest {
    pub height: u64,
    pub time: u64,
    pub chain_id: String,
    pub total_power: i64,
    pub proposer_address: Vec<u8>,
}

/// Validator update for consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorUpdate {
    pub pub_key: PubKey,
    pub power: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PubKey {
    pub type_url: String,
    pub value: Vec<u8>,
}

/// Governance proposal information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u64,
    pub voting_end_time: u64,
    pub tally: TallyResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TallyResult {
    pub yes_votes: u64,
    pub no_votes: u64,
    pub abstain_votes: u64,
    pub no_with_veto_votes: u64,
}

/// EndBlock response
#[derive(Debug, Serialize, Deserialize)]
pub struct EndBlockResponse {
    pub validator_updates: Vec<ValidatorUpdate>,
    pub consensus_param_updates: Option<ConsensusParams>,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusParams {
    pub block_max_bytes: i64,
    pub block_max_gas: i64,
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

/// Module state retrieved from VFS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleState {
    pub pending_validator_updates: Vec<ValidatorUpdate>,
    pub active_proposals: Vec<Proposal>,
    pub inflation_rate: f64,
    pub last_reward_height: u64,
}

/// WASI EndBlock handler implementation
pub struct WasiEndBlockHandler {
    /// Reward distribution frequency (blocks)
    reward_frequency: u64,
    /// Governance quorum requirement (percentage)
    quorum_threshold: f64,
    /// Pass threshold for proposals (percentage)
    pass_threshold: f64,
}

impl WasiEndBlockHandler {
    pub fn new() -> Self {
        Self {
            reward_frequency: 1000,  // Distribute rewards every 1000 blocks
            quorum_threshold: 0.334, // 33.4% quorum
            pass_threshold: 0.5,     // 50% to pass
        }
    }

    /// Main entry point for end block handling
    pub fn handle(&self, req: &EndBlockRequest, state: &ModuleState) -> EndBlockResponse {
        log::info!(
            "WASI EndBlock: Processing block {} on chain {}",
            req.height,
            req.chain_id
        );

        let mut validator_updates = vec![];
        let mut events = vec![];

        // Process validator set updates
        if !state.pending_validator_updates.is_empty() {
            let (updates, update_events) = self.process_validator_updates(&state.pending_validator_updates);
            validator_updates = updates;
            events.extend(update_events);
        }

        // Process reward distribution
        if self.should_distribute_rewards(req.height, state.last_reward_height) {
            events.extend(self.trigger_reward_distribution(req, state));
        }

        // Process governance proposals
        let (governance_events, param_updates) = self.process_governance_proposals(req, &state.active_proposals);
        events.extend(governance_events);

        // Process inflation adjustments
        events.extend(self.process_inflation_adjustment(req, state.inflation_rate));

        // Emit block completion event
        events.push(Event {
            event_type: "block_completed".to_string(),
            attributes: vec![
                Attribute {
                    key: "height".to_string(),
                    value: req.height.to_string(),
                },
                Attribute {
                    key: "validator_updates".to_string(),
                    value: validator_updates.len().to_string(),
                },
            ],
        });

        EndBlockResponse {
            validator_updates,
            consensus_param_updates: param_updates,
            events,
        }
    }

    fn process_validator_updates(&self, pending_updates: &[ValidatorUpdate]) -> (Vec<ValidatorUpdate>, Vec<Event>) {
        let mut events = vec![];
        let mut final_updates = vec![];

        for update in pending_updates {
            log::info!(
                "Processing validator update: {:?} with power {}",
                hex::encode(&update.pub_key.value),
                update.power
            );

            // Validate update
            if update.power < 0 {
                log::warn!("Invalid negative power {} for validator", update.power);
                continue;
            }

            final_updates.push(update.clone());

            // Emit event for each update
            events.push(Event {
                event_type: "validator_update".to_string(),
                attributes: vec![
                    Attribute {
                        key: "pubkey".to_string(),
                        value: hex::encode(&update.pub_key.value),
                    },
                    Attribute {
                        key: "power".to_string(),
                        value: update.power.to_string(),
                    },
                    Attribute {
                        key: "action".to_string(),
                        value: if update.power == 0 { "remove" } else { "update" }.to_string(),
                    },
                ],
            });
        }

        (final_updates, events)
    }

    fn should_distribute_rewards(&self, current_height: u64, last_reward_height: u64) -> bool {
        current_height >= last_reward_height + self.reward_frequency
    }

    fn trigger_reward_distribution(&self, req: &EndBlockRequest, state: &ModuleState) -> Vec<Event> {
        let mut events = vec![];

        // Calculate rewards based on inflation
        let block_rewards = self.calculate_block_rewards(state.inflation_rate, req.total_power);

        events.push(Event {
            event_type: "rewards_distribution".to_string(),
            attributes: vec![
                Attribute {
                    key: "height".to_string(),
                    value: req.height.to_string(),
                },
                Attribute {
                    key: "inflation_rate".to_string(),
                    value: format!("{:.4}%", state.inflation_rate * 100.0),
                },
                Attribute {
                    key: "total_rewards".to_string(),
                    value: block_rewards.to_string(),
                },
                Attribute {
                    key: "proposer".to_string(),
                    value: hex::encode(&req.proposer_address),
                },
            ],
        });

        events
    }

    fn calculate_block_rewards(&self, inflation_rate: f64, total_power: i64) -> u64 {
        // Simplified reward calculation
        // In reality, this would consider total supply, bonded ratio, etc.
        let annual_inflation = (total_power as f64) * inflation_rate;
        let blocks_per_year = 365 * 24 * 60 * 60; // Assuming 1s blocks
        (annual_inflation / blocks_per_year as f64) as u64
    }

    fn process_governance_proposals(
        &self,
        req: &EndBlockRequest,
        proposals: &[Proposal]
    ) -> (Vec<Event>, Option<ConsensusParams>) {
        let mut events = vec![];
        let mut param_updates = None;

        for proposal in proposals {
            // Check if voting period has ended
            if req.time >= proposal.voting_end_time {
                let (passed, reason) = self.evaluate_proposal(&proposal.tally);

                events.push(Event {
                    event_type: "proposal_finalized".to_string(),
                    attributes: vec![
                        Attribute {
                            key: "proposal_id".to_string(),
                            value: proposal.id.to_string(),
                        },
                        Attribute {
                            key: "result".to_string(),
                            value: if passed { "passed" } else { "failed" }.to_string(),
                        },
                        Attribute {
                            key: "reason".to_string(),
                            value: reason,
                        },
                        Attribute {
                            key: "yes_votes".to_string(),
                            value: proposal.tally.yes_votes.to_string(),
                        },
                        Attribute {
                            key: "no_votes".to_string(),
                            value: proposal.tally.no_votes.to_string(),
                        },
                    ],
                });

                // If proposal passed and it's a parameter change proposal
                if passed && proposal.id % 10 == 0 { // Simplified: every 10th proposal is param change
                    param_updates = Some(ConsensusParams {
                        block_max_bytes: 21_000_000, // 21MB
                        block_max_gas: 10_000_000,   // 10M gas
                    });
                }
            }
        }

        (events, param_updates)
    }

    fn evaluate_proposal(&self, tally: &TallyResult) -> (bool, String) {
        let total_votes = tally.yes_votes + tally.no_votes + tally.abstain_votes + tally.no_with_veto_votes;
        
        if total_votes == 0 {
            return (false, "no votes cast".to_string());
        }

        let participation_rate = total_votes as f64 / 1_000_000.0; // Assuming 1M total voting power
        
        // Check quorum
        if participation_rate < self.quorum_threshold {
            return (false, format!("quorum not met: {:.2}%", participation_rate * 100.0));
        }

        // Check veto threshold (more than 33.4% veto fails the proposal)
        let veto_rate = tally.no_with_veto_votes as f64 / total_votes as f64;
        if veto_rate > 0.334 {
            return (false, format!("vetoed: {:.2}% veto votes", veto_rate * 100.0));
        }

        // Check pass threshold (excluding abstain votes)
        let active_votes = tally.yes_votes + tally.no_votes + tally.no_with_veto_votes;
        if active_votes == 0 {
            return (false, "no active votes".to_string());
        }

        let yes_rate = tally.yes_votes as f64 / active_votes as f64;
        if yes_rate > self.pass_threshold {
            (true, format!("passed with {:.2}% yes votes", yes_rate * 100.0))
        } else {
            (false, format!("failed with only {:.2}% yes votes", yes_rate * 100.0))
        }
    }

    fn process_inflation_adjustment(&self, req: &EndBlockRequest, current_rate: f64) -> Vec<Event> {
        let mut events = vec![];

        // Adjust inflation every 1M blocks (roughly 11.5 days)
        if req.height % 1_000_000 == 0 {
            // Simplified inflation adjustment logic
            let target_bonded_ratio = 0.67; // Target 67% bonded
            let current_bonded_ratio = 0.65; // Would be calculated from actual state
            
            let new_rate = if current_bonded_ratio < target_bonded_ratio {
                // Increase inflation to incentivize bonding
                (current_rate * 1.01).min(0.20) // Max 20% inflation
            } else {
                // Decrease inflation
                (current_rate * 0.99).max(0.07) // Min 7% inflation
            };

            events.push(Event {
                event_type: "inflation_adjustment".to_string(),
                attributes: vec![
                    Attribute {
                        key: "height".to_string(),
                        value: req.height.to_string(),
                    },
                    Attribute {
                        key: "old_rate".to_string(),
                        value: format!("{:.4}%", current_rate * 100.0),
                    },
                    Attribute {
                        key: "new_rate".to_string(),
                        value: format!("{:.4}%", new_rate * 100.0),
                    },
                    Attribute {
                        key: "bonded_ratio".to_string(),
                        value: format!("{:.2}%", current_bonded_ratio * 100.0),
                    },
                ],
            });
        }

        events
    }
}

/// WASI entry point function
/// This function is called by the WASI host to process end block
#[no_mangle]
pub extern "C" fn end_block() -> i32 {
    // Initialize logging
    env_logger::init();

    let handler = WasiEndBlockHandler::new();

    // Read input from stdin
    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        log::error!("Failed to read input: {}", e);
        return 1;
    }

    // Parse input as tuple of request and state
    let (request, state): (EndBlockRequest, ModuleState) = match serde_json::from_str(&input) {
        Ok(data) => data,
        Err(e) => {
            log::error!("Failed to parse input JSON: {}", e);
            return 1;
        }
    };

    // Process end block
    let response = handler.handle(&request, &state);

    // Write response to stdout
    match serde_json::to_string(&response) {
        Ok(output) => {
            if let Err(e) = io::stdout().write_all(output.as_bytes()) {
                log::error!("Failed to write output: {}", e);
                return 1;
            }
        }
        Err(e) => {
            log::error!("Failed to serialize response: {}", e);
            return 1;
        }
    }

    0 // Success
}

/// Alternative entry point for testing
#[no_mangle]
pub extern "C" fn _start() {
    std::process::exit(end_block());
}

// For non-WASI environments, provide a library interface
impl Default for WasiEndBlockHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_request() -> EndBlockRequest {
        EndBlockRequest {
            height: 10000,
            time: 1234567890,
            chain_id: "test-chain".to_string(),
            total_power: 1000000,
            proposer_address: vec![1, 2, 3, 4],
        }
    }

    fn create_test_state() -> ModuleState {
        ModuleState {
            pending_validator_updates: vec![],
            active_proposals: vec![],
            inflation_rate: 0.10,
            last_reward_height: 9000,
        }
    }

    #[test]
    fn test_end_block_handler_creation() {
        let handler = WasiEndBlockHandler::new();
        assert_eq!(handler.reward_frequency, 1000);
        assert_eq!(handler.quorum_threshold, 0.334);
        assert_eq!(handler.pass_threshold, 0.5);
    }

    #[test]
    fn test_validator_updates() {
        let handler = WasiEndBlockHandler::new();
        
        let updates = vec![
            ValidatorUpdate {
                pub_key: PubKey {
                    type_url: "/cosmos.crypto.ed25519.PubKey".to_string(),
                    value: vec![1; 32],
                },
                power: 100,
            },
            ValidatorUpdate {
                pub_key: PubKey {
                    type_url: "/cosmos.crypto.ed25519.PubKey".to_string(),
                    value: vec![2; 32],
                },
                power: 0, // Remove validator
            },
        ];
        
        let (final_updates, events) = handler.process_validator_updates(&updates);
        
        assert_eq!(final_updates.len(), 2);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].attributes[2].value, "remove");
    }

    #[test]
    fn test_reward_distribution_trigger() {
        let handler = WasiEndBlockHandler::new();
        
        // Should trigger (1000 blocks passed)
        assert!(handler.should_distribute_rewards(10000, 9000));
        
        // Should not trigger (only 500 blocks passed)
        assert!(!handler.should_distribute_rewards(9500, 9000));
    }

    #[test]
    fn test_proposal_evaluation() {
        let handler = WasiEndBlockHandler::new();
        
        // Test passing proposal
        let tally = TallyResult {
            yes_votes: 600_000,
            no_votes: 200_000,
            abstain_votes: 100_000,
            no_with_veto_votes: 50_000,
        };
        
        let (passed, reason) = handler.evaluate_proposal(&tally);
        assert!(passed);
        assert!(reason.contains("passed"));
        
        // Test failed quorum
        let tally_low = TallyResult {
            yes_votes: 100_000,
            no_votes: 50_000,
            abstain_votes: 50_000,
            no_with_veto_votes: 0,
        };
        
        let (passed, reason) = handler.evaluate_proposal(&tally_low);
        assert!(!passed);
        assert!(reason.contains("quorum"));
        
        // Test veto
        let tally_veto = TallyResult {
            yes_votes: 400_000,
            no_votes: 100_000,
            abstain_votes: 100_000,
            no_with_veto_votes: 400_000,
        };
        
        let (passed, reason) = handler.evaluate_proposal(&tally_veto);
        assert!(!passed);
        assert!(reason.contains("vetoed"));
    }
}