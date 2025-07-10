//! KVStore Resource Implementation
//!
//! This module provides the WASI resource implementation for KVStore access,
//! allowing WASI components to interact with blockchain state through resource handles.

use helium_store::KVStore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::component::Resource;
use wasmtime_wasi::ResourceTable;

/// KVStore resource data that will be stored in the ResourceTable
pub struct KVStoreResource {
    /// Name of the store
    pub name: String,
    /// Actual KVStore implementation
    pub store: Arc<Mutex<dyn KVStore>>,
}

impl KVStoreResource {
    pub fn new(name: String, store: Arc<Mutex<dyn KVStore>>) -> Self {
        Self { name, store }
    }

    /// Get a value by key
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store: {e}"))?;
        store.get(key).map_err(|e| e.to_string())
    }

    /// Set a key-value pair
    pub fn set(&self, key: &[u8], value: &[u8]) -> Result<(), String> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store: {e}"))?;
        store.set(key, value).map_err(|e| e.to_string())
    }

    /// Delete a key
    pub fn delete(&self, key: &[u8]) -> Result<(), String> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store: {e}"))?;
        store.delete(key).map_err(|e| e.to_string())
    }

    /// Check if a key exists
    pub fn has(&self, key: &[u8]) -> Result<bool, String> {
        let store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store: {e}"))?;
        store.has(key).map_err(|e| e.to_string())
    }

    /// Iterate over a range of keys using prefix iteration (simplified)
    pub fn range(
        &self,
        start: Option<&[u8]>,
        _end: Option<&[u8]>,
        limit: u32,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
        let store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store: {e}"))?;

        // For simplicity, use prefix_iterator with the start key as prefix
        // In a full implementation, this would need proper range support
        let prefix = start.unwrap_or(&[]);
        let mut results = Vec::new();
        let mut count = 0;

        for (key, value) in store.prefix_iterator(prefix) {
            if count >= limit {
                break;
            }
            results.push((key, value));
            count += 1;
        }

        Ok(results)
    }
}

/// KVStore resource host implementation
pub struct KVStoreResourceHost {
    /// Available stores by name
    stores: Arc<Mutex<HashMap<String, Arc<Mutex<dyn KVStore>>>>>,
}

impl KVStoreResourceHost {
    pub fn new() -> Self {
        Self {
            stores: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Mount a KVStore with the given name
    pub fn mount_store(&self, name: String, store: Arc<Mutex<dyn KVStore>>) -> Result<(), String> {
        let mut stores = self
            .stores
            .lock()
            .map_err(|e| format!("Failed to lock stores: {e}"))?;
        stores.insert(name, store);
        Ok(())
    }

    /// Open a store by name and create a resource handle
    pub fn open_store(
        &self,
        table: &mut ResourceTable,
        name: &str,
    ) -> Result<Resource<KVStoreResource>, String> {
        let stores = self
            .stores
            .lock()
            .map_err(|e| format!("Failed to lock stores: {e}"))?;

        let store = stores
            .get(name)
            .ok_or_else(|| format!("Store '{name}' not found"))?
            .clone();

        let resource = KVStoreResource::new(name.to_string(), store);

        table
            .push(resource)
            .map_err(|e| format!("Failed to create resource: {e}"))
    }

    /// Get a KVStore resource from a handle
    pub fn get_resource<'a>(
        &self,
        table: &'a mut ResourceTable,
        handle: Resource<KVStoreResource>,
    ) -> Result<&'a mut KVStoreResource, String> {
        table
            .get_mut(&handle)
            .map_err(|e| format!("Failed to get resource: {e}"))
    }

    /// Delete a KVStore resource handle
    pub fn delete_resource(
        &self,
        table: &mut ResourceTable,
        handle: Resource<KVStoreResource>,
    ) -> Result<(), String> {
        table
            .delete(handle)
            .map_err(|e| format!("Failed to delete resource: {e}"))?;
        Ok(())
    }
}

impl Default for KVStoreResourceHost {
    fn default() -> Self {
        Self::new()
    }
}
