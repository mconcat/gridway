//! KVStore Resource Tests
//!
//! Test the KVStore resource implementation for WASI components.

#[cfg(test)]
mod tests {
    use super::super::component_bindings::SimpleKVStoreManager;
    use helium_store::MemStore;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_kvstore_manager_basic_operations() {
        let manager = SimpleKVStoreManager::new();

        // Create a test store
        let store: Arc<Mutex<dyn helium_store::KVStore>> = Arc::new(Mutex::new(MemStore::new()));

        // Mount the store
        manager
            .mount_store("test_store".to_string(), store.clone())
            .unwrap();

        // Verify we can retrieve the store
        let retrieved_store = manager.get_store("test_store").unwrap();

        // Test basic operations on the retrieved store
        {
            let mut s = retrieved_store.lock().unwrap();
            s.set(b"key1", b"value1").unwrap();
            let value = s.get(b"key1").unwrap();
            assert_eq!(value, Some(b"value1".to_vec()));
        }

        // List stores
        let store_names = manager.list_stores().unwrap();
        assert_eq!(store_names, vec!["test_store"]);
    }

    #[test]
    fn test_kvstore_manager_multiple_stores() {
        let manager = SimpleKVStoreManager::new();

        // Create multiple test stores
        let store1: Arc<Mutex<dyn helium_store::KVStore>> = Arc::new(Mutex::new(MemStore::new()));
        let store2: Arc<Mutex<dyn helium_store::KVStore>> = Arc::new(Mutex::new(MemStore::new()));

        // Mount the stores
        manager.mount_store("store1".to_string(), store1).unwrap();
        manager.mount_store("store2".to_string(), store2).unwrap();

        // Test that we can access both stores independently
        {
            let s1 = manager.get_store("store1").unwrap();
            let mut s1_lock = s1.lock().unwrap();
            s1_lock.set(b"key1", b"value1").unwrap();
        }

        {
            let s2 = manager.get_store("store2").unwrap();
            let mut s2_lock = s2.lock().unwrap();
            s2_lock.set(b"key2", b"value2").unwrap();
        }

        // Verify isolation
        {
            let s1 = manager.get_store("store1").unwrap();
            let s1_lock = s1.lock().unwrap();
            assert_eq!(s1_lock.get(b"key1").unwrap(), Some(b"value1".to_vec()));
            assert_eq!(s1_lock.get(b"key2").unwrap(), None);
        }

        {
            let s2 = manager.get_store("store2").unwrap();
            let s2_lock = s2.lock().unwrap();
            assert_eq!(s2_lock.get(b"key2").unwrap(), Some(b"value2".to_vec()));
            assert_eq!(s2_lock.get(b"key1").unwrap(), None);
        }

        // List stores
        let mut store_names = manager.list_stores().unwrap();
        store_names.sort();
        assert_eq!(store_names, vec!["store1", "store2"]);
    }

    #[test]
    fn test_kvstore_manager_nonexistent_store() {
        let manager = SimpleKVStoreManager::new();

        // Try to get a non-existent store
        let result = manager.get_store("nonexistent");
        assert!(result.is_err());
        let error_msg = result.err().unwrap();
        assert!(error_msg.contains("not found"));
    }
}
