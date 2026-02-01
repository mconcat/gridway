//! Simple hash-based Merkle state store.
//!
//! Replaces JMT+RocksDB with a deterministic SHA-256 hash computation
//! over sorted state entries. Suitable for consensus-critical state root
//! agreement without external dependencies.

use crate::{Hash, KVStore, Result, StoreError};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

/// A Merkle state store that computes deterministic state root hashes
/// from the key-value state using sorted SHA-256 hashing.
pub struct MerkleStore {
    /// The authoritative state (committed data)
    committed: BTreeMap<Vec<u8>, Vec<u8>>,
    /// Pending changes not yet committed
    pending: HashMap<Vec<u8>, Option<Vec<u8>>>,
    /// Current version (incremented on commit)
    version: u64,
    /// Store name
    name: String,
}

impl MerkleStore {
    /// Create a new MerkleStore
    pub fn new(name: String) -> Self {
        Self {
            committed: BTreeMap::new(),
            pending: HashMap::new(),
            version: 0,
            name,
        }
    }

    /// Get current version
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Compute the state root hash from all committed + pending state.
    /// Uses a sorted iteration over all keys to ensure determinism.
    pub fn root_hash(&self) -> Hash {
        let mut hasher = Sha256::new();

        // Merge committed and pending into a sorted view
        let mut merged = self.committed.clone();
        for (k, v) in &self.pending {
            match v {
                Some(val) => { merged.insert(k.clone(), val.clone()); }
                None => { merged.remove(k); }
            }
        }

        // Hash version
        hasher.update(self.version.to_be_bytes());

        // Hash all key-value pairs in sorted order (BTreeMap guarantees this)
        for (k, v) in &merged {
            hasher.update((k.len() as u32).to_be_bytes());
            hasher.update(k);
            hasher.update((v.len() as u32).to_be_bytes());
            hasher.update(v);
        }

        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    /// Commit pending changes — flushes pending into committed state,
    /// increments version, returns the new state root hash.
    pub fn commit(&mut self) -> Result<Hash> {
        // Apply pending changes to committed state
        for (k, v) in self.pending.drain() {
            match v {
                Some(val) => { self.committed.insert(k, val); }
                None => { self.committed.remove(&k); }
            }
        }
        self.version += 1;
        Ok(self.root_hash())
    }
}

impl KVStore for MerkleStore {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // Check pending first
        if let Some(pending_val) = self.pending.get(key) {
            return Ok(pending_val.clone());
        }
        // Then committed
        Ok(self.committed.get(key).cloned())
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.pending.insert(key.to_vec(), Some(value.to_vec()));
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.pending.insert(key.to_vec(), None);
        Ok(())
    }

    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        // Merge committed and pending
        let mut merged = BTreeMap::new();
        for (k, v) in &self.committed {
            if k.starts_with(prefix) {
                merged.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in &self.pending {
            if k.starts_with(prefix) {
                match v {
                    Some(val) => { merged.insert(k.clone(), val.clone()); }
                    None => { merged.remove(k); }
                }
            }
        }
        Box::new(merged.into_iter().collect::<Vec<_>>().into_iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merkle_store_basic() {
        let mut store = MerkleStore::new("test".to_string());
        assert!(store.get(b"key1").unwrap().is_none());

        store.set(b"key1", b"value1").unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        let hash1 = store.commit().unwrap();
        assert_ne!(hash1, [0u8; 32]);
        assert_eq!(store.version(), 1);
    }

    #[test]
    fn test_merkle_store_determinism() {
        // Same operations should produce same hash
        let run = || {
            let mut store = MerkleStore::new("test".to_string());
            store.set(b"alice", b"1000").unwrap();
            store.set(b"bob", b"2000").unwrap();
            store.commit().unwrap()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn test_merkle_store_hash_changes() {
        let mut store = MerkleStore::new("test".to_string());
        store.set(b"key1", b"value1").unwrap();
        let hash1 = store.commit().unwrap();

        store.set(b"key1", b"value2").unwrap();
        let hash2 = store.commit().unwrap();

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_merkle_store_delete() {
        let mut store = MerkleStore::new("test".to_string());
        store.set(b"key1", b"value1").unwrap();
        store.commit().unwrap();

        store.delete(b"key1").unwrap();
        assert!(store.get(b"key1").unwrap().is_none());
        store.commit().unwrap();
    }

    #[test]
    fn test_merkle_store_prefix_iterator() {
        let mut store = MerkleStore::new("test".to_string());
        store.set(b"bank:alice", b"1000").unwrap();
        store.set(b"bank:bob", b"2000").unwrap();
        store.set(b"auth:alice", b"nonce1").unwrap();
        store.commit().unwrap();

        let bank_items: Vec<_> = store.prefix_iterator(b"bank:").collect();
        assert_eq!(bank_items.len(), 2);
        assert_eq!(bank_items[0].0, b"bank:alice".to_vec());
        assert_eq!(bank_items[1].0, b"bank:bob".to_vec());
    }
}
