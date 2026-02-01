//! Patricia Merkle Trie state store backed by Parity's trie-db.
//!
//! Uses SHA-256 as the hash function and a no-extension trie layout.
//! The underlying storage is `MemoryDB` from the `memory-db` crate —
//! suitable for experiments. For production, swap in a persistent
//! `HashDB` implementation over RocksDB / sled / etc.

use crate::{Hash, KVStore, Result, StoreError};
use hash_db::Hasher;
use memory_db::{HashKey, MemoryDB};
use reference_trie::GenericNoExtensionLayout;
use sha2::{Digest, Sha256};
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
        }
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
        Ok(self.root)
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
