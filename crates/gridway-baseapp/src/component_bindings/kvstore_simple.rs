//! Simple KVStore Resource Implementation
//!
//! This provides a simplified KVStore resource implementation for WASI components.

use gridway_store::KVStore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Type alias for a shared KVStore instance
type SharedKVStore = Arc<Mutex<dyn KVStore>>;

/// Type alias for the stores map
type StoresMap = HashMap<String, SharedKVStore>;

/// Simple KVStore resource that can be shared with WASI components
pub struct SimpleKVStoreResource {
    /// Name of the store
    pub name: String,
    /// Actual KVStore implementation
    pub store: Arc<Mutex<dyn KVStore>>,
}

impl SimpleKVStoreResource {
    pub fn new(name: String, store: Arc<Mutex<dyn KVStore>>) -> Self {
        Self { name, store }
    }

    /// Get a value by key
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store:: {e}"))?;
        store.get(key).map_err(|e| e.to_string())
    }

    /// Set a key-value pair
    pub fn set(&self, key: &[u8], value: &[u8]) -> Result<(), String> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store:: {e}"))?;
        store.set(key, value).map_err(|e| e.to_string())
    }

    /// Delete a key
    pub fn delete(&self, key: &[u8]) -> Result<(), String> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store:: {e}"))?;
        store.delete(key).map_err(|e| e.to_string())
    }

    /// Check if a key exists
    pub fn has(&self, key: &[u8]) -> Result<bool, String> {
        let store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store:: {e}"))?;
        store.has(key).map_err(|e| e.to_string())
    }
}

/// Simple KVStore resource manager for component hosts
#[derive(Clone)]
pub struct SimpleKVStoreManager {
    /// Available stores by name
    stores: Arc<Mutex<StoresMap>>,
}

impl SimpleKVStoreManager {
    pub fn new() -> Self {
        Self {
            stores: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Mount a KVStore with the given name
    pub fn mount_store(&self, name: String, store: SharedKVStore) -> Result<(), String> {
        let mut stores = self
            .stores
            .lock()
            .map_err(|e| format!("Failed to lock stores:: {e}"))?;
        stores.insert(name, store);
        Ok(())
    }

    /// Get a store by name
    pub fn get_store(&self, name: &str) -> Result<SharedKVStore, String> {
        let stores = self
            .stores
            .lock()
            .map_err(|e| format!("Failed to lock stores:: {e}"))?;

        stores
            .get(name)
            .cloned()
            .ok_or_else(|| format!("Store '{name}' not found"))
    }

    /// List all available store names
    pub fn list_stores(&self) -> Result<Vec<String>, String> {
        let stores = self
            .stores
            .lock()
            .map_err(|e| format!("Failed to lock stores:: {e}"))?;

        Ok(stores.keys().cloned().collect())
    }
}

impl Default for SimpleKVStoreManager {
    fn default() -> Self {
        Self::new()
    }
}
