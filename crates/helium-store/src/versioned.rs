//! Versioned Merkle key-value store implementation with proof generation support.
//!
//! This module provides a trait and implementation for versioned stores that
//! support Merkle proofs, enabling state verification and IBC compatibility.

use crate::{Hash, JMTStore, KVStore, Result, StoreError};
use std::collections::{BTreeMap, HashMap};

/// A trait for versioned Merkle stores that support proof generation
pub trait VersionedMerkleKvStore: KVStore {
    /// Get the current version (block height)
    fn version(&self) -> u64;
    
    /// Set the current version
    fn set_version(&mut self, version: u64);
    
    /// Get the root hash at the current version
    fn root_hash(&self) -> Hash;
    
    /// Get a value with its Merkle proof at the current version
    fn get_with_proof(&self, key: &[u8]) -> Result<(Option<Vec<u8>>, Vec<u8>)>;
    
    /// Verify a Merkle proof for a key-value pair
    fn verify_proof(&self, key: &[u8], value: Option<&[u8]>, proof: &[u8]) -> Result<bool>;
    
    /// Get the root hash at a specific version
    fn root_hash_at_version(&self, version: u64) -> Result<Hash>;
    
    /// Commit the current state and advance to a new version
    fn commit(&mut self) -> Result<Hash>;
    
    /// Rollback to a previous version (discard uncommitted changes)
    fn rollback(&mut self, version: u64) -> Result<()>;
    
    /// Prune old versions to save storage space
    fn prune(&mut self, keep_recent: u64) -> Result<()>;
}

/// Versioned store implementation that maintains multiple versions of state
pub struct VersionedStore {
    /// Map of version to store state at that version
    versions: BTreeMap<u64, StoreSnapshot>,
    /// Current working version
    current_version: u64,
    /// Pending changes not yet committed
    pending: HashMap<Vec<u8>, Option<Vec<u8>>>,
    /// Store name for identification
    name: String,
}

/// A snapshot of the store state at a specific version
#[derive(Clone)]
struct StoreSnapshot {
    /// The data at this version
    data: HashMap<Vec<u8>, Vec<u8>>,
    /// Root hash at this version
    root_hash: Hash,
}

impl VersionedStore {
    /// Create a new versioned store
    pub fn new(name: String) -> Self {
        let mut versions = BTreeMap::new();
        
        // Initialize with empty state at version 0
        let initial_snapshot = StoreSnapshot {
            data: HashMap::new(),
            root_hash: [0u8; 32],
        };
        versions.insert(0, initial_snapshot);
        
        Self {
            versions,
            current_version: 0,
            pending: HashMap::new(),
            name,
        }
    }
    
    /// Get the current snapshot (including pending changes)
    fn current_snapshot(&self) -> Result<StoreSnapshot> {
        let base_snapshot = self.versions.get(&self.current_version)
            .ok_or_else(|| StoreError::InvalidKey(format!("Version {} not found", self.current_version)))?;
        
        // Apply pending changes to create current view
        let mut data = base_snapshot.data.clone();
        for (key, value_opt) in &self.pending {
            if let Some(value) = value_opt {
                data.insert(key.clone(), value.clone());
            } else {
                data.remove(key);
            }
        }
        
        let root_hash = self.compute_root_hash(&data);
        Ok(StoreSnapshot {
            data,
            root_hash,
        })
    }
    
    /// Compute root hash from data
    fn compute_root_hash(&self, data: &HashMap<Vec<u8>, Vec<u8>>) -> Hash {
        // Return empty hash for empty data
        if data.is_empty() {
            return [0u8; 32];
        }
        
        use sha2::{Digest, Sha256};
        
        let mut hasher = Sha256::new();
        
        // Hash store name and version
        hasher.update(&self.name.as_bytes());
        hasher.update(&self.current_version.to_be_bytes());
        
        // Hash all key-value pairs in sorted order
        let mut entries: Vec<_> = data.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        
        for (key, value) in entries {
            hasher.update(key);
            hasher.update(value);
        }
        
        hasher.finalize().into()
    }
    
