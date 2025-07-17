//! Global application store with namespace-based key prefixing
//!
//! This module implements a single global store that replaces the MultiStore pattern.
//! It provides namespace isolation through key prefixing, allowing different modules
//! to have isolated storage spaces while using a single underlying JMT store.

use crate::{CommittableStore, Hash, KVStore, Result, StoreError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Global application store that provides namespace isolation
pub struct GlobalAppStore {
    /// The underlying CommittableStore (JMTStore, RealJMTStore, etc.)
    store: Arc<Mutex<Box<dyn CommittableStore + Send + Sync>>>,
    /// Registered namespaces
    namespaces: Arc<Mutex<HashMap<String, StoreConfig>>>,
}

/// Configuration for a namespace
#[derive(Clone, Debug)]
pub struct StoreConfig {
    /// The namespace name (e.g., "auth", "bank")
    pub namespace: String,
    /// Whether this namespace is read-only
    pub read_only: bool,
}

impl GlobalAppStore {
    /// Create a new global app store with a CommittableStore implementation
    pub fn new<S: CommittableStore + Send + Sync + 'static>(store: S) -> Self {
        Self {
            store: Arc::new(Mutex::new(Box::new(store))),
            namespaces: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a namespace
    pub fn register_namespace(&self, namespace: &str, read_only: bool) -> Result<()> {
        let mut namespaces = self
            .namespaces
            .lock()
            .map_err(|e| StoreError::BackendError(format!("Failed to lock namespaces: {e}")))?;

        if namespaces.contains_key(namespace) {
            return Err(StoreError::BackendError(format!(
                "Namespace {namespace} already registered"
            )));
        }

        namespaces.insert(
            namespace.to_string(),
            StoreConfig {
                namespace: namespace.to_string(),
                read_only,
            },
        );

        Ok(())
    }

    /// Get a namespaced view of the store
    pub fn get_namespace(&self, namespace: &str) -> Result<NamespacedStore> {
        let namespaces = self
            .namespaces
            .lock()
            .map_err(|e| StoreError::BackendError(format!("Failed to lock namespaces: {e}")))?;

        let config = namespaces
            .get(namespace)
            .ok_or_else(|| StoreError::StoreNotFound(namespace.to_string()))?
            .clone();

        Ok(NamespacedStore {
            store: self.store.clone(),
            config,
        })
    }

    /// Get a value from a specific namespace
    pub fn get_namespaced(&self, namespace: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let ns_store = self.get_namespace(namespace)?;
        ns_store.get(key)
    }

    /// Set a value in a specific namespace
    pub fn set_namespaced(&self, namespace: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let mut ns_store = self.get_namespace(namespace)?;
        ns_store.set(key, value)
    }

    /// Delete a value from a specific namespace
    pub fn delete_namespaced(&self, namespace: &str, key: &[u8]) -> Result<()> {
        let mut ns_store = self.get_namespace(namespace)?;
        ns_store.delete(key)
    }

    /// Check if a key exists in a specific namespace
    pub fn has_namespaced(&self, namespace: &str, key: &[u8]) -> Result<bool> {
        let ns_store = self.get_namespace(namespace)?;
        ns_store.has(key)
    }

    /// List all registered namespaces
    pub fn list_namespaces(&self) -> Result<Vec<String>> {
        let namespaces = self
            .namespaces
            .lock()
            .map_err(|e| StoreError::BackendError(format!("Failed to lock namespaces: {e}")))?;
        Ok(namespaces.keys().cloned().collect())
    }

    /// Get the underlying store for direct access (use with caution)
    pub fn get_store(&self) -> Arc<Mutex<Box<dyn CommittableStore + Send + Sync>>> {
        self.store.clone()
    }

    /// Commit pending changes and return the root hash
    pub fn commit(&self) -> Result<Hash> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| StoreError::BackendError(format!("Failed to lock store: {e}")))?;
        store.commit()
    }

    /// Get the current root hash without committing
    pub fn root_hash(&self) -> Result<Hash> {
        let store = self
            .store
            .lock()
            .map_err(|e| StoreError::BackendError(format!("Failed to lock store: {e}")))?;
        Ok(store.root_hash())
    }
}

/// A namespaced view of the global store
pub struct NamespacedStore {
    store: Arc<Mutex<Box<dyn CommittableStore + Send + Sync>>>,
    config: StoreConfig,
}

impl NamespacedStore {
    /// Create a prefixed key for this namespace
    fn prefix_key(&self, key: &[u8]) -> Vec<u8> {
        let mut prefixed = Vec::with_capacity(self.config.namespace.len() + 1 + key.len());
        prefixed.extend_from_slice(self.config.namespace.as_bytes());
        prefixed.push(b'/');
        prefixed.extend_from_slice(key);
        prefixed
    }

    /// Remove the namespace prefix from a key
    #[allow(dead_code)]
    fn strip_prefix(&self, key: &[u8]) -> Option<Vec<u8>> {
        let prefix_len = self.config.namespace.len() + 1;
        if key.len() > prefix_len
            && key.starts_with(self.config.namespace.as_bytes())
            && key[self.config.namespace.len()] == b'/'
        {
            Some(key[prefix_len..].to_vec())
        } else {
            None
        }
    }
}

