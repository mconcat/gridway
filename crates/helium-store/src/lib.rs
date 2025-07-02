//! Storage layer implementation for the helium blockchain framework.
//!
//! This crate provides key-value storage abstractions and implementations
//! for helium applications, including multi-store support and caching layers.

pub mod global;
pub mod jmt;
pub mod state;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

pub use global::{GlobalAppStore, NamespacedStore};
pub use jmt::{Hash, JMTStore, VersionedJMTStore};
pub use state::StateManager;

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
}

/// Result type for store operations
pub type Result<T> = std::result::Result<T, StoreError>;

/// Basic key-value store trait
pub trait KVStore: Send + Sync {
    /// Get a value by key
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Set a key-value pair
    fn set(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()>;

    /// Delete a key
    fn delete(&mut self, key: &[u8]) -> Result<()>;

    /// Check if a key exists
    fn has(&self, key: &[u8]) -> Result<bool> {
        Ok(self.get(key)?.is_some())
    }

    /// Iterate over keys with a prefix
    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>>;
}

// Implement KVStore for Box<dyn KVStore>
impl KVStore for Box<dyn KVStore> {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        (**self).get(key)
    }

    fn set(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        (**self).set(key, value)
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        (**self).delete(key)
    }

    fn has(&self, key: &[u8]) -> Result<bool> {
        (**self).has(key)
    }

    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>> {
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

    fn set(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        self.data.insert(key, value);
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.data.remove(key);
        Ok(())
    }

    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>> {
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
    /// Track cache hit/miss statistics for performance monitoring
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
}

impl<S: KVStore> CacheStore<S> {
    /// Create a new cache store wrapping an inner store
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            cache: HashMap::new(),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
        }
    }

    /// Write all cached changes to the underlying store
    pub fn write(&mut self) -> Result<()> {
        for (key, value) in self.cache.drain() {
            match value {
                Some(v) => self.inner.set(key, v)?,
                None => self.inner.delete(&key)?,
            }
        }
        Ok(())
    }

    /// Consume the cache store and return the inner store after writing changes
    pub fn into_inner(mut self) -> Result<S> {
        self.write()?;
        Ok(self.inner)
    }

    /// Discard all cached changes
    pub fn discard(&mut self) {
        self.cache.clear();
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
    }

    /// Invalidate a specific key in the cache
    pub fn invalidate(&mut self, key: &[u8]) {
        self.cache.remove(key);
    }

    /// Invalidate all keys matching a prefix
    pub fn invalidate_prefix(&mut self, prefix: &[u8]) {
        self.cache.retain(|k, _| !k.starts_with(prefix));
    }

    /// Get the number of cached entries
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Check if a key is cached
    pub fn is_cached(&self, key: &[u8]) -> bool {
        self.cache.contains_key(key)
    }

    /// Get a snapshot of all cached changes
    pub fn get_cached_changes(&self) -> HashMap<Vec<u8>, Option<Vec<u8>>> {
        self.cache.clone()
    }

    /// Get cache hit rate (hits / (hits + misses))
    pub fn cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Get cache statistics (hits, misses)
    pub fn cache_stats(&self) -> (u64, u64) {
        (
            self.cache_hits.load(Ordering::Relaxed),
            self.cache_misses.load(Ordering::Relaxed),
        )
    }

