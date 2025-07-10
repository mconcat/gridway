//! WASI EndBlock Handler Component
//!
//! This module implements the EndBlock ABCI handler as a WASI component using the
//! component model and WIT interfaces.

// Removed serde imports - now using WIT-generated types

// Include generated bindings
mod bindings;

use bindings::exports::helium::framework::end_blocker::{
    EndBlockRequest, EndBlockResponse, Event, EventAttribute, ValidatorUpdate, ValidatorPubKey, Guest,
};
use bindings::helium::framework::module_state;

// Using WIT-generated types instead of local structs

struct Component;

impl Guest for Component {
    fn end_block(request: EndBlockRequest) -> EndBlockResponse {
        // Get module state data from the module-state interface
        let state_data = module_state::get_state();

        let mut validator_updates = vec![];
        let mut events = vec![];

        // Constants
        const REWARD_FREQUENCY: u64 = 1000; // Distribute rewards every 1000 blocks
        const QUORUM_THRESHOLD: f64 = 0.334; // 33.4% quorum
        const PASS_THRESHOLD: f64 = 0.5; // 50% to pass

        // Process validator set updates
        if !state_data.pending_validator_updates.is_empty() {
            let (updates, update_events) =
                process_validator_updates(&state_data.pending_validator_updates);
            validator_updates = updates;
            events.extend(update_events);
        }

        // Process reward distribution
        if should_distribute_rewards(
            request.height,
            state_data.last_reward_height,
            REWARD_FREQUENCY,
        ) {
            events.extend(trigger_reward_distribution(request.height, &state_data));
        }

        // Process governance proposals  
        let governance_events = process_governance_proposals(
            &state_data.active_proposals,
            QUORUM_THRESHOLD,
            PASS_THRESHOLD,
        );
        events.extend(governance_events);

        // Process inflation adjustments
        events.extend(process_inflation_adjustment(
            request.height,
            state_data.inflation_rate,
        ));

        // Emit block completion event
        events.push(Event {
            event_type: "block_completed".to_string(),
            attributes: vec![
                EventAttribute {
                    key: "height".to_string(),
                    value: request.height.to_string(),
                },
                EventAttribute {
                    key: "validator_updates".to_string(),
                    value: validator_updates.len().to_string(),
                },
            ],
        });

        EndBlockResponse {
            success: true,
            validator_updates,
            events,
            error: None,
        }
    }
}

fn process_validator_updates(
    pending_updates: &[module_state::ValidatorUpdateData],
) -> (Vec<ValidatorUpdate>, Vec<Event>) {
    let mut events = vec![];
    let mut final_updates = vec![];

    for update in pending_updates {
        // Validate update
        if update.power < 0 {
            continue; // Skip invalid negative power
        }

        // Create structured validator update
        let validator_update = ValidatorUpdate {
            pub_key: ValidatorPubKey {
                key_type: update.pub_key_type.clone(),
                value: update.pub_key_value.clone(),
            },
            power: update.power,
        };

        final_updates.push(validator_update);

        // Emit event for each update
        events.push(Event {
            event_type: "validator_update".to_string(),
            attributes: vec![
                EventAttribute {
                    key: "pubkey".to_string(),
                    value: hex::encode(&update.pub_key_value),
                },
                EventAttribute {
                    key: "power".to_string(),
                    value: update.power.to_string(),
                },
                EventAttribute {
                    key: "action".to_string(),
                    value: if update.power == 0 {
                        "remove"
                    } else {
                        "update"
                    }
                    .to_string(),
                },
            ],
        });
    }

    (final_updates, events)
}

fn should_distribute_rewards(
    current_height: u64,
    last_reward_height: u64,
    reward_frequency: u64,
) -> bool {
    current_height >= last_reward_height + reward_frequency
}

fn trigger_reward_distribution(height: u64, state_data: &module_state::StateData) -> Vec<Event> {
    let mut events = vec![];

    // Calculate rewards based on inflation
    let block_rewards =
        calculate_block_rewards(state_data.inflation_rate, state_data.total_power);

    events.push(Event {
        event_type: "rewards_distribution".to_string(),
        attributes: vec![
            EventAttribute {
                key: "height".to_string(),
                value: height.to_string(),
            },
            EventAttribute {
                key: "inflation_rate".to_string(),
                value: format!("{:.4}%", state_data.inflation_rate * 100.0),
            },
            EventAttribute {
                key: "total_rewards".to_string(),
                value: block_rewards.to_string(),
            },
            EventAttribute {
                key: "proposer".to_string(),
                value: hex::encode(&state_data.proposer_address),
            },
        ],
    });

    events
}

