//! Global application store with namespace isolation.
//!
//! Provides a multi-namespace store backed by a single MerkleStore.
//! Each module (bank, auth, staking, etc.) gets its own isolated namespace.

use crate::{KVStore, MerkleStore, Result, StoreError};
use std::path::Path;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Global application store that manages namespaced sub-stores.
/// All namespaces share a single underlying MerkleStore for
/// unified state root computation.
pub struct GlobalAppStore {
    /// The underlying MerkleStore that computes unified state roots
    store: Arc<Mutex<MerkleStore>>,
    /// Registered namespaces
    namespaces: Mutex<HashMap<String, bool>>,
}

impl GlobalAppStore {
    /// Create a new GlobalAppStore with a MerkleStore backend
    pub fn new(store: MerkleStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            namespaces: Mutex::new(HashMap::new()),
        }
    }

    /// Create a new GlobalAppStore with sled persistence at the given path.
    ///
    /// The underlying MerkleStore will automatically flush to sled on commit
    /// and load from sled on startup.
    pub fn with_persistence(path: &Path) -> Result<Self> {
        let store = MerkleStore::with_persistence("state".to_string(), path)?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            namespaces: Mutex::new(HashMap::new()),
        })
    }

    /// Register a namespace
    pub fn register_namespace(&self, name: &str, _read_only: bool) -> Result<()> {
        let mut ns = self.namespaces.lock()
            .map_err(|e| StoreError::BackendError(format!("lock failed: {e}")))?;
        ns.insert(name.to_string(), _read_only);
        Ok(())
    }

    /// Get a namespaced store view
    pub fn get_namespace(&self, name: &str) -> Result<NamespacedStore> {
        let ns = self.namespaces.lock()
            .map_err(|e| StoreError::BackendError(format!("lock failed: {e}")))?;
        if !ns.contains_key(name) {
            return Err(StoreError::StoreNotFound(name.to_string()));
        }
        Ok(NamespacedStore {
            store: self.store.clone(),
            prefix: format!("{name}:"),
        })
    }

    /// Get reference to underlying store
    pub fn get_store(&self) -> Arc<Mutex<MerkleStore>> {
        self.store.clone()
    }

    /// Set a value in a namespace (convenience method)
    pub fn set_namespaced(&self, namespace: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let prefixed_key = Self::make_key(namespace, key);
        let mut store = self.store.lock()
            .map_err(|e| StoreError::BackendError(format!("lock failed: {e}")))?;
        store.set(&prefixed_key, value)
    }

    /// Get a value from a namespace (convenience method)
    pub fn get_namespaced(&self, namespace: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let prefixed_key = Self::make_key(namespace, key);
        let store = self.store.lock()
            .map_err(|e| StoreError::BackendError(format!("lock failed: {e}")))?;
        store.get(&prefixed_key)
    }

    fn make_key(namespace: &str, key: &[u8]) -> Vec<u8> {
        let mut prefixed = Vec::with_capacity(namespace.len() + 1 + key.len());
        prefixed.extend_from_slice(namespace.as_bytes());
        prefixed.push(b':');
        prefixed.extend_from_slice(key);
        prefixed
    }
}

/// A namespaced view into the GlobalAppStore.
/// All keys are automatically prefixed with the namespace.
pub struct NamespacedStore {
    store: Arc<Mutex<MerkleStore>>,
    prefix: String,
}

impl NamespacedStore {
    fn prefixed_key(&self, key: &[u8]) -> Vec<u8> {
        let mut prefixed = Vec::with_capacity(self.prefix.len() + key.len());
        prefixed.extend_from_slice(self.prefix.as_bytes());
        prefixed.extend_from_slice(key);
        prefixed
    }
}

impl KVStore for NamespacedStore {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let prefixed = self.prefixed_key(key);
        let store = self.store.lock()
            .map_err(|e| StoreError::BackendError(format!("lock failed: {e}")))?;
        store.get(&prefixed)
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        let prefixed = self.prefixed_key(key);
        let mut store = self.store.lock()
            .map_err(|e| StoreError::BackendError(format!("lock failed: {e}")))?;
        store.set(&prefixed, value)
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        let prefixed = self.prefixed_key(key);
        let mut store = self.store.lock()
            .map_err(|e| StoreError::BackendError(format!("lock failed: {e}")))?;
        store.delete(&prefixed)
    }

    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        let mut full_prefix = self.prefix.as_bytes().to_vec();
        full_prefix.extend_from_slice(prefix);
        let store = self.store.lock().unwrap();
        let items: Vec<_> = store
            .prefix_iterator(&full_prefix)
            .map(|(k, v)| {
                // Strip namespace prefix from returned keys
                let stripped = k[self.prefix.len()..].to_vec();
                (stripped, v)
            })
            .collect();
        Box::new(items.into_iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_store_namespaces() {
        let store = MerkleStore::new("state".to_string());
        let global = GlobalAppStore::new(store);

        global.register_namespace("bank", false).unwrap();
        global.register_namespace("auth", false).unwrap();

        global.set_namespaced("bank", b"alice", b"1000").unwrap();
        global.set_namespaced("auth", b"alice", b"nonce:1").unwrap();

        assert_eq!(
            global.get_namespaced("bank", b"alice").unwrap(),
            Some(b"1000".to_vec())
        );
        assert_eq!(
            global.get_namespaced("auth", b"alice").unwrap(),
            Some(b"nonce:1".to_vec())
        );
    }

    #[test]
    fn test_namespaced_store() {
        let store = MerkleStore::new("state".to_string());
        let global = GlobalAppStore::new(store);
        global.register_namespace("bank", false).unwrap();

        let mut ns = global.get_namespace("bank").unwrap();
        ns.set(b"bob", b"500").unwrap();

        assert_eq!(ns.get(b"bob").unwrap(), Some(b"500".to_vec()));

        // Also accessible through global
        assert_eq!(
            global.get_namespaced("bank", b"bob").unwrap(),
            Some(b"500".to_vec())
        );
    }

    #[test]
    fn test_namespace_isolation() {
        let store = MerkleStore::new("state".to_string());
        let global = GlobalAppStore::new(store);
        global.register_namespace("bank", false).unwrap();
        global.register_namespace("auth", false).unwrap();

        global.set_namespaced("bank", b"key1", b"bank_val").unwrap();
        global.set_namespaced("auth", b"key1", b"auth_val").unwrap();

        assert_eq!(
            global.get_namespaced("bank", b"key1").unwrap(),
            Some(b"bank_val".to_vec())
        );
        assert_eq!(
            global.get_namespaced("auth", b"key1").unwrap(),
            Some(b"auth_val".to_vec())
        );
    }
}
