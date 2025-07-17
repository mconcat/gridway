//! Real JMT (Jellyfish Merkle Tree) implementation using the jmt crate.
//!
//! This module provides a proper authenticated data structure implementation
//! with cryptographic proofs and efficient versioned storage.

use crate::{KVStore, Result, StoreError};
use jmt::{
    storage::{LeafNode, Node, NodeBatch, NodeKey, TreeReader, TreeWriter},
    JellyfishMerkleTree, KeyHash, OwnedValue, Version as JmtVersion,
};
use rocksdb::{Options, WriteBatch, DB};
use sha2::Sha256;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Empty tree root hash - this is the hash of an empty sparse merkle tree
const SPARSE_MERKLE_PLACEHOLDER_HASH: [u8; 32] = [0u8; 32];

/// Type alias for JMT version
pub type Version = u64;

/// Type alias for hash values
pub type Hash = [u8; 32];

/// RocksDB-backed tree reader for JMT
pub struct RocksDBTreeReader {
    db: Arc<DB>,
}

impl TreeReader for RocksDBTreeReader {
    fn get_node_option(&self, node_key: &NodeKey) -> anyhow::Result<Option<Node>> {
        let key = encode_node_key(node_key);
        match self.db.get(&key) {
            Ok(Some(value)) => {
                let node = bincode::deserialize(&value)
                    .map_err(|e| anyhow::anyhow!("Failed to deserialize node: {}", e))?;
                Ok(Some(node))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("RocksDB error: {}", e)),
        }
    }

    fn get_value_option(
        &self,
        version: JmtVersion,
        key_hash: KeyHash,
    ) -> anyhow::Result<Option<OwnedValue>> {
        let key = encode_value_key(version, &key_hash);
        match self.db.get(&key) {
            Ok(Some(value)) => Ok(Some(value)),
            Ok(None) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("RocksDB error: {}", e)),
        }
    }

    fn get_rightmost_leaf(&self) -> anyhow::Result<Option<(NodeKey, LeafNode)>> {
        // This is a simplified implementation
        // In production, we'd maintain an index for efficient rightmost leaf lookup
        Ok(None)
    }
}

/// RocksDB-backed tree writer for JMT
pub struct RocksDBTreeWriter<'a> {
    batch: RefCell<&'a mut WriteBatch>,
}

impl<'a> TreeWriter for RocksDBTreeWriter<'a> {
    fn write_node_batch(&self, node_batch: &NodeBatch) -> anyhow::Result<()> {
        let mut batch = self.batch.borrow_mut();

        // Write nodes
        for (node_key, node) in node_batch.nodes() {
            let key = encode_node_key(node_key);
            let value = bincode::serialize(node)
                .map_err(|e| anyhow::anyhow!("Failed to serialize node: {}", e))?;
            batch.put(&key, &value);
        }

        // Write values
        for ((version, key_hash), value_opt) in node_batch.values() {
            let key = encode_value_key(*version, key_hash);
            match value_opt {
                Some(value) => batch.put(&key, value),
                None => batch.delete(&key),
            }
        }

        Ok(())
    }
}

/// Real JMT store implementation
pub struct RealJMTStore {
    /// RocksDB instance
    db: Arc<DB>,
    /// Current version
    version: Arc<Mutex<Version>>,
    /// Pending changes to be committed
    pending: Arc<Mutex<HashMap<Vec<u8>, Option<Vec<u8>>>>>,
    /// Store name
    #[allow(dead_code)]
    name: String,
}

