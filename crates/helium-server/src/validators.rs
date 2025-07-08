//! Validator set management for the ABCI server
//!
//! This module handles validator set updates, power changes, and validator state management.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::abci_server::abci::Validator;
use helium_store::{KVStore, MemStore};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Validator management errors
#[derive(Error, Debug)]
pub enum ValidatorError {
    /// Store error
    #[error("Store error: {0}")]
    Store(String),

    /// Invalid validator
    #[error("Invalid validator: {0}")]
    InvalidValidator(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),
}

pub type Result<T> = std::result::Result<T, ValidatorError>;

/// Validator information stored in the state
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidatorInfo {
    /// Validator address (typically 20 bytes)
    pub address: Vec<u8>,
    /// Voting power
    pub power: i64,
    /// Public key bytes
    pub pub_key: Vec<u8>,
    /// Whether the validator is currently active
    pub active: bool,
    /// Block height when the validator was last updated
    pub last_update_height: u64,
}

/// Manages the validator set and updates
pub struct ValidatorManager {
    /// Current validator set
    validators: Arc<RwLock<HashMap<Vec<u8>, ValidatorInfo>>>,
    /// Pending validator updates for the next block
    pending_updates: Arc<RwLock<Vec<Validator>>>,
    /// Store for persisting validator state
    store: Arc<RwLock<Box<dyn KVStore>>>,
    /// Maximum number of validators
    max_validators: usize,
}

impl ValidatorManager {
    /// Create a new validator manager
    pub fn new(max_validators: usize) -> Self {
        Self {
            validators: Arc::new(RwLock::new(HashMap::new())),
            pending_updates: Arc::new(RwLock::new(Vec::new())),
            store: Arc::new(RwLock::new(Box::new(MemStore::new()))),
            max_validators,
        }
    }

    /// Initialize validator manager with a specific store
    pub fn with_store(store: Box<dyn KVStore>, max_validators: usize) -> Self {
        Self {
            validators: Arc::new(RwLock::new(HashMap::new())),
            pending_updates: Arc::new(RwLock::new(Vec::new())),
            store: Arc::new(RwLock::new(store)),
            max_validators,
        }
    }

    /// Load validators from store
    pub async fn load_validators(&self) -> Result<()> {
        let store = self.store.read().await;
        let mut validators = self.validators.write().await;

        // Load validator addresses for stable ordering
        if let Some(addr_list_bytes) = store
            .get(b"validators/addresses")
            .map_err(|e| ValidatorError::Store(e.to_string()))?
        {
            let addresses: Vec<String> = serde_json::from_slice(&addr_list_bytes)
                .map_err(|e| ValidatorError::Serialization(e.to_string()))?;

            // Load each validator by address
            for addr_hex in addresses {
                let key = format!("validators/addr/{addr_hex}");
                if let Some(val_bytes) = store
                    .get(key.as_bytes())
                    .map_err(|e| ValidatorError::Store(e.to_string()))?
                {
                    let validator: ValidatorInfo = serde_json::from_slice(&val_bytes)
                        .map_err(|e| ValidatorError::Serialization(e.to_string()))?;
                    validators.insert(validator.address.clone(), validator);
                }
            }

            info!("Loaded {} validators from store", validators.len());
        } else {
            // Fallback: try loading with old format for backward compatibility
            if let Some(count_bytes) = store
                .get(b"validators/count")
                .map_err(|e| ValidatorError::Store(e.to_string()))?
            {
                let count = u32::from_be_bytes(count_bytes.try_into().map_err(|_| {
                    ValidatorError::Serialization("Invalid count format".to_string())
                })?);

                // Load each validator
                for i in 0..count {
                    let key = format!("validators/{i}");
                    if let Some(val_bytes) = store
                        .get(key.as_bytes())
                        .map_err(|e| ValidatorError::Store(e.to_string()))?
                    {
                        let validator: ValidatorInfo = serde_json::from_slice(&val_bytes)
                            .map_err(|e| ValidatorError::Serialization(e.to_string()))?;
                        validators.insert(validator.address.clone(), validator);
                    }
                }

                info!(
                    "Loaded {} validators from store (legacy format)",
                    validators.len()
                );
            }
        }

        Ok(())
    }

