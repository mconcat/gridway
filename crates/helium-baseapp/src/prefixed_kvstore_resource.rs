//! Prefixed KVStore Resource Implementation
//!
//! This module provides prefix-based access control for KVStore resources.
//! Each resource handle is bound to a specific key prefix, and all operations
//! are automatically scoped to that prefix.

use helium_store::KVStore;
use std::sync::{Arc, Mutex};

/// A KVStore resource that enforces prefix-based access control
#[derive(Clone)]
pub struct PrefixedKVStore {
    /// The key prefix this resource is bound to (e.g., "/ante/")
    prefix: Vec<u8>,
    /// The underlying KVStore implementation
    store: Arc<Mutex<dyn KVStore>>,
}

impl PrefixedKVStore {
    /// Create a new prefixed KVStore with the given prefix
    pub fn new(prefix: Vec<u8>, store: Arc<Mutex<dyn KVStore>>) -> Self {
        Self { prefix, store }
    }

    /// Create a new prefixed KVStore from a string prefix
    pub fn new_from_str(prefix: &str, store: Arc<Mutex<dyn KVStore>>) -> Self {
        Self::new(prefix.as_bytes().to_vec(), store)
    }

    /// Get the prefix for this store
    pub fn prefix(&self) -> &[u8] {
        &self.prefix
    }

    /// Prepend the prefix to a key
    fn make_key(&self, key: &[u8]) -> Vec<u8> {
        let mut full_key = self.prefix.clone();
        full_key.extend_from_slice(key);
        full_key
    }

    /// Remove the prefix from a key (for return values)
    fn strip_prefix(&self, key: &[u8]) -> Option<Vec<u8>> {
        key.strip_prefix(self.prefix.as_slice()).map(|k| k.to_vec())
    }

    /// Get a value by key (automatically prefixed)
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let full_key = self.make_key(key);
        let store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store: {e}"))?;
        store.get(&full_key).map_err(|e| e.to_string())
    }

    /// Set a key-value pair (automatically prefixed)
    pub fn set(&self, key: &[u8], value: &[u8]) -> Result<(), String> {
        let full_key = self.make_key(key);
        let mut store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store: {e}"))?;
        store.set(&full_key, value).map_err(|e| e.to_string())
    }

    /// Delete a key (automatically prefixed)
    pub fn delete(&self, key: &[u8]) -> Result<(), String> {
        let full_key = self.make_key(key);
        let mut store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store: {e}"))?;
        store.delete(&full_key).map_err(|e| e.to_string())
    }

    /// Check if a key exists (automatically prefixed)
    pub fn has(&self, key: &[u8]) -> Result<bool, String> {
        let full_key = self.make_key(key);
        let store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store: {e}"))?;
        store.has(&full_key).map_err(|e| e.to_string())
    }

    /// Iterate over a range of keys within this prefix
    pub fn range(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
        limit: u32,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
        let store = self
            .store
            .lock()
            .map_err(|e| format!("Failed to lock store: {e}"))?;

        // Construct the full range with prefix
        let full_start = start
            .map(|s| self.make_key(s))
            .unwrap_or_else(|| self.prefix.clone());

        // For the end key, we need to be careful to stay within our prefix
        let full_end = if let Some(e) = end {
            Some(self.make_key(e))
        } else {
            // If no end is specified, we want all keys with our prefix
            // Create an end key that's just past our prefix range
            let mut prefix_end = self.prefix.clone();
            // Increment the last byte to get the exclusive upper bound
            if let Some(last) = prefix_end.last_mut() {
                *last = last.saturating_add(1);
            } else {
                // Empty prefix means scan everything
                prefix_end.push(0xFF);
            }
            Some(prefix_end)
        };

        let mut results = Vec::new();
        let mut count = 0;

        // Use prefix iterator to ensure we only get keys with our prefix
        for (key, value) in store.prefix_iterator(&self.prefix) {
            // Apply start/end bounds if specified
            if key < full_start {
                continue;
            }
            if let Some(e) = &full_end {
                if &key >= e {
                    break;
                }
            }

            if count >= limit {
                break;
            }

            // Strip the prefix from the returned key
            if let Some(stripped_key) = self.strip_prefix(&key) {
                results.push((stripped_key, value));
                count += 1;
            }
        }

        Ok(results)
    }

    /// Create a sub-prefixed store from this store
    /// For example, if this store has prefix "/ante/", calling sub_prefix("accounts/")
    /// creates a new store with prefix "/ante/accounts/"
    pub fn sub_prefix(&self, sub: &str) -> Self {
        let mut new_prefix = self.prefix.clone();
        new_prefix.extend_from_slice(sub.as_bytes());
        Self::new(new_prefix, self.store.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helium_store::MemStore;

    #[test]
    fn test_prefixed_kvstore() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let prefixed = PrefixedKVStore::new_from_str("/ante/", base_store.clone());

        // Test set and get with prefix
        prefixed.set(b"key1", b"value1").unwrap();
        assert_eq!(prefixed.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        // Verify the actual key in the base store includes the prefix
        let base = base_store.lock().unwrap();
        assert_eq!(base.get(b"/ante/key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(base.get(b"key1").unwrap(), None); // Without prefix, not found
    }

    #[test]
    fn test_prefix_isolation() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));

        // Create two prefixed stores
        let ante_store = PrefixedKVStore::new_from_str("/ante/", base_store.clone());
        let bank_store = PrefixedKVStore::new_from_str("/bank/", base_store.clone());

        // Set same key in both stores
        ante_store.set(b"balance", b"100").unwrap();
        bank_store.set(b"balance", b"200").unwrap();

        // Verify isolation
        assert_eq!(ante_store.get(b"balance").unwrap(), Some(b"100".to_vec()));
        assert_eq!(bank_store.get(b"balance").unwrap(), Some(b"200".to_vec()));

        // Verify actual keys in base store
        let base = base_store.lock().unwrap();
        assert_eq!(base.get(b"/ante/balance").unwrap(), Some(b"100".to_vec()));
        assert_eq!(base.get(b"/bank/balance").unwrap(), Some(b"200".to_vec()));
    }

    #[test]
    fn test_range_query() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let prefixed = PrefixedKVStore::new_from_str("/test/", base_store);

        // Set some keys
        prefixed.set(b"a", b"1").unwrap();
        prefixed.set(b"b", b"2").unwrap();
        prefixed.set(b"c", b"3").unwrap();

        // Range query
        let results = prefixed.range(None, None, 10).unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0], (b"a".to_vec(), b"1".to_vec()));
        assert_eq!(results[1], (b"b".to_vec(), b"2".to_vec()));
        assert_eq!(results[2], (b"c".to_vec(), b"3".to_vec()));

        // Limited range query
        let results = prefixed.range(Some(b"b"), None, 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], (b"b".to_vec(), b"2".to_vec()));
    }

    #[test]
    fn test_sub_prefix() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let ante_store = PrefixedKVStore::new_from_str("/ante/", base_store.clone());

        // Create sub-prefixed store
        let accounts_store = ante_store.sub_prefix("accounts/");

        // Set value in sub-prefixed store
        accounts_store.set(b"user1", b"data").unwrap();

        // Verify the full key path
        let base = base_store.lock().unwrap();
        assert_eq!(
            base.get(b"/ante/accounts/user1").unwrap(),
            Some(b"data".to_vec())
        );
    }
}
