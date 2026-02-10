//! Patricia Merkle Trie state store backed by Parity's trie-db.
//!
//! Uses SHA-256 as the hash function and a no-extension trie layout.
//! The underlying storage is `MemoryDB` from the `memory-db` crate —
//! suitable for experiments. For production, swap in a persistent
//! `HashDB` implementation over RocksDB / sled / etc.

use crate::{Hash, KVStore, Result, StoreError};
use crate::persistent::SledBackend;
use hash_db::{HashDB, Hasher};
use serde::{Serialize, Deserialize};
use memory_db::{HashKey, MemoryDB};
use reference_trie::GenericNoExtensionLayout;
use sha2::{Digest, Sha256};
use std::path::Path;
use trie_db::{DBValue, Trie, TrieDBBuilder, TrieDBMutBuilder, TrieMut};

// ─── SHA-256 Hasher for hash-db ──────────────────────────────────────────────

/// A thin `std::hash::Hasher` that hashes `[u8; 32]` for use in HashMaps.
/// Only the first 8 bytes are folded into the u64 output.
#[derive(Default)]
pub struct Sha256StdHasher {
    state: u64,
}

impl std::hash::Hasher for Sha256StdHasher {
    fn finish(&self) -> u64 {
        self.state
    }

    fn write(&mut self, bytes: &[u8]) {
        // Simple fold — sufficient for HashMap distribution of 32-byte keys.
        for chunk in bytes.chunks(8) {
            let mut buf = [0u8; 8];
            buf[..chunk.len()].copy_from_slice(chunk);
            self.state ^= u64::from_le_bytes(buf);
        }
    }
}

/// SHA-256 hasher implementing `hash_db::Hasher`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha256Hasher;

impl Hasher for Sha256Hasher {
    type Out = [u8; 32];
    type StdHasher = Sha256StdHasher;
    const LENGTH: usize = 32;

    fn hash(x: &[u8]) -> [u8; 32] {
        let result = Sha256::digest(x);
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        out
    }
}

// ─── Trie layout ─────────────────────────────────────────────────────────────

/// Gridway trie layout: SHA-256 hasher, no extension nodes.
pub type GridwayTrieLayout = GenericNoExtensionLayout<Sha256Hasher>;

/// Convenience alias for the `MemoryDB` parameterised with our hasher.
type GridwayMemoryDB = MemoryDB<Sha256Hasher, HashKey<Sha256Hasher>, DBValue>;

// ─── MerkleStore ─────────────────────────────────────────────────────────────


/// A snapshot of the entire MerkleStore state.
/// Can be serialized/deserialized for state sync between nodes.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StateSnapshot {
    /// All key-value pairs in the trie.
    pub entries: Vec<(Vec<u8>, Vec<u8>)>,
    /// The Merkle root hash of the trie.
    pub root_hash: Hash,
    /// The version number (number of commits).
    pub version: u64,
}

/// A Patricia Merkle Trie state store.
///
/// Replaces the earlier flat-hash BTreeMap placeholder with a proper
/// Merkle Patricia Trie via Parity's `trie-db`.
///
/// * `get` / `set` / `delete` mutate the trie directly.
/// * `root_hash()` returns the current Merkle root.
/// * `commit()` snapshots the trie version and returns the root hash.
pub struct MerkleStore {
    /// In-memory hash-addressed node database.
    db: GridwayMemoryDB,
    /// Current trie root.
    root: Hash,
    /// Current version (incremented on commit).
    version: u64,
    /// Store name (for logging / debugging).
    #[allow(dead_code)]
    name: String,
    /// Optional sled persistence backend.
    persistence: Option<SledBackend>,
}

impl MerkleStore {
    /// Create a new, empty `MerkleStore`.
    pub fn new(name: String) -> Self {
        let mut db = GridwayMemoryDB::default();
        let mut root = Hash::default();

        // Initialise an empty trie — this sets root to the hash of the empty trie.
        {
            let _ = TrieDBMutBuilder::<GridwayTrieLayout>::new(&mut db, &mut root).build();
            // The builder's drop commits the empty trie automatically.
        }

        Self {
            db,
            root,
            version: 0,
            name,
            persistence: None,
        }
    }

    /// Create a new `MerkleStore` with sled persistence at the given path.
    ///
    /// If the database already contains state, it will be loaded automatically.
    /// Otherwise, an empty trie is initialized and persisted.
    pub fn with_persistence(name: String, path: &Path) -> Result<Self> {
        let backend = SledBackend::open(path)?;

        let store = if backend.has_state()? {
            // Load existing state from disk
            let mut store = Self::new(name);
            store.persistence = Some(backend);
            store.load_from_disk()?;
            store
        } else {
            // Fresh database — initialize empty trie and persist it
            let mut store = Self::new(name);
            store.persistence = Some(backend);
            store.flush_to_disk()?;
            store
        };

        Ok(store)
    }

