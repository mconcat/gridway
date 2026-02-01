//! Storage layer for gridway.
//!
//! Provides key-value storage abstractions and a simple hash-based Merkle store.
//! The JMT/RocksDB backend has been replaced with an in-memory store that computes
//! deterministic state root hashes for consensus. This is suitable for the
//! experimental Commonware migration — production use would want a persistent backend.

pub mod global;
pub mod merkle;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

pub use global::{GlobalAppStore, NamespacedStore};
pub use merkle::MerkleStore;

/// Store error types
#[derive(Error, Debug)]
pub enum StoreError {
    #[error("key not found")]
    KeyNotFound,

    #[error("write failed: {0}")]
    WriteFailed(String),

    #[error("read failed: {0}")]
    ReadFailed(String),

    #[error("invalid key: {0}")]
    InvalidKey(String),

    #[error("invalid value: {0}")]
    InvalidValue(String),

    #[error("store not found: {0}")]
    StoreNotFound(String),

    #[error("backend error: {0}")]
    BackendError(String),

    #[error("backend error: {0}")]
    Backend(String),

    #[error("invalid data: {0}")]
    InvalidData(String),

    #[error("invalid config: {0}")]
    InvalidConfig(String),
}

/// Result type for store operations
pub type Result<T> = std::result::Result<T, StoreError>;

/// A 32-byte hash used for state roots
pub type Hash = [u8; 32];

/// Basic key-value store trait — the core interface that VFS and WASM modules use.
pub trait KVStore: Send + Sync {
    /// Get a value by key
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Set a key-value pair
    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()>;

    /// Delete a key
    fn delete(&mut self, key: &[u8]) -> Result<()>;

    /// Check if a key exists
    fn has(&self, key: &[u8]) -> Result<bool> {
        Ok(self.get(key)?.is_some())
    }

    /// Iterate over keys with a prefix
    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_>;
}

// Implement KVStore for Box<dyn KVStore>
impl KVStore for Box<dyn KVStore> {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        (**self).get(key)
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        (**self).set(key, value)
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        (**self).delete(key)
    }

    fn has(&self, key: &[u8]) -> Result<bool> {
        (**self).has(key)
    }

    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        (**self).prefix_iterator(prefix)
    }
}

/// In-memory key-value store implementation
pub struct MemStore {
    data: HashMap<Vec<u8>, Vec<u8>>,
}

impl MemStore {
    /// Create a new memory store
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }
}

impl Default for MemStore {
    fn default() -> Self {
        Self::new()
    }
}

impl KVStore for MemStore {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self.data.get(key).cloned())
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.data.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.data.remove(key);
        Ok(())
    }

    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        let prefix = prefix.to_vec();
        let mut items: Vec<_> = self
            .data
            .iter()
            .filter_map(|(k, v)| {
                if k.starts_with(&prefix) {
                    Some((k.clone(), v.clone()))
                } else {
                    None
                }
            })
            .collect();
        items.sort_by(|(a, _), (b, _)| a.cmp(b));
        Box::new(items.into_iter())
    }
}

/// Cache layer for stores
pub struct CacheStore<S: KVStore> {
    inner: S,
    cache: HashMap<Vec<u8>, Option<Vec<u8>>>,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
}

impl<S: KVStore> CacheStore<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            cache: HashMap::new(),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
        }
    }

    pub fn write(&mut self) -> Result<()> {
        for (key, value) in self.cache.drain() {
            match value {
                Some(v) => self.inner.set(&key, &v)?,
                None => self.inner.delete(&key)?,
            }
        }
        Ok(())
    }

    pub fn discard(&mut self) {
        self.cache.clear();
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
    }

    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    pub fn is_cached(&self, key: &[u8]) -> bool {
        self.cache.contains_key(key)
    }

    pub fn invalidate(&mut self, key: &[u8]) {
        self.cache.remove(key);
    }

    pub fn invalidate_prefix(&mut self, prefix: &[u8]) {
        self.cache.retain(|k, _| !k.starts_with(prefix));
    }

    pub fn get_cached_changes(&self) -> HashMap<Vec<u8>, Option<Vec<u8>>> {
        self.cache.clone()
    }

    pub fn cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 { 0.0 } else { hits as f64 / total as f64 }
    }

    pub fn cache_stats(&self) -> (u64, u64) {
        (
            self.cache_hits.load(Ordering::Relaxed),
            self.cache_misses.load(Ordering::Relaxed),
        )
    }

    pub fn reset_stats(&mut self) {
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
    }
}

impl<S: KVStore> KVStore for CacheStore<S> {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if let Some(cached) = self.cache.get(key) {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(cached.clone());
        }
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
        self.inner.get(key)
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.cache.insert(key.to_vec(), Some(value.to_vec()));
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.cache.insert(key.to_vec(), None);
        Ok(())
    }

    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        let prefix_vec = prefix.to_vec();
        let prefix_clone = prefix_vec.clone();

        let mut cached_entries: Vec<(Vec<u8>, Vec<u8>)> = self
            .cache
            .iter()
            .filter_map(|(k, v)| {
                if k.starts_with(&prefix_vec) {
                    v.as_ref().map(|value| (k.clone(), value.clone()))
                } else {
                    None
                }
            })
            .collect();
        cached_entries.sort_by(|a, b| a.0.cmp(&b.0));

        let inner_iter = self.inner.prefix_iterator(&prefix_clone);
        let cache_keys: HashSet<Vec<u8>> = self
            .cache
            .keys()
            .filter(|k| k.starts_with(&prefix_vec))
            .cloned()
            .collect();

        let filtered_inner: Vec<(Vec<u8>, Vec<u8>)> = inner_iter
            .filter(|(k, _)| !cache_keys.contains(k))
            .collect();

        let mut all_entries = cached_entries;
        all_entries.extend(filtered_inner);
        all_entries.sort_by(|a, b| a.0.cmp(&b.0));
        Box::new(all_entries.into_iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mem_store() {
        let mut store = MemStore::new();
        assert!(store.get(b"key1").unwrap().is_none());
        store.set(b"key1", b"value1").unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        store.delete(b"key1").unwrap();
        assert!(store.get(b"key1").unwrap().is_none());
    }

    #[test]
    fn test_cache_store() {
        let inner = MemStore::new();
        let mut cache = CacheStore::new(inner);
        cache.set(b"key1", b"value1").unwrap();
        assert_eq!(cache.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(cache.cache_size(), 1);
        cache.delete(b"key1").unwrap();
        assert_eq!(cache.get(b"key1").unwrap(), None);
    }

    #[test]
    fn test_cache_prefix_iterator() {
        let mut inner = MemStore::new();
        inner.set(b"app:key1", b"inner1").unwrap();
        inner.set(b"app:key2", b"inner2").unwrap();
        inner.set(b"other:key", b"other").unwrap();

        let mut cache = CacheStore::new(inner);
        cache.set(b"app:key2", b"cached2").unwrap();
        cache.set(b"app:key3", b"cached3").unwrap();

        let items: Vec<_> = cache.prefix_iterator(b"app:").collect();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], (b"app:key1".to_vec(), b"inner1".to_vec()));
        assert_eq!(items[1], (b"app:key2".to_vec(), b"cached2".to_vec()));
        assert_eq!(items[2], (b"app:key3".to_vec(), b"cached3".to_vec()));
    }
}
