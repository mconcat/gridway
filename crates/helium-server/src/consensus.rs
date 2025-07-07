//! Consensus parameter management for the ABCI server
//!
//! This module handles consensus parameter updates and validation.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::abci_server::abci::{
    AbciParams, BlockParams, ConsensusParams, EvidenceParams, ValidatorParams, VersionParams,
};
use helium_store::{KVStore, MemStore};
use thiserror::Error;
use tracing::{debug, info};

/// Consensus parameter errors
#[derive(Error, Debug)]
pub enum ConsensusError {
    /// Store error
    #[error("Store error: {0}")]
    Store(String),

    /// Invalid parameters
    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),
}

pub type Result<T> = std::result::Result<T, ConsensusError>;

/// Default consensus parameters
pub struct DefaultConsensusParams;

impl DefaultConsensusParams {
    /// Get default block parameters
    pub fn block() -> BlockParams {
        BlockParams {
            max_bytes: 22020096, // 21MB
            max_gas: -1,         // No gas limit by default
        }
    }

    /// Get default evidence parameters
    pub fn evidence() -> EvidenceParams {
        EvidenceParams {
            max_age_num_blocks: 100000,
            max_age_duration: Some(prost_types::Duration {
                seconds: 48 * 3600, // 48 hours
                nanos: 0,
            }),
            max_bytes: 1048576, // 1MB
        }
    }

    /// Get default validator parameters
    pub fn validator() -> ValidatorParams {
        ValidatorParams {
            pub_key_types: vec!["ed25519".to_string(), "secp256k1".to_string()],
        }
    }

    /// Get default version parameters
    pub fn version() -> VersionParams {
        VersionParams {
            app: 0, // Version 0 by default
        }
    }

    /// Get default ABCI parameters
    pub fn abci() -> AbciParams {
        AbciParams {
            vote_extensions_enable_height: 0, // Vote extensions disabled by default
        }
    }

    /// Get complete default consensus parameters
    pub fn all() -> ConsensusParams {
        ConsensusParams {
            block: Some(Self::block()),
            evidence: Some(Self::evidence()),
            validator: Some(Self::validator()),
            version: Some(Self::version()),
            abci: Some(Self::abci()),
        }
    }
}

/// Manages consensus parameters
pub struct ConsensusParamsManager {
    /// Current consensus parameters
    current_params: Arc<RwLock<ConsensusParams>>,
    /// Pending parameter updates for the next block
    pending_updates: Arc<RwLock<Option<ConsensusParams>>>,
    /// Store for persisting consensus parameters
    store: Arc<RwLock<Box<dyn KVStore>>>,
}

impl ConsensusParamsManager {
    /// Create a new consensus params manager with defaults
    pub fn new() -> Self {
        Self {
            current_params: Arc::new(RwLock::new(DefaultConsensusParams::all())),
            pending_updates: Arc::new(RwLock::new(None)),
            store: Arc::new(RwLock::new(Box::new(MemStore::new()))),
        }
    }

    /// Create with a specific store
    pub fn with_store(store: Box<dyn KVStore>) -> Self {
        Self {
            current_params: Arc::new(RwLock::new(DefaultConsensusParams::all())),
            pending_updates: Arc::new(RwLock::new(None)),
            store: Arc::new(RwLock::new(store)),
        }
    }

    /// Initialize with specific parameters
    pub async fn init(&self, params: ConsensusParams) -> Result<()> {
        // Validate parameters
        self.validate_params(&params)?;

        // Set as current
        let mut current = self.current_params.write().await;
        *current = params;

        // Save to store
        self.save_params().await?;

        info!("Initialized consensus parameters");
        Ok(())
    }