impl KVStore for NamespacedStore {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let prefixed_key = self.prefix_key(key);
        let store = self
            .store
            .lock()
            .map_err(|e| StoreError::BackendError(format!("Failed to lock store: {e}")))?;
        store.get(&prefixed_key)
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        if self.config.read_only {
            return Err(StoreError::WriteFailed(
                "Namespace is read-only".to_string(),
            ));
        }

        let prefixed_key = self.prefix_key(key);
        let mut store = self
            .store
            .lock()
            .map_err(|e| StoreError::BackendError(format!("Failed to lock store: {e}")))?;
        store.set(&prefixed_key, value)
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        if self.config.read_only {
            return Err(StoreError::WriteFailed(
                "Namespace is read-only".to_string(),
            ));
        }

        let prefixed_key = self.prefix_key(key);
        let mut store = self
            .store
            .lock()
            .map_err(|e| StoreError::BackendError(format!("Failed to lock store: {e}")))?;
        store.delete(&prefixed_key)
    }

    fn has(&self, key: &[u8]) -> Result<bool> {
        let prefixed_key = self.prefix_key(key);
        let store = self
            .store
            .lock()
            .map_err(|e| StoreError::BackendError(format!("Failed to lock store: {e}")))?;
        store.has(&prefixed_key)
    }

    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        let full_prefix = self.prefix_key(prefix);
        let store = match self.store.lock() {
            Ok(s) => s,
            Err(_) => return Box::new(std::iter::empty()),
        };

        // Get iterator from underlying store and strip namespace prefix from keys
        let namespace = self.config.namespace.clone();
        let items: Vec<_> = store
            .prefix_iterator(&full_prefix)
            .filter_map(move |(k, v)| {
                let prefix_len = namespace.len() + 1;
                if k.len() > prefix_len {
                    Some((k[prefix_len..].to_vec(), v))
                } else {
                    None
                }
            })
            .collect();

        Box::new(items.into_iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_store() -> (GlobalAppStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let jmt_store = crate::jmt::JMTStore::new("test".to_string(), temp_dir.path()).unwrap();
        let global_store = GlobalAppStore::new(jmt_store);
        (global_store, temp_dir)
    }

    #[test]
    fn test_namespace_registration() {
        let (store, _temp) = create_test_store();

        // Register namespaces
        assert!(store.register_namespace("auth", false).is_ok());
        assert!(store.register_namespace("bank", false).is_ok());
        assert!(store.register_namespace("readonly", true).is_ok());

        // Duplicate registration should fail
        assert!(store.register_namespace("auth", false).is_err());

        // List namespaces
        let namespaces = store.list_namespaces().unwrap();
        assert_eq!(namespaces.len(), 3);
        assert!(namespaces.contains(&"auth".to_string()));
        assert!(namespaces.contains(&"bank".to_string()));
        assert!(namespaces.contains(&"readonly".to_string()));
    }

    #[test]
    fn test_namespace_isolation() {
        let (store, _temp) = create_test_store();

        store.register_namespace("auth", false).unwrap();
        store.register_namespace("bank", false).unwrap();

        // Set values in different namespaces
        store
            .set_namespaced("auth", b"key1", b"auth_value")
            .unwrap();
        store
            .set_namespaced("bank", b"key1", b"bank_value")
            .unwrap();

        // Get values - should be isolated
        let auth_value = store.get_namespaced("auth", b"key1").unwrap().unwrap();
        let bank_value = store.get_namespaced("bank", b"key1").unwrap().unwrap();

        assert_eq!(auth_value, b"auth_value");
        assert_eq!(bank_value, b"bank_value");
    }

    #[test]
    fn test_namespaced_store_operations() {
        let (store, _temp) = create_test_store();

        store.register_namespace("test", false).unwrap();
        let mut ns_store = store.get_namespace("test").unwrap();

        // Test KVStore operations
        assert!(ns_store.get(b"nonexistent").unwrap().is_none());
        assert!(!ns_store.has(b"nonexistent").unwrap());

        ns_store.set(b"key", b"value").unwrap();
        assert!(ns_store.has(b"key").unwrap());
        assert_eq!(ns_store.get(b"key").unwrap().unwrap(), b"value");

        ns_store.delete(b"key").unwrap();
        assert!(!ns_store.has(b"key").unwrap());
        assert!(ns_store.get(b"key").unwrap().is_none());
    }

    #[test]
    fn test_read_only_namespace() {
        let (store, _temp) = create_test_store();

        store.register_namespace("readonly", true).unwrap();
        let mut ns_store = store.get_namespace("readonly").unwrap();

        // Read operations should work
        assert!(ns_store.get(b"key").unwrap().is_none());
        assert!(!ns_store.has(b"key").unwrap());

        // Write operations should fail
        assert!(ns_store.set(b"key", b"value").is_err());
        assert!(ns_store.delete(b"key").is_err());
    }

    #[test]
    fn test_unregistered_namespace() {
        let (store, _temp) = create_test_store();

        // Operations on unregistered namespace should fail
        assert!(store.get_namespace("unregistered").is_err());
        assert!(store.get_namespaced("unregistered", b"key").is_err());
        assert!(store
            .set_namespaced("unregistered", b"key", b"value")
            .is_err());
    }
}
