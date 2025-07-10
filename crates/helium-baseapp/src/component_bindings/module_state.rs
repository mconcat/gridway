//! Module state component bindings
//!
//! Provides module state interface for WASI components.

use std::sync::{Arc, Mutex};

/// Validator update data
#[derive(Debug, Clone)]
pub struct ValidatorUpdateData {
    pub pub_key_type: String,
    pub pub_key_value: Vec<u8>,
    pub power: i64,
}

/// Governance proposal
#[derive(Debug, Clone)]
pub struct Proposal {
    pub id: u64,
    pub voting_end_time: u64,
    pub yes_votes: u64,
    pub no_votes: u64,
    pub abstain_votes: u64,
    pub no_with_veto_votes: u64,
}

/// Module state data
#[derive(Debug, Clone)]
pub struct StateData {
    /// Pending validator updates
    pub pending_validator_updates: Vec<ValidatorUpdateData>,
    /// Active governance proposals
    pub active_proposals: Vec<Proposal>,
    /// Current inflation rate
    pub inflation_rate: f64,
    /// Last reward distribution height
    pub last_reward_height: u64,
    /// Total voting power
    pub total_power: i64,
    /// Block proposer address
    pub proposer_address: Vec<u8>,
}

impl Default for StateData {
    fn default() -> Self {
        Self {
            pending_validator_updates: vec![],
            active_proposals: vec![],
            inflation_rate: 0.05, // 5% default inflation
            last_reward_height: 0,
            total_power: 0,
            proposer_address: vec![],
        }
    }
}

/// Module state manager for component hosts
#[derive(Clone)]
pub struct ModuleStateManager {
    /// Current module state
    state: Arc<Mutex<StateData>>,
}

impl ModuleStateManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(StateData::default())),
        }
    }

    /// Get current module state
    pub fn get_state(&self) -> Result<StateData, String> {
        let state = self
            .state
            .lock()
            .map_err(|e| format!("Failed to lock state: {e}"))?;
        Ok(state.clone())
    }

    /// Update module state
    pub fn update_state(&self, new_state: StateData) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|e| format!("Failed to lock state: {e}"))?;
        *state = new_state;
        Ok(())
    }

    /// Add a validator update
    pub fn add_validator_update(&self, update: ValidatorUpdateData) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|e| format!("Failed to lock state: {e}"))?;
        state.pending_validator_updates.push(update);
        Ok(())
    }

    /// Set block proposer
    pub fn set_proposer(&self, proposer_address: Vec<u8>) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|e| format!("Failed to lock state: {e}"))?;
        state.proposer_address = proposer_address;
        Ok(())
    }

    /// Set total voting power
    pub fn set_total_power(&self, total_power: i64) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|e| format!("Failed to lock state: {e}"))?;
        state.total_power = total_power;
        Ok(())
    }
}

impl Default for ModuleStateManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Host trait for module-state interface
pub trait Host {
    /// Get current module state
    fn get_state(&mut self) -> StateData;
}

/// Add module-state interface to linker
pub fn add_to_linker<T>(
    _linker: &mut wasmtime::component::Linker<T>,
    _get: impl Fn(&mut T) -> &mut dyn Host + Send + Sync + 'static,
) -> wasmtime::Result<()> {
    // For now, this is a placeholder implementation
    // A full implementation would use the wasmtime component model's
    // canonical ABI to properly encode/decode the StateData record type
    
    // TODO: Implement proper canonical ABI encoding for StateData
    Ok(())
}