    /// Load consensus parameters from store
    pub async fn load_params(&self) -> Result<()> {
        let store = self.store.read().await;

        if let Some(_params_bytes) = store
            .get(b"consensus/params")
            .map_err(|e| ConsensusError::Store(e.to_string()))?
        {
            // For now, we'll skip deserialization of consensus params from store
            // This would require custom serialization or using protobuf
            debug!("Loading consensus parameters from store not yet implemented");
        } else {
            debug!("No stored consensus parameters, using defaults");
        }

        Ok(())
    }

    /// Save current consensus parameters to store
    pub async fn save_params(&self) -> Result<()> {
        let _params = self.current_params.read().await;
        let _store = self.store.write().await;

        // For now, we'll skip serialization of consensus params to store
        // This would require custom serialization or using protobuf
        debug!("Saving consensus parameters to store not yet implemented");

        debug!("Saved consensus parameters to store");
        Ok(())
    }

    /// Get current consensus parameters
    pub async fn get_params(&self) -> ConsensusParams {
        self.current_params.read().await.clone()
    }

    /// Propose consensus parameter updates
    pub async fn propose_updates(&self, updates: ConsensusParams) -> Result<()> {
        // Validate the proposed updates
        self.validate_params(&updates)?;

        let mut pending = self.pending_updates.write().await;
        *pending = Some(updates);

        info!("Proposed consensus parameter updates");
        Ok(())
    }

    /// Get and clear pending updates
    pub async fn take_pending_updates(&self) -> Option<ConsensusParams> {
        let mut pending = self.pending_updates.write().await;
        pending.take()
    }

    /// Apply consensus parameter updates
    pub async fn apply_updates(&self, updates: ConsensusParams) -> Result<()> {
        // Validate parameters
        self.validate_params(&updates)?;

        let mut current = self.current_params.write().await;

        // Merge updates with current parameters
        if let Some(block) = updates.block {
            current.block = Some(block);
        }
        if let Some(evidence) = updates.evidence {
            current.evidence = Some(evidence);
        }
        if let Some(validator) = updates.validator {
            current.validator = Some(validator);
        }
        if let Some(version) = updates.version {
            current.version = Some(version);
        }
        if let Some(abci) = updates.abci {
            current.abci = Some(abci);
        }

        drop(current);

        // Save updated parameters
        self.save_params().await?;

        info!("Applied consensus parameter updates");
        Ok(())
    }

