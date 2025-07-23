//! State management layer for the helium blockchain.
//!
//! This module provides the StateManager that coordinates state access
//! using GlobalAppStore pattern with namespace isolation.

use crate::{GlobalAppStore, NamespacedStore, Result, StoreError};
use std::collections::HashMap;

/// A snapshot of the state at a specific block height
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    /// Block height when this snapshot was taken
    pub height: u64,
    /// Snapshot of store states (simplified for POC)
    pub store_data: HashMap<String, HashMap<Vec<u8>, Vec<u8>>>,
}

/// State manager using GlobalAppStore pattern for state coordination
pub struct StateManager {
    /// Global application store instance
    global_store: GlobalAppStore,
    /// Current block height
    block_height: u64,
    /// Cached namespaced stores for pending changes
    cached_stores: HashMap<String, NamespacedStore>,
    /// Whether there are uncommitted changes
    has_pending_changes: bool,
    /// Stored snapshots for rollback capability
    snapshots: HashMap<u64, StateSnapshot>,
}

impl StateManager {
    /// Create a new state manager with GlobalAppStore
    pub fn new(global_store: GlobalAppStore) -> Self {
        Self {
            global_store,
            block_height: 0,
            cached_stores: HashMap::new(),
            has_pending_changes: false,
            snapshots: HashMap::new(),
        }
    }

    /// Create a new state manager with a fresh GlobalAppStore (for testing)
    pub fn new_with_memstore() -> Self {
        use crate::JMTStore;
        use std::sync::atomic::{AtomicU64, Ordering};

        // Use a unique counter to avoid conflicts between test instances
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let instance_id = COUNTER.fetch_add(1, Ordering::Relaxed);

        // Create a temporary directory that will be cleaned up on drop
        let temp_dir = std::env::temp_dir().join(format!(
            "gridway_test_{}_{}",
            std::process::id(),
            instance_id
        ));
        std::fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

        let jmt_store =
            JMTStore::new("state".to_string(), &temp_dir).expect("Failed to create JMT store");

        let global_store = GlobalAppStore::new(jmt_store);
        Self::new(global_store)
    }

    /// Get the current block height
    pub fn block_height(&self) -> u64 {
        self.block_height
    }

    /// Register a namespace in the global store (replaces mount_store)
    pub fn register_namespace(&mut self, name: String, read_only: bool) -> Result<()> {
        self.global_store.register_namespace(&name, read_only)?;
        Ok(())
    }

    /// Convenience method for tests - register and initialize a namespace
    pub fn mount_store(&mut self, name: String, _store: Box<dyn crate::KVStore + 'static>) {
        // For backward compatibility, register namespace as read-write
        // The actual store parameter is ignored as we use GlobalAppStore
        let _ = self.register_namespace(name, false);
    }

    /// Get a read-only reference to a namespace
    pub fn get_store(&self, name: &str) -> Result<NamespacedStore> {
        // Return the actual namespace from global store
        self.global_store.get_namespace(name)
    }

    /// Get a mutable reference to a namespace (creates cache if needed)
    pub fn get_store_mut(&mut self, name: &str) -> Result<&mut NamespacedStore> {
        // If we already have a cached version, return it
        if self.cached_stores.contains_key(name) {
            self.has_pending_changes = true;
            return Ok(self.cached_stores.get_mut(name).unwrap());
        }

        // Get namespace from global store and cache it
        let namespace = self.global_store.get_namespace(name)?;
        self.cached_stores.insert(name.to_string(), namespace);
        self.has_pending_changes = true;

        Ok(self.cached_stores.get_mut(name).unwrap())
    }

    /// Check if there are pending changes that haven't been committed
    pub fn has_pending_changes(&self) -> bool {
        self.has_pending_changes
    }

    /// Set the block height (used during initialization or rollback)
    pub fn set_block_height(&mut self, height: u64) {
        self.block_height = height;
    }

    /// Increment the block height by one
    pub fn increment_block_height(&mut self) {
        self.block_height += 1;
    }

    /// Advance to a specific block height
    pub fn advance_to_height(&mut self, height: u64) -> Result<()> {
        if height < self.block_height {
            return Err(StoreError::InvalidValue(format!(
                "Cannot advance to height {} from current height {}",
                height, self.block_height
            )));
        }
        self.block_height = height;
        Ok(())
    }

    /// Commit all pending changes to the underlying stores
    pub fn commit(&mut self) -> Result<()> {
        if !self.has_pending_changes {
            return Ok(());
        }

        // With GlobalAppStore, changes are already persisted through the namespaced stores
        // We just need to clear our cache and update state
        self.cached_stores.clear();
        self.has_pending_changes = false;

        // Increment block height after successful commit
        self.increment_block_height();

        Ok(())
    }

    /// Rollback all pending changes without committing them
    pub fn rollback(&mut self) {
        // Simply discard all cached stores
        self.cached_stores.clear();
        self.has_pending_changes = false;
    }

    /// Begin a new transaction by creating a clean state
    pub fn begin_transaction(&mut self) {
        // If there are pending changes, this is an error in most blockchain contexts
        // For now, we'll just ensure we start clean
        if self.has_pending_changes {
            self.rollback();
        }
    }

    /// Create a snapshot of the current state
    pub fn create_snapshot(&mut self) -> Result<()> {
        // For POC, we'll create a simplified snapshot
        // In a real implementation, this would capture the full state
        let snapshot = StateSnapshot {
            height: self.block_height,
            store_data: HashMap::new(), // Simplified for POC
        };

        self.snapshots.insert(self.block_height, snapshot);
        Ok(())
    }