impl RealJMTStore {
    /// Create a new real JMT store
    pub fn new<P: AsRef<Path>>(name: String, db_path: P) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);

        let db = Arc::new(
            DB::open(&opts, db_path)
                .map_err(|e| StoreError::BackendError(format!("RocksDB error: {e}")))?,
        );

        // Load the current version from storage
        let version = match db.get(b"__current_version") {
            Ok(Some(v)) => {
                let bytes: [u8; 8] = v
                    .try_into()
                    .map_err(|_| StoreError::BackendError("Invalid version bytes".to_string()))?;
                u64::from_be_bytes(bytes)
            }
            Ok(None) => 0,
            Err(e) => {
                return Err(StoreError::BackendError(format!(
                    "Failed to read version: {e}"
                )))
            }
        };

        Ok(Self {
            db,
            version: Arc::new(Mutex::new(version)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            name,
        })
    }

    /// Get the current version
    pub fn version(&self) -> Version {
        *self.version.lock().unwrap()
    }

    /// Set the current version
    pub fn set_version(&mut self, version: Version) {
        *self.version.lock().unwrap() = version;
    }

    /// Get the root hash at current version
    pub fn root_hash(&self) -> Hash {
        self.get_root_hash(self.version()).unwrap_or([0u8; 32])
    }

    /// Get the root hash at a specific version
    pub fn get_root_hash(&self, version: Version) -> Result<Hash> {
        // Get the root hash from storage
        let key = format!("__root_hash_{version}");
        match self.db.get(key.as_bytes()) {
            Ok(Some(hash_bytes)) => {
                if hash_bytes.len() == 32 {
                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(&hash_bytes);
                    Ok(hash)
                } else {
                    Ok(SPARSE_MERKLE_PLACEHOLDER_HASH)
                }
            }
            Ok(None) => Ok(SPARSE_MERKLE_PLACEHOLDER_HASH),
            Err(e) => Err(StoreError::BackendError(format!("RocksDB error: {e}"))),
        }
    }

    /// Commit pending changes
    pub fn commit(&mut self) -> Result<Hash> {
        let pending = {
            let mut pending_guard = self.pending.lock().unwrap();
            std::mem::take(&mut *pending_guard)
        };

        if pending.is_empty() {
            return Ok(self.root_hash());
        }

        let current_version = self.version();
        let new_version = current_version + 1;

        // Convert pending changes to JMT format
        let value_set: Vec<(KeyHash, Option<OwnedValue>)> = pending
            .into_iter()
            .map(|(key, value_opt)| {
                let key_hash = hash_key(&key);
                (key_hash, value_opt)
            })
            .collect();

        // Create JMT instance for this update
        let reader = RocksDBTreeReader {
            db: self.db.clone(),
        };
        let tree = JellyfishMerkleTree::<_, Sha256>::new(&reader);

        // Apply updates to the tree
        let (new_root_hash, tree_update_batch) = tree
            .put_value_set(value_set, new_version)
            .map_err(|e| StoreError::BackendError(format!("JMT update failed: {e}")))?;

        // Write to RocksDB
        let mut batch = WriteBatch::default();

        // Write tree updates via TreeWriter
        let writer = RocksDBTreeWriter {
            batch: RefCell::new(&mut batch),
        };
        writer
            .write_node_batch(&tree_update_batch.node_batch)
            .map_err(|e| StoreError::BackendError(format!("Failed to write nodes: {e}")))?;

        // Handle stale nodes (for pruning)
        for stale_node in &tree_update_batch.stale_node_index_batch {
            // Store stale node information for potential pruning
            let stale_key = format!(
                "__stale_{}_{:?}",
                stale_node.stale_since_version, stale_node.node_key
            );
            batch.put(stale_key.as_bytes(), b"");
        }

        // Store the new root hash
        let root_key = format!("__root_hash_{new_version}");
        batch.put(root_key.as_bytes(), new_root_hash.0.as_slice());

        // Update version
        batch.put(b"__current_version", new_version.to_be_bytes());

        // Apply batch
        self.db
            .write(batch)
            .map_err(|e| StoreError::BackendError(format!("RocksDB write failed: {e}")))?;

        // Update in-memory version
        *self.version.lock().unwrap() = new_version;

        Ok(new_root_hash.0)
    }

    /// Get a value with proof
    pub fn get_with_proof(&self, key: &[u8]) -> Result<(Option<Vec<u8>>, Vec<u8>)> {
        let key_hash = hash_key(key);
        let version = self.version();

        // Create JMT instance for reading
        let reader = RocksDBTreeReader {
            db: self.db.clone(),
        };
        let tree = JellyfishMerkleTree::<_, Sha256>::new(&reader);

        let (value, proof) = tree
            .get_with_proof(key_hash, version)
            .map_err(|e| StoreError::BackendError(format!("Failed to get with proof: {e}")))?;

        // Serialize proof
        let proof_bytes = bincode::serialize(&proof)
            .map_err(|e| StoreError::BackendError(format!("Failed to serialize proof: {e}")))?;

        Ok((value, proof_bytes))
    }

    /// Load committed data from storage
    pub fn load_committed_data(&mut self) -> Result<()> {
        // This is called during initialization to populate any caches if needed
        Ok(())
    }
}