    /// Generate a simple proof for a key
    fn generate_proof(&self, key: &[u8], snapshot: &StoreSnapshot) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        
        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.update(&snapshot.root_hash);
        hasher.update(&self.current_version.to_be_bytes());
        
        // In a real implementation, this would include the Merkle path
        // For now, we use a simplified proof
        if let Some(value) = snapshot.data.get(key) {
            hasher.update(value);
        }
        
        hasher.finalize().to_vec()
    }
}

impl KVStore for VersionedStore {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // Check pending changes first
        if let Some(value_opt) = self.pending.get(key) {
            return Ok(value_opt.clone());
        }
        
        // Then check committed state
        let snapshot = self.versions.get(&self.current_version)
            .ok_or_else(|| StoreError::InvalidKey(format!("Version {} not found", self.current_version)))?;
        
        Ok(snapshot.data.get(key).cloned())
    }
    
    fn set(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        self.pending.insert(key, Some(value));
        Ok(())
    }
    
    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.pending.insert(key.to_vec(), None);
        Ok(())
    }
    
    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>> {
        let snapshot = match self.current_snapshot() {
            Ok(s) => s,
            Err(_) => return Box::new(std::iter::empty()),
        };
        
        let mut items: Vec<_> = snapshot.data.into_iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .collect();
        
        items.sort_by(|(a, _), (b, _)| a.cmp(b));
        Box::new(items.into_iter())
    }
}

impl VersionedMerkleKvStore for VersionedStore {
    fn version(&self) -> u64 {
        self.current_version
    }
    
    fn set_version(&mut self, version: u64) {
        if self.versions.contains_key(&version) {
            self.current_version = version;
            self.pending.clear(); // Clear pending when switching versions
        }
    }
    
    fn root_hash(&self) -> Hash {
        match self.current_snapshot() {
            Ok(snapshot) => snapshot.root_hash,
            Err(_) => [0u8; 32],
        }
    }
    
    fn get_with_proof(&self, key: &[u8]) -> Result<(Option<Vec<u8>>, Vec<u8>)> {
        let snapshot = self.current_snapshot()?;
        let value = snapshot.data.get(key).cloned();
        let proof = self.generate_proof(key, &snapshot);
        
        Ok((value, proof))
    }
    
    fn verify_proof(&self, key: &[u8], value: Option<&[u8]>, proof: &[u8]) -> Result<bool> {
        let snapshot = self.current_snapshot()?;
        let expected_proof = self.generate_proof(key, &snapshot);
        
        // Verify the proof matches
        if proof != expected_proof {
            return Ok(false);
        }
        
        // Verify the value matches
        match (snapshot.data.get(key), value) {
            (Some(stored), Some(provided)) => Ok(stored == provided),
            (None, None) => Ok(true),
            _ => Ok(false),
        }
    }
    
    fn root_hash_at_version(&self, version: u64) -> Result<Hash> {
        let snapshot = self.versions.get(&version)
            .ok_or_else(|| StoreError::InvalidKey(format!("Version {} not found", version)))?;
        
        Ok(snapshot.root_hash)
    }
    
    fn commit(&mut self) -> Result<Hash> {
        if self.pending.is_empty() {
            // Nothing to commit, return current root hash
            return Ok(self.root_hash());
        }
        
        // Create new snapshot with pending changes applied
        let new_snapshot = self.current_snapshot()?;
        let new_version = self.current_version + 1;
        let root_hash = new_snapshot.root_hash;
        
        // Store the new version
        self.versions.insert(new_version, new_snapshot);
        self.current_version = new_version;
        self.pending.clear();
        
        Ok(root_hash)
    }
    
    fn rollback(&mut self, version: u64) -> Result<()> {
        if !self.versions.contains_key(&version) {
            return Err(StoreError::InvalidKey(format!("Version {} not found", version)));
        }
        
        self.current_version = version;
        self.pending.clear();
        
        Ok(())
    }
    
    fn prune(&mut self, keep_recent: u64) -> Result<()> {
        if self.current_version <= keep_recent {
            // Nothing to prune
            return Ok(());
        }
        
        let cutoff_version = self.current_version - keep_recent;
        
        // Keep all versions after cutoff
        self.versions = self.versions.split_off(&cutoff_version);
        
        // Always keep at least the cutoff version
        if !self.versions.contains_key(&cutoff_version) && cutoff_version > 0 {
            // This shouldn't happen, but handle it gracefully
            return Err(StoreError::InvalidKey("Pruning would remove all versions".to_string()));
        }
        
        Ok(())
    }
}