    /// Reset cache statistics
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
        // Cache miss - get from inner store
        let result = self.inner.get(key);
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
        result
    }

    fn set(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        self.cache.insert(key, Some(value));
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.cache.insert(key.to_vec(), None);
        Ok(())
    }

    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>> {
        // Create a combined iterator that merges cached and inner store entries
        let prefix_vec = prefix.to_vec();
        let prefix_clone = prefix_vec.clone();

        // Get cached entries matching the prefix
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

        // Sort cached entries by key for deterministic ordering
        cached_entries.sort_by(|a, b| a.0.cmp(&b.0));

        // Get inner store entries
        let inner_iter = self.inner.prefix_iterator(&prefix_clone);

        // Track which keys are in the cache (including deletions)
        let cache_keys: HashSet<Vec<u8>> = self
            .cache
            .keys()
            .filter(|k| k.starts_with(&prefix_vec))
            .cloned()
            .collect();

        // Filter out inner entries that are overridden in cache
        let filtered_inner: Vec<(Vec<u8>, Vec<u8>)> = inner_iter
            .filter(|(k, _)| !cache_keys.contains(k))
            .collect();

        // Merge cached and filtered inner entries
        let mut all_entries = cached_entries;
        all_entries.extend(filtered_inner);

        // Sort all entries by key
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

        // Test basic operations
        assert!(store.get(b"key1").unwrap().is_none());

        store.set(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        store.delete(b"key1").unwrap();
        assert!(store.get(b"key1").unwrap().is_none());
    }

    #[test]
    fn test_cache_store_basic() {
        let inner = MemStore::new();
        let mut cache = CacheStore::new(inner);

        // Test basic get/set operations
        cache.set(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        assert_eq!(cache.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        // Test cache size
        assert_eq!(cache.cache_size(), 1);
        assert!(cache.is_cached(b"key1"));

        // Test delete
        cache.delete(b"key1").unwrap();
        assert_eq!(cache.get(b"key1").unwrap(), None);
        assert_eq!(cache.cache_size(), 1); // Still in cache as None
    }

    #[test]
    fn test_cache_store_write() {
        let mut inner = MemStore::new();
        inner
            .set(b"existing".to_vec(), b"original".to_vec())
            .unwrap();

        let mut cache = CacheStore::new(inner);

        // Modify existing key
        cache
            .set(b"existing".to_vec(), b"modified".to_vec())
            .unwrap();

        // Add new key
        cache.set(b"new".to_vec(), b"value".to_vec()).unwrap();

        // Delete a key
        cache.set(b"to_delete".to_vec(), b"temp".to_vec()).unwrap();
        cache.delete(b"to_delete").unwrap();

        // Write changes
        cache.write().unwrap();

        // Cache should be empty after write
        assert_eq!(cache.cache_size(), 0);
    }

    #[test]
    fn test_cache_store_discard() {
        let inner = MemStore::new();
        let mut cache = CacheStore::new(inner);

        cache.set(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        cache.set(b"key2".to_vec(), b"value2".to_vec()).unwrap();
        assert_eq!(cache.cache_size(), 2);

        cache.discard();
        assert_eq!(cache.cache_size(), 0);
    }

    #[test]
    fn test_cache_invalidation() {
        let inner = MemStore::new();
        let mut cache = CacheStore::new(inner);

        cache.set(b"key1".to_vec(), b"value1".to_vec()).unwrap();
        cache.set(b"key2".to_vec(), b"value2".to_vec()).unwrap();

        // Invalidate specific key
        cache.invalidate(b"key1");
        assert!(!cache.is_cached(b"key1"));
        assert!(cache.is_cached(b"key2"));

        // Invalidate by prefix
        cache.set(b"prefix:a".to_vec(), b"a".to_vec()).unwrap();
        cache.set(b"prefix:b".to_vec(), b"b".to_vec()).unwrap();
        cache.set(b"other:c".to_vec(), b"c".to_vec()).unwrap();

        cache.invalidate_prefix(b"prefix:");
        assert!(!cache.is_cached(b"prefix:a"));
        assert!(!cache.is_cached(b"prefix:b"));
        assert!(cache.is_cached(b"other:c"));
    }

    #[test]
    fn test_cache_aware_prefix_iterator() {
        let mut inner = MemStore::new();
        inner.set(b"app:key1".to_vec(), b"inner1".to_vec()).unwrap();
        inner.set(b"app:key2".to_vec(), b"inner2".to_vec()).unwrap();
        inner.set(b"app:key3".to_vec(), b"inner3".to_vec()).unwrap();
        inner.set(b"other:key".to_vec(), b"other".to_vec()).unwrap();

        let mut cache = CacheStore::new(inner);

        // Override one key in cache
        cache
            .set(b"app:key2".to_vec(), b"cached2".to_vec())
            .unwrap();

        // Add new key in cache
        cache
            .set(b"app:key4".to_vec(), b"cached4".to_vec())
            .unwrap();

        // Delete one key
        cache.delete(b"app:key3").unwrap();

        // Iterate with prefix
        let items: Vec<_> = cache.prefix_iterator(b"app:").collect();

        // Should return: key1 (from inner), key2 (from cache), key4 (from cache)
        // Should NOT return: key3 (deleted)
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], (b"app:key1".to_vec(), b"inner1".to_vec()));
        assert_eq!(items[1], (b"app:key2".to_vec(), b"cached2".to_vec()));
        assert_eq!(items[2], (b"app:key4".to_vec(), b"cached4".to_vec()));
    }

    #[test]
    fn test_cache_store_get_changes() {
        let inner = MemStore::new();
        let mut cache = CacheStore::new(inner);

        cache.set(b"add".to_vec(), b"new_value".to_vec()).unwrap();
        cache.delete(b"remove").unwrap();

        let changes = cache.get_cached_changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(
            changes.get(b"add".as_slice()),
            Some(&Some(b"new_value".to_vec()))
        );
        assert_eq!(changes.get(b"remove".as_slice()), Some(&None));
    }

    #[test]
    fn test_cache_performance_metrics() {
        let mut inner = MemStore::new();
        inner.set(b"existing".to_vec(), b"value".to_vec()).unwrap();

        let mut cache = CacheStore::new(inner);

        // Initial stats should be zero
        assert_eq!(cache.cache_stats(), (0, 0));
        assert_eq!(cache.cache_hit_rate(), 0.0);

        // Cache miss on first get
        assert_eq!(cache.get(b"existing").unwrap(), Some(b"value".to_vec()));
        assert_eq!(cache.cache_stats(), (0, 1));
        assert_eq!(cache.cache_hit_rate(), 0.0);

        // Cache hit on subsequent get of cached value
        cache
            .set(b"cached".to_vec(), b"cached_value".to_vec())
            .unwrap();
        assert_eq!(
            cache.get(b"cached").unwrap(),
            Some(b"cached_value".to_vec())
        );
        assert_eq!(cache.cache_stats(), (1, 1));
        assert_eq!(cache.cache_hit_rate(), 0.5);

        // Another cache hit
        assert_eq!(
            cache.get(b"cached").unwrap(),
            Some(b"cached_value".to_vec())
        );
        assert_eq!(cache.cache_stats(), (2, 1));
        assert!((cache.cache_hit_rate() - 0.666667).abs() < 0.001);

        // Reset stats
        cache.reset_stats();
        assert_eq!(cache.cache_stats(), (0, 0));
        assert_eq!(cache.cache_hit_rate(), 0.0);

        // Discard should also reset stats
        cache.set(b"temp".to_vec(), b"temp".to_vec()).unwrap();
        let _ = cache.get(b"temp");
        cache.discard();
        assert_eq!(cache.cache_stats(), (0, 0));
    }
}