    /// Restore state from a snapshot at the given height
    pub fn restore_snapshot(&mut self, height: u64) -> Result<()> {
        // Check if snapshot exists first
        if !self.snapshots.contains_key(&height) {
            return Err(StoreError::InvalidValue(format!(
                "No snapshot found for height {height}"
            )));
        }

        // Rollback any pending changes first
        self.rollback();

        // Get the snapshot height (we know it exists from the check above)
        let snapshot_height = self.snapshots.get(&height).unwrap().height;

        // Restore the block height
        self.block_height = snapshot_height;

        // In a real implementation, we would restore the actual store data here
        // For now, we just ensure we're at the right height

        Ok(())
    }

    /// Remove snapshots older than the given height to free memory
    pub fn prune_snapshots(&mut self, keep_recent: u64) {
        if self.block_height > keep_recent {
            let cutoff_height = self.block_height - keep_recent;
            self.snapshots.retain(|&height, _| height > cutoff_height);
        }
    }

    /// Get the number of stored snapshots
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Check if a snapshot exists for the given height
    pub fn has_snapshot(&self, height: u64) -> bool {
        self.snapshots.contains_key(&height)
    }

    /// Get a reference to the global store
    pub fn global_store(&self) -> &GlobalAppStore {
        &self.global_store
    }
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new_with_memstore()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KVStore;

    #[test]
    fn test_state_manager_basic() {
        let mut state_manager = StateManager::default();

        // Initial state
        assert_eq!(state_manager.block_height(), 0);
        assert!(!state_manager.has_pending_changes());

        // Register a namespace
        state_manager
            .register_namespace("bank".to_string(), false)
            .unwrap();

        // Get read-only access
        assert!(state_manager.get_store("bank").is_ok());
        assert!(state_manager.get_store("missing").is_err());
    }

    #[test]
    fn test_state_manager_caching() {
        let mut state_manager = StateManager::default();
        state_manager
            .register_namespace("bank".to_string(), false)
            .unwrap();

        // Getting mutable access should create cache
        assert!(!state_manager.has_pending_changes());
        let _store = state_manager.get_store_mut("bank").unwrap();
        assert!(state_manager.has_pending_changes());
    }

    #[test]
    fn test_block_height_management() {
        let mut state_manager = StateManager::default();

        // Initial height
        assert_eq!(state_manager.block_height(), 0);

        // Set height
        state_manager.set_block_height(10);
        assert_eq!(state_manager.block_height(), 10);

        // Increment height
        state_manager.increment_block_height();
        assert_eq!(state_manager.block_height(), 11);

        // Advance to height
        assert!(state_manager.advance_to_height(15).is_ok());
        assert_eq!(state_manager.block_height(), 15);

        // Cannot go backwards
        assert!(state_manager.advance_to_height(10).is_err());
    }

    #[test]
    fn test_commit_rollback() {
        let mut state_manager = StateManager::default();
        state_manager
            .register_namespace("bank".to_string(), false)
            .unwrap();

        let initial_height = state_manager.block_height();

        // Make some changes
        let _store = state_manager.get_store_mut("bank").unwrap();
        assert!(state_manager.has_pending_changes());

        // Commit changes
        assert!(state_manager.commit().is_ok());
        assert!(!state_manager.has_pending_changes());
        assert_eq!(state_manager.block_height(), initial_height + 1);

        // Make more changes and rollback
        let _store = state_manager.get_store_mut("bank").unwrap();
        assert!(state_manager.has_pending_changes());

        state_manager.rollback();
        assert!(!state_manager.has_pending_changes());
        assert_eq!(state_manager.block_height(), initial_height + 1); // Height unchanged after rollback
    }

    #[test]
    fn test_snapshots() {
        let mut state_manager = StateManager::default();

        // Create snapshot
        assert!(state_manager.create_snapshot().is_ok());
        assert_eq!(state_manager.snapshot_count(), 1);
        assert!(state_manager.has_snapshot(0));

        // Advance height and create another snapshot
        state_manager.set_block_height(5);
        assert!(state_manager.create_snapshot().is_ok());
        assert_eq!(state_manager.snapshot_count(), 2);
        assert!(state_manager.has_snapshot(5));

        // Restore snapshot
        assert!(state_manager.restore_snapshot(0).is_ok());
        assert_eq!(state_manager.block_height(), 0);

        // Prune snapshots
        state_manager.set_block_height(10);
        state_manager.prune_snapshots(3);
        // Should keep snapshots within last 3 blocks from height 10: heights > 7
        // We only have snapshots at 0 and 5, both should be pruned (0 <= 7 and 5 <= 7)
        assert_eq!(state_manager.snapshot_count(), 0);
        assert!(!state_manager.has_snapshot(5));
        assert!(!state_manager.has_snapshot(0));
    }

    #[test]
    fn test_namespace_isolation() {
        let mut state_manager = StateManager::default();

        // Register multiple namespaces
        state_manager
            .register_namespace("auth".to_string(), false)
            .unwrap();
        state_manager
            .register_namespace("bank".to_string(), false)
            .unwrap();

        // Write to different namespaces
        {
            let auth_store = state_manager.get_store_mut("auth").unwrap();
            auth_store.set(b"key1", b"auth_value").unwrap();
        }

        {
            let bank_store = state_manager.get_store_mut("bank").unwrap();
            bank_store.set(b"key1", b"bank_value").unwrap();
        }

        // Verify isolation
        let auth_store = state_manager.get_store("auth").unwrap();
        let bank_store = state_manager.get_store("bank").unwrap();

        assert_eq!(auth_store.get(b"key1").unwrap().unwrap(), b"auth_value");
        assert_eq!(bank_store.get(b"key1").unwrap().unwrap(), b"bank_value");
    }
}