    /// Flush the current memory-db state to sled.
    ///
    /// Writes all trie node entries from memory-db to the sled backend,
    /// along with the current root hash and version.
    pub fn flush_to_disk(&self) -> Result<()> {
        let backend = self.persistence.as_ref()
            .ok_or_else(|| StoreError::BackendError("no persistence backend configured".into()))?;

        // Extract all entries from memory-db by iterating its internal data.
        // memory-db stores data as HashMap<hash, (value, rc)> — we serialize
        // each entry as key=hash_bytes, value=node_bytes.
        //
        // We use the HashDB::keys() method to get all keys, then get() each one.
        let keys = self.db.keys();
        let mut entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(keys.len());

        for (key, rc) in &keys {
            if *rc > 0 {
                // Only persist entries with positive reference count
                if let Some(value) = HashDB::<Sha256Hasher, _>::get(&self.db, key, hash_db::EMPTY_PREFIX) {
                    let entry_key = key.as_ref().to_vec();
                    entries.push((entry_key, value));
                }
            }
        }

        // Clear and rewrite all nodes
        backend.clear_nodes()?;
        let refs: Vec<(&[u8], &[u8])> = entries.iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect();
        backend.write_nodes(&refs)?;

        // Store metadata
        backend.set_root(&self.root)?;
        backend.set_version(self.version)?;
        backend.flush()?;

        log::debug!(
            "flushed {} trie nodes to disk (root={}, version={})",
            entries.len(),
            hex::encode(&self.root),
            self.version
        );

        Ok(())
    }

    /// Load state from the sled backend into memory-db.
    ///
    /// Replaces the current in-memory state entirely. Returns the
    /// restored root hash.
    pub fn load_from_disk(&mut self) -> Result<[u8; 32]> {
        let backend = self.persistence.as_ref()
            .ok_or_else(|| StoreError::BackendError("no persistence backend configured".into()))?;

        let root = backend.get_root()?
            .ok_or_else(|| StoreError::BackendError("no root hash in sled database".into()))?;
        let version = backend.get_version()?
            .unwrap_or(0);
        let entries = backend.read_all_nodes()?;

        // Rebuild memory-db from sled entries
        let mut db = GridwayMemoryDB::default();

        // First initialize an empty trie to set up the DB properly
        let mut new_root = Hash::default();
        {
            let _ = TrieDBMutBuilder::<GridwayTrieLayout>::new(&mut db, &mut new_root).build();
        }

        // Insert all nodes from disk into memory-db using emplace
        for (key, value) in &entries {
            if key.len() == 32 {
                let mut hash = [0u8; 32];
                hash.copy_from_slice(key);
                HashDB::<Sha256Hasher, _>::emplace(&mut db, hash, hash_db::EMPTY_PREFIX, value.clone());
            }
        }

        self.db = db;
        self.root = root;
        self.version = version;

        log::info!(
            "loaded {} trie nodes from disk (root={}, version={})",
            entries.len(),
            hex::encode(&self.root),
            self.version
        );

        Ok(root)
    }

    /// Flush pending state and cleanly close the sled backend.
    pub fn close(&self) -> Result<()> {
        if self.persistence.is_some() {
            self.flush_to_disk()?;
        }
        Ok(())
    }

    /// Returns whether persistence is enabled.
    pub fn has_persistence(&self) -> bool {
        self.persistence.is_some()
    }

    /// Get the current version number.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Get the current Merkle root hash.
    pub fn root_hash(&self) -> Hash {
        self.root
    }

    /// Commit the current state: increment version, return the root hash.
    ///
    /// Since trie-db computes the Merkle root on every mutation (via
    /// `TrieDBMut` drop/commit), there are no pending changes to flush.
    /// This method simply bumps the version counter and returns the
    /// current root.
    pub fn commit(&mut self) -> Result<Hash> {
        self.version += 1;

        // Auto-flush to disk if persistence is enabled
        if self.persistence.is_some() {
            self.flush_to_disk()?;
        }

        Ok(self.root)
    }

    /// Export all key-value pairs from the trie.
    /// Returns sorted Vec of (key, value) pairs.
    pub fn export_state(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.prefix_iterator(b"").collect()
    }