/// Wrapper to make JMTStore implement VersionedMerkleKvStore
pub struct VersionedJMTWrapper {
    /// Map of version to JMT store at that version
    stores: BTreeMap<u64, JMTStore>,
    /// Current version
    current_version: u64,
    /// Pending changes for current version
    pending: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    /// Store name
    name: String,
}

impl VersionedJMTWrapper {
    /// Create a new versioned JMT wrapper
    pub fn new(name: String) -> Self {
        let mut stores = BTreeMap::new();
        stores.insert(0, JMTStore::new(name.clone()));
        
        Self {
            stores,
            current_version: 0,
            pending: Vec::new(),
            name,
        }
    }
    
    /// Get the current store
    fn current_store(&self) -> Result<&JMTStore> {
        self.stores.get(&self.current_version)
            .ok_or_else(|| StoreError::InvalidKey(format!("Version {} not found", self.current_version)))
    }
}

impl KVStore for VersionedJMTWrapper {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.current_store()?.get(key)
    }
    
    fn set(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        self.pending.push((key, Some(value)));
        Ok(())
    }
    
    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.pending.push((key.to_vec(), None));
        Ok(())
    }
    
    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>> {
        match self.current_store() {
            Ok(store) => store.prefix_iterator(prefix),
            Err(_) => Box::new(std::iter::empty()),
        }
    }
}

impl VersionedMerkleKvStore for VersionedJMTWrapper {
    fn version(&self) -> u64 {
        self.current_version
    }
    
    fn set_version(&mut self, version: u64) {
        if self.stores.contains_key(&version) {
            self.current_version = version;
            self.pending.clear();
        }
    }
    
    fn root_hash(&self) -> Hash {
        match self.current_store() {
            Ok(store) => store.root_hash(),
            Err(_) => [0u8; 32],
        }
    }
    
    fn get_with_proof(&self, key: &[u8]) -> Result<(Option<Vec<u8>>, Vec<u8>)> {
        self.current_store()?.get_with_proof(key)
    }
    
    fn verify_proof(&self, key: &[u8], value: Option<&[u8]>, proof: &[u8]) -> Result<bool> {
        self.current_store()?.verify_proof(key, value, proof)
    }
    
    fn root_hash_at_version(&self, version: u64) -> Result<Hash> {
        let store = self.stores.get(&version)
            .ok_or_else(|| StoreError::InvalidKey(format!("Version {} not found", version)))?;
        
        Ok(store.root_hash())
    }
    
    fn commit(&mut self) -> Result<Hash> {
        if self.pending.is_empty() {
            return Ok(self.root_hash());
        }
        
        // Take pending changes to avoid borrow issues
        let pending = std::mem::take(&mut self.pending);
        let new_version = self.current_version + 1;
        let name = self.name.clone();
        
        // Clone current store's data before applying changes
        let current_store = self.current_store()?;
        let mut new_data = current_store.data.clone();
        
        // Apply pending changes to the cloned data
        for (key, value_opt) in pending {
            if let Some(value) = value_opt {
                new_data.insert(key, value);
            } else {
                new_data.remove(&key);
            }
        }
        
        // Create new store with updated data
        let mut new_store = JMTStore::new_with_version(name, new_version);
        new_store.data = new_data;
        new_store.root_hash = new_store.compute_root_hash();
        
        self.stores.insert(new_version, new_store);
        self.current_version = new_version;
        
        Ok(self.stores[&new_version].root_hash)
    }
    
    fn rollback(&mut self, version: u64) -> Result<()> {
        if !self.stores.contains_key(&version) {
            return Err(StoreError::InvalidKey(format!("Version {} not found", version)));
        }
        
        self.current_version = version;
        self.pending.clear();
        
        Ok(())
    }
    