fn calculate_block_rewards(inflation_rate: f64, total_power: i64) -> u64 {
    // Simplified reward calculation
    let annual_inflation = (total_power as f64) * inflation_rate;
    let blocks_per_year = 365 * 24 * 60 * 60; // Assuming 1s blocks
    (annual_inflation / blocks_per_year as f64) as u64
}

fn process_governance_proposals(
    proposals: &[module_state::Proposal],
    quorum_threshold: f64,
    pass_threshold: f64,
) -> Vec<Event> {
    let mut events = vec![];

    for proposal in proposals {
        // For now, process all proposals (time check would need to be added later)
        let (passed, reason) = evaluate_proposal(proposal, quorum_threshold, pass_threshold);

        events.push(Event {
            event_type: "proposal_finalized".to_string(),
            attributes: vec![
                EventAttribute {
                    key: "proposal_id".to_string(),
                    value: proposal.id.to_string(),
                },
                EventAttribute {
                    key: "result".to_string(),
                    value: if passed { "passed" } else { "failed" }.to_string(),
                },
                EventAttribute {
                    key: "reason".to_string(),
                    value: reason,
                },
                EventAttribute {
                    key: "yes_votes".to_string(),
                    value: proposal.yes_votes.to_string(),
                },
                EventAttribute {
                    key: "no_votes".to_string(),
                    value: proposal.no_votes.to_string(),
                },
                EventAttribute {
                    key: "abstain_votes".to_string(),
                    value: proposal.abstain_votes.to_string(),
                },
                EventAttribute {
                    key: "veto_votes".to_string(),
                    value: proposal.no_with_veto_votes.to_string(),
                },
            ],
        });
    }

    events
}

fn evaluate_proposal(
    proposal: &module_state::Proposal,
    quorum_threshold: f64,
    pass_threshold: f64,
) -> (bool, String) {
    let total_votes =
        proposal.yes_votes + proposal.no_votes + proposal.abstain_votes + proposal.no_with_veto_votes;

    if total_votes == 0 {
        return (false, "no votes cast".to_string());
    }

    // Check quorum
    let voting_power_percentage = 0.5; // Simplified - assume 50% of total power voted
    if voting_power_percentage < quorum_threshold {
        return (false, "quorum not reached".to_string());
    }

    // Check veto threshold (1/3 of votes)
    if proposal.no_with_veto_votes as f64 / total_votes as f64 > 0.334 {
        return (false, "vetoed".to_string());
    }

    // Check pass threshold
    let yes_no_total = proposal.yes_votes + proposal.no_votes;
    if yes_no_total == 0 {
        return (false, "no yes/no votes".to_string());
    }

    let yes_percentage = proposal.yes_votes as f64 / yes_no_total as f64;
    if yes_percentage >= pass_threshold {
        (true, "passed".to_string())
    } else {
        (false, "did not reach pass threshold".to_string())
    }
}

fn process_inflation_adjustment(height: u64, current_rate: f64) -> Vec<Event> {
    let mut events = vec![];

    // Check if it's time for inflation adjustment (e.g., daily)
    if height % 86400 == 0 {
        // In a real implementation, this would calculate new inflation based on bonding ratio
        let new_rate = current_rate; // Simplified - keep same rate

        events.push(Event {
            event_type: "inflation_adjustment".to_string(),
            attributes: vec![
                EventAttribute {
                    key: "height".to_string(),
                    value: height.to_string(),
                },
                EventAttribute {
                    key: "old_rate".to_string(),
                    value: format!("{:.4}%", current_rate * 100.0),
                },
                EventAttribute {
                    key: "new_rate".to_string(),
                    value: format!("{:.4}%", new_rate * 100.0),
                },
            ],
        });
    }

    events
}

bindings::export!(Component with_types_in bindings);