    /// Import key-value pairs into a fresh trie, returning the new root hash.
    /// Clears existing state first.
    pub fn import_state(&mut self, entries: &[(Vec<u8>, Vec<u8>)]) -> Result<Hash> {
        // Create fresh MemoryDB and root
        self.db = GridwayMemoryDB::default();
        self.root = Hash::default();
        // Build empty trie
        {
            let _ = TrieDBMutBuilder::<GridwayTrieLayout>::new(&mut self.db, &mut self.root).build();
        }
        // Insert all entries
        for (key, value) in entries {
            self.set(key, value)?;
        }
        Ok(self.root)
    }

    /// Create a snapshot of the current state.
    pub fn to_snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            entries: self.export_state(),
            root_hash: self.root_hash(),
            version: self.version(),
        }
    }

    /// Restore state from a snapshot.
    pub fn from_snapshot(&mut self, snapshot: &StateSnapshot) -> Result<()> {
        let root = self.import_state(&snapshot.entries)?;
        if root != snapshot.root_hash {
            return Err(StoreError::InvalidData("snapshot root hash mismatch".into()));
        }
        self.version = snapshot.version;
        Ok(())
    }
}

impl KVStore for MerkleStore {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let trie = TrieDBBuilder::<GridwayTrieLayout>::new(&self.db, &self.root).build();
        trie.get(key)
            .map_err(|e| StoreError::ReadFailed(format!("trie get: {e}")))
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut trie =
            TrieDBMutBuilder::<GridwayTrieLayout>::from_existing(&mut self.db, &mut self.root)
                .build();
        trie.insert(key, value)
            .map_err(|e| StoreError::WriteFailed(format!("trie insert: {e}")))?;
        // Drop commits the trie and updates self.root.
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        let mut trie =
            TrieDBMutBuilder::<GridwayTrieLayout>::from_existing(&mut self.db, &mut self.root)
                .build();
        trie.remove(key)
            .map_err(|e| StoreError::WriteFailed(format!("trie remove: {e}")))?;
        Ok(())
    }

    fn has(&self, key: &[u8]) -> Result<bool> {
        let trie = TrieDBBuilder::<GridwayTrieLayout>::new(&self.db, &self.root).build();
        trie.contains(key)
            .map_err(|e| StoreError::ReadFailed(format!("trie contains: {e}")))
    }

    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        // Build the trie — we need to collect results because the iterator
        // borrows the trie which we create inline. Collecting avoids lifetime issues.
        let trie = TrieDBBuilder::<GridwayTrieLayout>::new(&self.db, &self.root).build();

        let items: Vec<(Vec<u8>, Vec<u8>)> = match trie.iter() {
            Ok(mut iter) => {
                // Seek to the first key >= prefix
                let _ = iter.seek(prefix);
                iter.filter_map(|r| r.ok())
                    .take_while(|(k, _)| k.starts_with(prefix))
                    .collect()
            }
            Err(_) => Vec::new(),
        };

        Box::new(items.into_iter())
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

    // ── New trie-specific tests ──────────────────────────────────────────

    #[test]
    fn test_merkle_root_changes_on_mutation() {
        let mut store = MerkleStore::new("test".to_string());
        let empty_root = store.root_hash();

        store.set(b"key1", b"value1").unwrap();
        let root_after_insert = store.root_hash();
        assert_ne!(empty_root, root_after_insert, "root must change after insert");

        store.delete(b"key1").unwrap();
        let root_after_delete = store.root_hash();
        assert_eq!(
            empty_root, root_after_delete,
            "root must return to empty after deleting only key"
        );
    }

    #[test]
    fn test_prefix_iteration_balance_keys() {
        let mut store = MerkleStore::new("test".to_string());
        store.set(b"balance_alice_ugridway", b"1000").unwrap();
        store.set(b"balance_bob_ugridway", b"2000").unwrap();
        store.set(b"balance_carol_ugridway", b"3000").unwrap();
        store.set(b"supply_ugridway", b"6000").unwrap();

        let balance_items: Vec<_> = store.prefix_iterator(b"balance_").collect();
        assert_eq!(balance_items.len(), 3);
        for (k, _) in &balance_items {
            assert!(k.starts_with(b"balance_"), "key {:?} should start with balance_", k);
        }

        // supply_ prefix should yield exactly one entry
        let supply_items: Vec<_> = store.prefix_iterator(b"supply_").collect();
        assert_eq!(supply_items.len(), 1);
    }

    #[test]
    fn test_has_key() {
        let mut store = MerkleStore::new("test".to_string());
        assert!(!store.has(b"key1").unwrap());
        store.set(b"key1", b"value1").unwrap();
        assert!(store.has(b"key1").unwrap());
        store.delete(b"key1").unwrap();
        assert!(!store.has(b"key1").unwrap());
    }

    #[test]
    fn test_overwrite_value() {
        let mut store = MerkleStore::new("test".to_string());
        store.set(b"key1", b"v1").unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"v1".to_vec()));
        store.set(b"key1", b"v2").unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn test_export_state() {
        let mut store = MerkleStore::new("test".to_string());
        store.set(b"key1", b"value1").unwrap();
        store.set(b"key2", b"value2").unwrap();
        store.commit().unwrap();

        let entries = store.export_state();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, b"key1".to_vec());
        assert_eq!(entries[0].1, b"value1".to_vec());
        assert_eq!(entries[1].0, b"key2".to_vec());
        assert_eq!(entries[1].1, b"value2".to_vec());
    }

    #[test]
    fn test_import_state() {
        let mut store1 = MerkleStore::new("test1".to_string());
        store1.set(b"alice", b"1000").unwrap();
        store1.set(b"bob", b"2000").unwrap();
        store1.commit().unwrap();

        let entries = store1.export_state();
        let root1 = store1.root_hash();

        let mut store2 = MerkleStore::new("test2".to_string());
        let root2 = store2.import_state(&entries).unwrap();
        assert_eq!(root1, root2);
    }

    #[test]
    fn test_snapshot_roundtrip() {
        let mut store = MerkleStore::new("test".to_string());
        store.set(b"bank:balance_alice", b"1000").unwrap();
        store.set(b"bank:balance_bob", b"2000").unwrap();
        store.set(b"auth:account_alice", b"{}").unwrap();
        store.commit().unwrap();

        let snapshot = store.to_snapshot();
        assert_eq!(snapshot.entries.len(), 3);
        assert_eq!(snapshot.version, 1);

        let mut store2 = MerkleStore::new("test2".to_string());
        store2.from_snapshot(&snapshot).unwrap();
        assert_eq!(store2.root_hash(), store.root_hash());
        assert_eq!(store2.version(), store.version());

        // Verify individual keys
        assert_eq!(store2.get(b"bank:balance_alice").unwrap(), Some(b"1000".to_vec()));
        assert_eq!(store2.get(b"bank:balance_bob").unwrap(), Some(b"2000".to_vec()));
    }

    #[test]
    fn test_snapshot_root_hash_mismatch() {
        let mut store = MerkleStore::new("test".to_string());
        store.set(b"key1", b"value1").unwrap();
        store.commit().unwrap();

        let mut snapshot = store.to_snapshot();
        snapshot.root_hash = [0xff; 32]; // corrupt the hash

        let mut store2 = MerkleStore::new("test2".to_string());
        assert!(store2.from_snapshot(&snapshot).is_err());
    }

    #[test]
    fn test_many_keys() {
        let mut store = MerkleStore::new("test".to_string());
        for i in 0..100u32 {
            store.set(format!("key_{i:04}").as_bytes(), format!("val_{i}").as_bytes()).unwrap();
        }
        let hash = store.commit().unwrap();
        assert_ne!(hash, [0u8; 32]);

        // Spot-check a few entries
        assert_eq!(store.get(b"key_0042").unwrap(), Some(b"val_42".to_vec()));
        assert_eq!(store.get(b"key_0099").unwrap(), Some(b"val_99".to_vec()));
        assert!(store.get(b"key_0100").unwrap().is_none());
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_persistent_store_basic() {
        let tmp = TempDir::new().unwrap();
        let mut store = MerkleStore::with_persistence("test".to_string(), tmp.path()).unwrap();
        assert!(store.has_persistence());

        store.set(b"key1", b"value1").unwrap();
        let hash = store.commit().unwrap();
        assert_ne!(hash, [0u8; 32]);
        assert_eq!(store.version(), 1);
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
    }

    #[test]
    fn test_persistent_store_survives_reopen() {
        let tmp = TempDir::new().unwrap();
        let root_after_commit;

        // Write data and commit
        {
            let mut store = MerkleStore::with_persistence("test".to_string(), tmp.path()).unwrap();
            store.set(b"alice", b"1000").unwrap();
            store.set(b"bob", b"2000").unwrap();
            root_after_commit = store.commit().unwrap();
            assert_eq!(store.version(), 1);
        }

        // Reopen and verify data survives
        {
            let store = MerkleStore::with_persistence("test".to_string(), tmp.path()).unwrap();
            assert_eq!(store.version(), 1);
            assert_eq!(store.root_hash(), root_after_commit);
            assert_eq!(store.get(b"alice").unwrap(), Some(b"1000".to_vec()));
            assert_eq!(store.get(b"bob").unwrap(), Some(b"2000".to_vec()));
        }
    }

    #[test]
    fn test_persistent_store_multiple_commits() {
        let tmp = TempDir::new().unwrap();
        let root_v2;

        {
            let mut store = MerkleStore::with_persistence("test".to_string(), tmp.path()).unwrap();
            store.set(b"key1", b"v1").unwrap();
            store.commit().unwrap(); // version 1

            store.set(b"key2", b"v2").unwrap();
            root_v2 = store.commit().unwrap(); // version 2
            assert_eq!(store.version(), 2);
        }

        // Reopen — should have the latest state
        {
            let store = MerkleStore::with_persistence("test".to_string(), tmp.path()).unwrap();
            assert_eq!(store.version(), 2);
            assert_eq!(store.root_hash(), root_v2);
            assert_eq!(store.get(b"key1").unwrap(), Some(b"v1".to_vec()));
            assert_eq!(store.get(b"key2").unwrap(), Some(b"v2".to_vec()));
        }
    }

    #[test]
    fn test_persistent_store_delete_survives_reopen() {
        let tmp = TempDir::new().unwrap();

        {
            let mut store = MerkleStore::with_persistence("test".to_string(), tmp.path()).unwrap();
            store.set(b"key1", b"value1").unwrap();
            store.set(b"key2", b"value2").unwrap();
            store.commit().unwrap();

            store.delete(b"key1").unwrap();
            store.commit().unwrap();
        }

        {
            let store = MerkleStore::with_persistence("test".to_string(), tmp.path()).unwrap();
            assert!(store.get(b"key1").unwrap().is_none());
            assert_eq!(store.get(b"key2").unwrap(), Some(b"value2".to_vec()));
        }
    }

    #[test]
    fn test_persistent_store_snapshot_compatibility() {
        let tmp = TempDir::new().unwrap();

        // Create persistent store with data
        let mut store = MerkleStore::with_persistence("test".to_string(), tmp.path()).unwrap();
        store.set(b"bank:alice", b"1000").unwrap();
        store.set(b"bank:bob", b"2000").unwrap();
        store.commit().unwrap();

        // Export snapshot should still work
        let snapshot = store.to_snapshot();
        assert_eq!(snapshot.entries.len(), 2);
        assert_eq!(snapshot.version, 1);

        // Import snapshot into a non-persistent store should also work
        let mut mem_store = MerkleStore::new("mem".to_string());
        mem_store.from_snapshot(&snapshot).unwrap();
        assert_eq!(mem_store.root_hash(), store.root_hash());
    }

    #[test]
    fn test_persistent_store_many_keys() {
        let tmp = TempDir::new().unwrap();
        let expected_root;

        {
            let mut store = MerkleStore::with_persistence("test".to_string(), tmp.path()).unwrap();
            for i in 0..100u32 {
                store.set(
                    format!("key_{i:04}").as_bytes(),
                    format!("val_{i}").as_bytes(),
                ).unwrap();
            }
            expected_root = store.commit().unwrap();
        }

        {
            let store = MerkleStore::with_persistence("test".to_string(), tmp.path()).unwrap();
            assert_eq!(store.root_hash(), expected_root);
            assert_eq!(store.get(b"key_0042").unwrap(), Some(b"val_42".to_vec()));
            assert_eq!(store.get(b"key_0099").unwrap(), Some(b"val_99".to_vec()));
            assert!(store.get(b"key_0100").unwrap().is_none());
        }
    }

    #[test]
    fn test_in_memory_mode_still_works() {
        // Ensure in-memory mode (no persistence) is not broken
        let mut store = MerkleStore::new("test".to_string());
        assert!(!store.has_persistence());

        store.set(b"key1", b"value1").unwrap();
        let hash = store.commit().unwrap();
        assert_ne!(hash, [0u8; 32]);
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
    }

    #[test]
    fn test_explicit_flush_and_load() {
        let tmp = TempDir::new().unwrap();

        {
            let mut store = MerkleStore::with_persistence("test".to_string(), tmp.path()).unwrap();
            store.set(b"x", b"1").unwrap();

            // Explicit flush (without commit — version stays 0 but data is saved)
            store.flush_to_disk().unwrap();
        }
        // Drop the first store so sled releases the lock

        // Reopen and verify data is present
        {
            let store2 = MerkleStore::with_persistence("test2".to_string(), tmp.path()).unwrap();
            assert_eq!(store2.get(b"x").unwrap(), Some(b"1".to_vec()));
        }
    }
}
