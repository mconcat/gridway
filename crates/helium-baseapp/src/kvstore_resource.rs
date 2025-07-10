//! KVStore Resource Implementation
//!
//! This module provides the WASI resource implementation for KVStore access,
//! allowing WASI components to interact with blockchain state through resource handles.

use crate::prefixed_kvstore_resource::PrefixedKVStore;
use helium_store::KVStore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::component::Resource;
use wasmtime_wasi::ResourceTable;

/// KVStore resource data that will be stored in the ResourceTable
pub struct KVStoreResource {
    /// Name of the store (for debugging/logging)
    pub name: String,
    /// Prefixed KVStore implementation that enforces access control
    pub store: PrefixedKVStore,
}

impl KVStoreResource {
    /// Create a new KVStore resource with a specific prefix
    pub fn new(name: String, prefix: &str, store: Arc<Mutex<dyn KVStore>>) -> Self {
        Self {
            name,
            store: PrefixedKVStore::new_from_str(prefix, store),
        }
    }

    /// Create from an existing PrefixedKVStore
    pub fn from_prefixed(name: String, store: PrefixedKVStore) -> Self {
        Self { name, store }
    }

    /// Get a value by key (automatically prefixed)
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, String> {
        self.store.get(key)
    }

    /// Set a key-value pair (automatically prefixed)
    pub fn set(&self, key: &[u8], value: &[u8]) -> Result<(), String> {
        self.store.set(key, value)
    }

    /// Delete a key (automatically prefixed)
    pub fn delete(&self, key: &[u8]) -> Result<(), String> {
        self.store.delete(key)
    }

    /// Check if a key exists (automatically prefixed)
    pub fn has(&self, key: &[u8]) -> Result<bool, String> {
        self.store.has(key)
    }

    /// Iterate over a range of keys within the prefix
    pub fn range(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        limit: u32,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
        self.store.range(start, end, limit)
    }
}

/// KVStore resource host implementation
#[derive(Clone)]
pub struct KVStoreResourceHost {
    /// Base KVStore implementation (usually the global merkle store)
    base_store: Arc<Mutex<dyn KVStore>>,
    /// Map of component names to their allowed prefixes
    component_prefixes: Arc<Mutex<HashMap<String, String>>>,
}

impl KVStoreResourceHost {
    pub fn new(base_store: Arc<Mutex<dyn KVStore>>) -> Self {
        Self {
            base_store,
            component_prefixes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a component with its allowed prefix
    /// For example: ("ante-handler", "/ante/")
    pub fn register_component_prefix(
        &self,
        component_name: String,
        prefix: String,
    ) -> Result<(), String> {
        let mut prefixes = self
            .component_prefixes
            .lock()
            .map_err(|e| format!("Failed to lock prefixes: {e}"))?;
        prefixes.insert(component_name, prefix);
        Ok(())
    }

    /// Open a store by name and create a resource handle
    /// The name parameter should match a registered component name
    pub fn open_store(
        &self,
        table: &mut ResourceTable,
        name: &str,
    ) -> Result<Resource<KVStoreResource>, String> {
        let prefixes = self
            .component_prefixes
            .lock()
            .map_err(|e| format!("Failed to lock prefixes: {e}"))?;

        let prefix = prefixes
            .get(name)
            .ok_or_else(|| format!("No prefix registered for component '{name}'"))?
            .clone();

        let resource = KVStoreResource::new(name.to_string(), &prefix, self.base_store.clone());

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