    fn prune(&mut self, keep_recent: u64) -> Result<()> {
        if self.current_version <= keep_recent {
            return Ok(());
        }
        
        let cutoff_version = self.current_version - keep_recent;
        self.stores = self.stores.split_off(&cutoff_version);
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_versioned_store_basic() {
        let store = VersionedStore::new("test".to_string());
        
        assert_eq!(store.version(), 0);
        assert_eq!(store.root_hash(), [0u8; 32]);
    }
    
    #[test]
    fn test_versioned_store_commit() {
        let mut store = VersionedStore::new("test".to_string());
        
        // Add some data
        store.set(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        store.set(b"key2".to_vec(), b"value2".to_vec()).unwrap();
        
        // Data should be available immediately
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        
        // Commit changes
        let root_hash = store.commit().unwrap();
        assert_ne!(root_hash, [0u8; 32]);
        assert_eq!(store.version(), 1);
        
        // Data should still be available after commit
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
    }
    
    #[test]
    fn test_versioned_store_rollback() {
        let mut store = VersionedStore::new("test".to_string());
        
        // Commit some data at version 1
        store.set(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        store.commit().unwrap();
        
        // Make changes at version 2
        store.set(b"key2".to_vec(), b"value2".to_vec()).unwrap();
        store.commit().unwrap();
        
        assert_eq!(store.version(), 2);
        assert!(store.get(b"key2").unwrap().is_some());
        
        // Rollback to version 1
        store.rollback(1).unwrap();
        assert_eq!(store.version(), 1);
        assert!(store.get(b"key2").unwrap().is_none());
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
    }
    
    #[test]
    fn test_versioned_store_proof() {
        let mut store = VersionedStore::new("test".to_string());
        
        // Set some data
        store.set(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        store.commit().unwrap();
        
        // Get with proof
        let (value, proof) = store.get_with_proof(b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
        assert!(!proof.is_empty());
        
        // Verify proof
        assert!(store.verify_proof(b"key1", Some(b"value1"), &proof).unwrap());
        assert!(!store.verify_proof(b"key1", Some(b"wrong"), &proof).unwrap());
    }
    
    #[test]
    fn test_versioned_store_prune() {
        let mut store = VersionedStore::new("test".to_string());
        
        // Create multiple versions
        for i in 1..=5 {
            store.set(format!("key{}", i).as_bytes().to_vec(), 
                     format!("value{}", i).as_bytes().to_vec()).unwrap();
            store.commit().unwrap();
        }
        
        assert_eq!(store.version(), 5);
        
        // Prune old versions, keeping only last 2
        store.prune(2).unwrap();
        
        // Should not be able to rollback to version 2
        assert!(store.rollback(2).is_err());
        
        // Should be able to rollback to version 3
        assert!(store.rollback(3).is_ok());
    }
    
    #[test]
    fn test_versioned_jmt_wrapper() {
        let mut store = VersionedJMTWrapper::new("test".to_string());
        
        // Test basic operations
        assert_eq!(store.version(), 0);
        
        // Set and commit
        store.set(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        let root_hash = store.commit().unwrap();
        assert_ne!(root_hash, [0u8; 32]);
        assert_eq!(store.version(), 1);
        
        // Test rollback
        store.set(b"key2".to_vec(), b"value2".to_vec()).unwrap();
        store.commit().unwrap();
        
        store.rollback(1).unwrap();
        assert_eq!(store.version(), 1);
        assert!(store.get(b"key2").unwrap().is_none());
    }
    
    #[test]
    fn test_root_hash_at_version() {
        let mut store = VersionedStore::new("test".to_string());
        
        // Get initial root hash
        let v0_hash = store.root_hash_at_version(0).unwrap();
        assert_eq!(v0_hash, [0u8; 32]);
        
        // Commit some data
        store.set(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        store.commit().unwrap();
        let v1_hash = store.root_hash_at_version(1).unwrap();
        
        // Add more data
        store.set(b"key2".to_vec(), b"value2".to_vec()).unwrap();
        store.commit().unwrap();
        let v2_hash = store.root_hash_at_version(2).unwrap();
        
        // All hashes should be different
        assert_ne!(v0_hash, v1_hash);
        assert_ne!(v1_hash, v2_hash);
        assert_ne!(v0_hash, v2_hash);
    }
}