impl KVStore for RealJMTStore {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // Check pending changes first
        let pending = self.pending.lock().unwrap();
        if let Some(value_opt) = pending.get(key) {
            return Ok(value_opt.clone());
        }
        drop(pending);

        // Then check the tree
        let key_hash = hash_key(key);
        let version = self.version();

        // Create JMT instance for reading
        let reader = RocksDBTreeReader {
            db: self.db.clone(),
        };

        // Use TreeReader directly to get the value
        match reader.get_value_option(version, key_hash) {
            Ok(value_opt) => Ok(value_opt),
            Err(e) => Err(StoreError::BackendError(format!("JMT get failed: {e}"))),
        }
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut pending = self.pending.lock().unwrap();
        pending.insert(key.to_vec(), Some(value.to_vec()));
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        let mut pending = self.pending.lock().unwrap();
        pending.insert(key.to_vec(), None);
        Ok(())
    }

    fn prefix_iterator(&self, _prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        // This is a simplified implementation
        // A proper implementation would traverse the tree efficiently
        Box::new(std::iter::empty())
    }
}

impl crate::CommittableStore for RealJMTStore {
    fn commit(&mut self) -> crate::Result<crate::Hash> {
        self.commit()
    }

    fn root_hash(&self) -> crate::Hash {
        self.root_hash()
    }
}

/// Hash a key for use in JMT
fn hash_key(key: &[u8]) -> KeyHash {
    use sha2::Digest;
    let mut hasher = <Sha256 as Digest>::new();
    Digest::update(&mut hasher, key);
    let hash: [u8; 32] = Digest::finalize(hasher).into();
    KeyHash(hash)
}

/// Encode a node key for storage
fn encode_node_key(node_key: &NodeKey) -> Vec<u8> {
    let mut key = Vec::with_capacity(33);
    key.push(b'n'); // prefix for nodes
    key.extend_from_slice(&bincode::serialize(node_key).unwrap());
    key
}

/// Encode a value key for storage
fn encode_value_key(version: JmtVersion, key_hash: &KeyHash) -> Vec<u8> {
    let mut key = Vec::with_capacity(41);
    key.push(b'v'); // prefix for values
    key.extend_from_slice(&version.to_be_bytes());
    key.extend_from_slice(&key_hash.0);
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KVStore;
    use tempfile::TempDir;

    #[test]
    fn test_real_jmt_basic() {
        let temp_dir = TempDir::new().unwrap();
        let mut store = RealJMTStore::new("test".to_string(), temp_dir.path()).unwrap();

        // Test empty tree
        assert_eq!(store.root_hash(), SPARSE_MERKLE_PLACEHOLDER_HASH);

        // Set a value
        store.set(b"key1", b"value1").unwrap();
        let root1 = store.commit().unwrap();
        assert_ne!(root1, SPARSE_MERKLE_PLACEHOLDER_HASH);

        // Get the value
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        // Update the value
        store.set(b"key1", b"value2").unwrap();
        let root2 = store.commit().unwrap();
        assert_ne!(root2, root1);

        // Delete the value
        store.delete(b"key1").unwrap();
        let root3 = store.commit().unwrap();
        assert_eq!(root3, SPARSE_MERKLE_PLACEHOLDER_HASH);
    }

    #[test]
    fn test_real_jmt_proof() {
        let temp_dir = TempDir::new().unwrap();
        let mut store = RealJMTStore::new("test".to_string(), temp_dir.path()).unwrap();

        // Add some data
        store.set(b"key1", b"value1").unwrap();
        store.set(b"key2", b"value2").unwrap();
        store.commit().unwrap();

        // Get with proof
        let (value, proof) = store.get_with_proof(b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
        assert!(!proof.is_empty());

        // Non-existent key should also have a proof
        let (value, proof) = store.get_with_proof(b"key3").unwrap();
        assert_eq!(value, None);
        assert!(!proof.is_empty());
    }
}