    /// Save validators to store
    pub async fn save_validators(&self) -> Result<()> {
        let validators = self.validators.read().await;
        let mut store = self.store.write().await;

        // Save validator count (kept for backward compatibility)
        let count = validators.len() as u32;
        store
            .set(b"validators/count", &count.to_be_bytes())
            .map_err(|e| ValidatorError::Store(e.to_string()))?;

        // Collect and save validator addresses for stable ordering
        let addresses: Vec<String> = validators.keys().map(hex::encode).collect();
        let addr_bytes = serde_json::to_vec(&addresses)
            .map_err(|e| ValidatorError::Serialization(e.to_string()))?;
        store
            .set(b"validators/addresses", &addr_bytes)
            .map_err(|e| ValidatorError::Store(e.to_string()))?;

        // Save each validator by address
        for (address, validator) in validators.iter() {
            let key = format!("validators/addr/{}", hex::encode(address));
            let val_bytes = serde_json::to_vec(validator)
                .map_err(|e| ValidatorError::Serialization(e.to_string()))?;
            store
                .set(key.as_bytes(), &val_bytes)
                .map_err(|e| ValidatorError::Store(e.to_string()))?;
        }

        // Clean up old indexed entries if they exist
        for i in 0..count {
            let old_key = format!("validators/{i}");
            let _ = store.delete(old_key.as_bytes());
        }

        debug!("Saved {} validators to store", validators.len());
        Ok(())
    }

    /// Get the current validator set as ABCI validators
    pub async fn get_validators(&self) -> Vec<Validator> {
        let validators = self.validators.read().await;
        validators
            .values()
            .filter(|v| v.active && v.power > 0)
            .map(|v| Validator {
                address: v.address.clone(),
                power: v.power,
            })
            .collect()
    }

    /// Update validator power
    pub async fn update_validator(
        &self,
        address: Vec<u8>,
        power: i64,
        pub_key: Vec<u8>,
        height: u64,
    ) -> Result<()> {
        let mut validators = self.validators.write().await;
        let mut pending = self.pending_updates.write().await;

        // Check if we're at max validators and this is a new validator
        if power > 0
            && !validators.contains_key(&address)
            && validators.values().filter(|v| v.active).count() >= self.max_validators
        {
            return Err(ValidatorError::InvalidValidator(format!(
                "Maximum number of validators ({}) reached",
                self.max_validators
            )));
        }

        // Update or insert validator
        let validator = validators.entry(address.clone()).or_insert(ValidatorInfo {
            address: address.clone(),
            power: 0,
            pub_key: pub_key.clone(),
            active: false,
            last_update_height: height,
        });

        // Update validator info
        validator.power = power;
        validator.pub_key = pub_key;
        validator.last_update_height = height;
        validator.active = power > 0;

        // Add to pending updates
        pending.push(Validator {
            address: address.clone(),
            power,
        });

        if power > 0 {
            info!(
                "Updated validator {} with power {}",
                hex::encode(&address),
                power
            );
        } else {
            info!("Removed validator {}", hex::encode(&address));
        }

        Ok(())
    }

    /// Remove a validator (set power to 0)
    pub async fn remove_validator(&self, address: Vec<u8>, height: u64) -> Result<()> {
        self.update_validator(address, 0, vec![], height).await
    }

    /// Get pending validator updates and clear them
    pub async fn take_pending_updates(&self) -> Vec<Validator> {
        let mut pending = self.pending_updates.write().await;
        std::mem::take(&mut *pending)
    }

    /// Apply validator updates from the consensus engine
    pub async fn apply_updates(&self, updates: Vec<Validator>, height: u64) -> Result<()> {
        for update in updates {
            if update.power > 0 {
                // For updates from consensus, we might not have the public key
                // In a real implementation, this would be provided by the staking module
                self.update_validator(
                    update.address.clone(),
                    update.power,
                    vec![], // Public key would come from staking module
                    height,
                )
                .await?;
            } else {
                self.remove_validator(update.address, height).await?;
            }
        }

        // Save to store after applying all updates
        self.save_validators().await?;

        Ok(())
    }

    /// Get validator by address
    pub async fn get_validator(&self, address: &[u8]) -> Option<ValidatorInfo> {
        let validators = self.validators.read().await;
        validators.get(address).cloned()
    }

    /// Get total voting power
    pub async fn get_total_power(&self) -> i64 {
        let validators = self.validators.read().await;
        validators
            .values()
            .filter(|v| v.active)
            .map(|v| v.power)
            .sum()
    }

    /// Check if an address is a validator
    pub async fn is_validator(&self, address: &[u8]) -> bool {
        let validators = self.validators.read().await;
        validators
            .get(address)
            .map(|v| v.active && v.power > 0)
            .unwrap_or(false)
    }

