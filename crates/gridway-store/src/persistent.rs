//! Sled-backed persistence layer for MerkleStore.
//!
//! Strategy: memory-db remains the working trie database for fast
//! trie operations. Sled is used as a checkpoint/durability layer:
//! - On `commit()` (when persistence is enabled): flush all memory-db
//!   entries to sled.
//! - On `load_from_disk()`: populate memory-db from sled entries,
//!   returning the stored root hash.
//!
//! This gives us persistence without changing trie-db integration.

use std::path::Path;

use crate::{Result, StoreError};

/// A sled-backed persistence layer for trie node storage.
///
/// Stores key-value pairs where keys are hash-addressed trie nodes
/// (from memory-db) and values are the node data. Also stores metadata
/// like the current root hash and version.
pub struct SledBackend {
    /// The sled database instance.
    db: sled::Db,
    /// Tree for trie node data (hash → node bytes).
    nodes: sled::Tree,
    /// Tree for metadata (root_hash, version, etc.).
    meta: sled::Tree,
}

const META_ROOT_KEY: &[u8] = b"root_hash";
const META_VERSION_KEY: &[u8] = b"version";

impl SledBackend {
    /// Open or create a sled database at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let db =
            sled::open(path).map_err(|e| StoreError::BackendError(format!("sled open: {e}")))?;

        let nodes = db
            .open_tree("trie_nodes")
            .map_err(|e| StoreError::BackendError(format!("sled open tree 'trie_nodes': {e}")))?;

        let meta = db
            .open_tree("meta")
            .map_err(|e| StoreError::BackendError(format!("sled open tree 'meta': {e}")))?;