    /// Validate consensus parameters
    fn validate_params(&self, params: &ConsensusParams) -> Result<()> {
        // Validate block parameters
        if let Some(block) = &params.block {
            if block.max_bytes <= 0 {
                return Err(ConsensusError::InvalidParams(
                    "Block max_bytes must be positive".to_string(),
                ));
            }
            if block.max_bytes > 104857600 {
                // 100MB max
                return Err(ConsensusError::InvalidParams(
                    "Block max_bytes too large (max 100MB)".to_string(),
                ));
            }
        }

        // Validate evidence parameters
        if let Some(evidence) = &params.evidence {
            if evidence.max_age_num_blocks <= 0 {
                return Err(ConsensusError::InvalidParams(
                    "Evidence max_age_num_blocks must be positive".to_string(),
                ));
            }
            if evidence.max_bytes < 0 {
                return Err(ConsensusError::InvalidParams(
                    "Evidence max_bytes cannot be negative".to_string(),
                ));
            }
        }

        // Validate validator parameters
        if let Some(validator) = &params.validator {
            if validator.pub_key_types.is_empty() {
                return Err(ConsensusError::InvalidParams(
                    "At least one public key type must be allowed".to_string(),
                ));
            }
            for pk_type in &validator.pub_key_types {
                if !["ed25519", "secp256k1", "sr25519"].contains(&pk_type.as_str()) {
                    return Err(ConsensusError::InvalidParams(format!(
                        "Unknown public key type: {pk_type}"
                    )));
                }
            }
        }

        // Validate ABCI parameters
        if let Some(abci) = &params.abci {
            if abci.vote_extensions_enable_height < 0 {
                return Err(ConsensusError::InvalidParams(
                    "Vote extensions enable height cannot be negative".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Check if vote extensions are enabled at a given height
    pub async fn vote_extensions_enabled(&self, height: i64) -> bool {
        let params = self.current_params.read().await;
        if let Some(abci) = &params.abci {
            abci.vote_extensions_enable_height > 0 && height >= abci.vote_extensions_enable_height
        } else {
            false
        }
    }

    /// Get maximum block size
    pub async fn max_block_bytes(&self) -> i64 {
        let params = self.current_params.read().await;
        params
            .block
            .as_ref()
            .map(|b| b.max_bytes)
            .unwrap_or(DefaultConsensusParams::block().max_bytes)
    }

    /// Get maximum gas per block
    pub async fn max_block_gas(&self) -> i64 {
        let params = self.current_params.read().await;
        params
            .block
            .as_ref()
            .map(|b| b.max_gas)
            .unwrap_or(DefaultConsensusParams::block().max_gas)
    }

    /// Check if a public key type is allowed
    pub async fn is_pubkey_type_allowed(&self, pk_type: &str) -> bool {
        let params = self.current_params.read().await;
        if let Some(validator) = &params.validator {
            validator.pub_key_types.contains(&pk_type.to_string())
        } else {
            // Default allowed types
            ["ed25519", "secp256k1"].contains(&pk_type)
        }
    }
}

impl Default for ConsensusParamsManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_consensus_params_manager() {
        let manager = ConsensusParamsManager::new();

        // Get default params
        let params = manager.get_params().await;
        assert!(params.block.is_some());
        assert!(params.evidence.is_some());
        assert!(params.validator.is_some());
    }

    #[tokio::test]
    async fn test_param_validation() {
        let manager = ConsensusParamsManager::new();

        // Invalid block params
        let mut params = DefaultConsensusParams::all();
        params.block = Some(BlockParams {
            max_bytes: -1,
            max_gas: -1,
        });
        assert!(manager.propose_updates(params).await.is_err());

        // Invalid evidence params
        let mut params = DefaultConsensusParams::all();
        params.evidence = Some(EvidenceParams {
            max_age_num_blocks: 0,
            max_age_duration: None,
            max_bytes: 0,
        });
        assert!(manager.propose_updates(params).await.is_err());

        // Invalid validator params
        let mut params = DefaultConsensusParams::all();
        params.validator = Some(ValidatorParams {
            pub_key_types: vec![],
        });
        assert!(manager.propose_updates(params).await.is_err());
    }

    #[tokio::test]
    async fn test_param_updates() {
        let manager = ConsensusParamsManager::new();

        // Propose updates
        let updates = ConsensusParams {
            block: Some(BlockParams {
                max_bytes: 10485760, // 10MB
                max_gas: 1000000,
            }),
            evidence: None,
            validator: None,
            version: None,
            abci: None,
        };
        manager.propose_updates(updates.clone()).await.unwrap();

        // Take pending updates
        let pending = manager.take_pending_updates().await;
        assert!(pending.is_some());

        // Apply updates
        manager.apply_updates(updates).await.unwrap();

        // Check updated values
        assert_eq!(manager.max_block_bytes().await, 10485760);
        assert_eq!(manager.max_block_gas().await, 1000000);
    }

    #[tokio::test]
    async fn test_vote_extensions() {
        let manager = ConsensusParamsManager::new();

        // Initially disabled
        assert!(!manager.vote_extensions_enabled(100).await);

        // Enable at height 50
        let updates = ConsensusParams {
            block: None,
            evidence: None,
            validator: None,
            version: None,
            abci: Some(AbciParams {
                vote_extensions_enable_height: 50,
            }),
        };
        manager.apply_updates(updates).await.unwrap();

        // Check at various heights
        assert!(!manager.vote_extensions_enabled(49).await);
        assert!(manager.vote_extensions_enabled(50).await);
        assert!(manager.vote_extensions_enabled(100).await);
    }
}