    /// Process evidence of misbehavior
    pub async fn slash_validator(
        &self,
        address: &[u8],
        slash_fraction: f64,
        height: u64,
    ) -> Result<()> {
        if !(0.0..=1.0).contains(&slash_fraction) {
            return Err(ValidatorError::InvalidValidator(
                "Invalid slash fraction".to_string(),
            ));
        }

        let validator_info = {
            let validators = self.validators.read().await;
            validators.get(address).cloned()
        };

        if let Some(validator) = validator_info {
            let current_power = validator.power;
            let slashed_power = (current_power as f64 * (1.0 - slash_fraction)) as i64;

            warn!(
                "Slashing validator {} by {}% (power: {} -> {})",
                hex::encode(address),
                slash_fraction * 100.0,
                current_power,
                slashed_power
            );

            self.update_validator(
                address.to_vec(),
                slashed_power,
                validator.pub_key.clone(),
                height,
            )
            .await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_validator_manager_basic() {
        let manager = ValidatorManager::new(100);

        // Add a validator
        let address = vec![1, 2, 3, 4];
        let pub_key = vec![5, 6, 7, 8];
        manager
            .update_validator(address.clone(), 100, pub_key.clone(), 1)
            .await
            .unwrap();

        // Check validator exists
        assert!(manager.is_validator(&address).await);

        // Get validators
        let validators = manager.get_validators().await;
        assert_eq!(validators.len(), 1);
        assert_eq!(validators[0].power, 100);

        // Remove validator
        manager.remove_validator(address.clone(), 2).await.unwrap();
        assert!(!manager.is_validator(&address).await);
    }

    #[tokio::test]
    async fn test_pending_updates() {
        let manager = ValidatorManager::new(100);

        // Add multiple updates
        manager
            .update_validator(vec![1], 100, vec![], 1)
            .await
            .unwrap();
        manager
            .update_validator(vec![2], 200, vec![], 1)
            .await
            .unwrap();

        // Take pending updates
        let updates = manager.take_pending_updates().await;
        assert_eq!(updates.len(), 2);

        // Pending should be empty now
        let updates2 = manager.take_pending_updates().await;
        assert_eq!(updates2.len(), 0);
    }

    #[tokio::test]
    async fn test_max_validators() {
        let manager = ValidatorManager::new(2);

        // Add two validators (at max)
        manager
            .update_validator(vec![1], 100, vec![], 1)
            .await
            .unwrap();
        manager
            .update_validator(vec![2], 200, vec![], 1)
            .await
            .unwrap();

        // Try to add third validator
        let result = manager.update_validator(vec![3], 300, vec![], 1).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_slash_validator() {
        let manager = ValidatorManager::new(100);

        // Add a validator with 1000 power
        let address = vec![1, 2, 3];
        manager
            .update_validator(address.clone(), 1000, vec![], 1)
            .await
            .unwrap();

        // Slash by 10%
        manager.slash_validator(&address, 0.1, 2).await.unwrap();

        // Check power reduced to 900
        let validator = manager.get_validator(&address).await.unwrap();
        assert_eq!(validator.power, 900);
    }

    #[tokio::test]
    async fn test_validator_persistence() {
        let manager = ValidatorManager::new(100);

        // Add multiple validators
        let validators = vec![
            (vec![1, 1, 1], 1000, vec![1, 2, 3]),
            (vec![2, 2, 2], 2000, vec![4, 5, 6]),
            (vec![3, 3, 3], 3000, vec![7, 8, 9]),
        ];

        for (addr, power, pubkey) in &validators {
            manager
                .update_validator(addr.clone(), *power, pubkey.clone(), 1)
                .await
                .unwrap();
        }

        // Save to store
        manager.save_validators().await.unwrap();

        // Create new manager with same store
        // We need to create a new MemStore since we can't clone the trait object
        let mut new_store = Box::new(MemStore::new()) as Box<dyn KVStore>;

        // Copy the validator data from the original store
        {
            let original_store = manager.store.read().await;

            // Copy validator count
            if let Some(count_bytes) = original_store.get(b"validators/count").unwrap() {
                new_store.set(b"validators/count", &count_bytes).unwrap();
            }

            // Copy addresses list
            if let Some(addr_bytes) = original_store.get(b"validators/addresses").unwrap() {
                new_store.set(b"validators/addresses", &addr_bytes).unwrap();

                // Copy each validator
                let addresses: Vec<String> = serde_json::from_slice(&addr_bytes).unwrap();
                for addr_hex in addresses {
                    let key = format!("validators/addr/{addr_hex}");
                    if let Some(val_bytes) = original_store.get(key.as_bytes()).unwrap() {
                        new_store.set(key.as_bytes(), &val_bytes).unwrap();
                    }
                }
            }
        }

        let new_manager = ValidatorManager::with_store(new_store, 100);

        // Load validators
        new_manager.load_validators().await.unwrap();

        // Verify all validators were loaded correctly
        for (addr, expected_power, expected_pubkey) in &validators {
            let validator = new_manager.get_validator(addr).await.unwrap();
            assert_eq!(validator.power, *expected_power);
            assert_eq!(validator.pub_key, *expected_pubkey);
        }

        // Verify count
        let all_validators = new_manager.get_validators().await;
        assert_eq!(all_validators.len(), 3);
    }

    #[tokio::test]
    async fn test_validator_persistence_backward_compatibility() {
        // Test that we can still load validators saved with the old format
        let store = Arc::new(RwLock::new(Box::new(MemStore::new()) as Box<dyn KVStore>));

        // Save validators in old format
        {
            let mut store_guard = store.write().await;

            // Save count
            store_guard
                .set(b"validators/count", &3u32.to_be_bytes())
                .unwrap();

            // Save validators with indexed keys (old format)
            let validators = vec![
                ValidatorInfo {
                    address: vec![1, 1, 1],
                    power: 1000,
                    pub_key: vec![1, 2, 3],
                    last_update_height: 1,
                    active: true,
                },
                ValidatorInfo {
                    address: vec![2, 2, 2],
                    power: 2000,
                    pub_key: vec![4, 5, 6],
                    last_update_height: 1,
                    active: true,
                },
                ValidatorInfo {
                    address: vec![3, 3, 3],
                    power: 3000,
                    pub_key: vec![7, 8, 9],
                    last_update_height: 1,
                    active: true,
                },
            ];

            for (i, validator) in validators.iter().enumerate() {
                let key = format!("validators/{i}");
                let val_bytes = serde_json::to_vec(validator).unwrap();
                store_guard.set(key.as_bytes(), &val_bytes).unwrap();
            }
        }

        // Load with new manager
        let store_box = {
            let store_guard = store.read().await;
            // Create a new MemStore and copy data
            let mut new_store = Box::new(MemStore::new()) as Box<dyn KVStore>;

            // Copy all old format data
            if let Some(count_bytes) = store_guard.get(b"validators/count").unwrap() {
                new_store.set(b"validators/count", &count_bytes).unwrap();
            }
            for i in 0..3 {
                let key = format!("validators/{i}");
                if let Some(val_bytes) = store_guard.get(key.as_bytes()).unwrap() {
                    new_store.set(key.as_bytes(), &val_bytes).unwrap();
                }
            }
            new_store
        };

        let manager = ValidatorManager::with_store(store_box, 100);
        manager.load_validators().await.unwrap();

        // Verify validators were loaded
        let all_validators = manager.get_validators().await;
        assert_eq!(all_validators.len(), 3);

        // Save again (should use new format)
        manager.save_validators().await.unwrap();

        // Create another manager and load
        let mut final_store = Box::new(MemStore::new()) as Box<dyn KVStore>;

        // Copy data from manager's store
        {
            let manager_store = manager.store.read().await;

            // Copy all relevant keys
            for key in [
                b"validators/count".as_slice(),
                b"validators/addresses".as_slice(),
            ] {
                if let Some(data) = manager_store.get(key).unwrap() {
                    final_store.set(key, &data).unwrap();
                }
            }

            // Copy validator data
            if let Some(addr_bytes) = manager_store.get(b"validators/addresses").unwrap() {
                let addresses: Vec<String> = serde_json::from_slice(&addr_bytes).unwrap();
                for addr_hex in addresses {
                    let key = format!("validators/addr/{addr_hex}");
                    if let Some(val_bytes) = manager_store.get(key.as_bytes()).unwrap() {
                        final_store.set(key.as_bytes(), &val_bytes).unwrap();
                    }
                }
            }
        }

        let new_manager = ValidatorManager::with_store(final_store, 100);
        new_manager.load_validators().await.unwrap();

        // Verify still works
        let all_validators = new_manager.get_validators().await;
        assert_eq!(all_validators.len(), 3);
    }
}