        Ok(Self { db, nodes, meta })
    }

    /// Write a batch of trie node entries to sled.
    pub fn write_nodes(&self, entries: &[(&[u8], &[u8])]) -> Result<()> {
        let mut batch = sled::Batch::default();
        for (key, value) in entries {
            batch.insert(*key, *value);
        }
        self.nodes
            .apply_batch(batch)
            .map_err(|e| StoreError::BackendError(format!("sled batch write: {e}")))?;
        Ok(())
    }

    /// Read all trie node entries from sled.
    pub fn read_all_nodes(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut entries = Vec::new();
        for item in self.nodes.iter() {
            let (k, v) = item.map_err(|e| StoreError::BackendError(format!("sled iter: {e}")))?;
            entries.push((k.to_vec(), v.to_vec()));
        }
        Ok(entries)
    }

    /// Store the root hash.
    pub fn set_root(&self, root: &[u8; 32]) -> Result<()> {
        self.meta
            .insert(META_ROOT_KEY, root.as_ref())
            .map_err(|e| StoreError::BackendError(format!("sled set root: {e}")))?;
        Ok(())
    }

    /// Load the stored root hash (if any).
    pub fn get_root(&self) -> Result<Option<[u8; 32]>> {
        match self.meta.get(META_ROOT_KEY) {
            Ok(Some(bytes)) => {
                if bytes.len() != 32 {
                    return Err(StoreError::BackendError(
                        "stored root hash is not 32 bytes".into(),
                    ));
                }
                let mut root = [0u8; 32];
                root.copy_from_slice(&bytes);
                Ok(Some(root))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(StoreError::BackendError(format!("sled get root: {e}"))),
        }
    }

    /// Store the version number.
    pub fn set_version(&self, version: u64) -> Result<()> {
        self.meta
            .insert(META_VERSION_KEY, &version.to_le_bytes())
            .map_err(|e| StoreError::BackendError(format!("sled set version: {e}")))?;
        Ok(())
    }

    /// Load the stored version number (if any).
    pub fn get_version(&self) -> Result<Option<u64>> {
        match self.meta.get(META_VERSION_KEY) {
            Ok(Some(bytes)) => {
                if bytes.len() != 8 {
                    return Err(StoreError::BackendError(
                        "stored version is not 8 bytes".into(),
                    ));
                }
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&bytes);
                Ok(Some(u64::from_le_bytes(buf)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(StoreError::BackendError(format!("sled get version: {e}"))),
        }
    }

    /// Clear all trie nodes (used when rebuilding from snapshot).
    pub fn clear_nodes(&self) -> Result<()> {
        self.nodes
            .clear()
            .map_err(|e| StoreError::BackendError(format!("sled clear nodes: {e}")))?;
        Ok(())
    }

    /// Flush all pending writes to disk.
    pub fn flush(&self) -> Result<()> {
        self.db
            .flush()
            .map_err(|e| StoreError::BackendError(format!("sled flush: {e}")))?;
        Ok(())
    }

    /// Check if the database has any stored state.
    pub fn has_state(&self) -> Result<bool> {
        Ok(self
            .meta
            .get(META_ROOT_KEY)
            .map_err(|e| StoreError::BackendError(format!("sled has_state: {e}")))?
            .is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sled_backend_open_and_close() {
        let tmp = TempDir::new().unwrap();
        let backend = SledBackend::open(tmp.path()).unwrap();
        assert!(!backend.has_state().unwrap());
    }

    #[test]
    fn test_sled_backend_metadata() {
        let tmp = TempDir::new().unwrap();
        let backend = SledBackend::open(tmp.path()).unwrap();

        assert!(backend.get_root().unwrap().is_none());
        assert!(backend.get_version().unwrap().is_none());

        let root = [42u8; 32];
        backend.set_root(&root).unwrap();
        assert_eq!(backend.get_root().unwrap(), Some(root));

        backend.set_version(5).unwrap();
        assert_eq!(backend.get_version().unwrap(), Some(5));
    }

    #[test]
    fn test_sled_backend_nodes_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let backend = SledBackend::open(tmp.path()).unwrap();

        let entries: Vec<(&[u8], &[u8])> = vec![
            (b"key1", b"value1"),
            (b"key2", b"value2"),
            (b"key3", b"value3"),
        ];
        backend.write_nodes(&entries).unwrap();

        let loaded = backend.read_all_nodes().unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0], (b"key1".to_vec(), b"value1".to_vec()));
        assert_eq!(loaded[1], (b"key2".to_vec(), b"value2".to_vec()));
        assert_eq!(loaded[2], (b"key3".to_vec(), b"value3".to_vec()));
    }

    #[test]
    fn test_sled_backend_persistence_across_reopen() {
        let tmp = TempDir::new().unwrap();

        {
            let backend = SledBackend::open(tmp.path()).unwrap();
            backend.write_nodes(&[(b"node1", b"data1")]).unwrap();
            backend.set_root(&[1u8; 32]).unwrap();
            backend.set_version(3).unwrap();
            backend.flush().unwrap();
        }

        {
            let backend = SledBackend::open(tmp.path()).unwrap();
            assert!(backend.has_state().unwrap());
            assert_eq!(backend.get_root().unwrap(), Some([1u8; 32]));
            assert_eq!(backend.get_version().unwrap(), Some(3));
            let nodes = backend.read_all_nodes().unwrap();
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0], (b"node1".to_vec(), b"data1".to_vec()));
        }
    }

    #[test]
    fn test_sled_backend_clear_nodes() {
        let tmp = TempDir::new().unwrap();
        let backend = SledBackend::open(tmp.path()).unwrap();

        backend.write_nodes(&[(b"a", b"1"), (b"b", b"2")]).unwrap();
        assert_eq!(backend.read_all_nodes().unwrap().len(), 2);

        backend.clear_nodes().unwrap();
        assert_eq!(backend.read_all_nodes().unwrap().len(), 0);

        backend.set_root(&[9u8; 32]).unwrap();
        assert_eq!(backend.get_root().unwrap(), Some([9u8; 32]));
    }
}
