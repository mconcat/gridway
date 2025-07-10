#[cfg(test)]
mod tests {
    use crate::kvstore_resource::KVStoreResourceHost;
    use crate::prefixed_kvstore_resource::PrefixedKVStore;
    use helium_store::{KVStore, MemStore};
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_prefixed_kvstore_isolation() {
        // Create a base store
        let base_store = Arc::new(Mutex::new(MemStore::new()));

        // Create prefixed stores for different components
        let ante_store = PrefixedKVStore::new_from_str("/ante/", base_store.clone());
        let bank_store = PrefixedKVStore::new_from_str("/bank/", base_store.clone());

        // Write to ante store
        ante_store.set(b"test_key", b"ante_value").unwrap();
        
        // Write to bank store with same key
        bank_store.set(b"test_key", b"bank_value").unwrap();

        // Verify isolation - each store has its own value
        assert_eq!(
            ante_store.get(b"test_key").unwrap(),
            Some(b"ante_value".to_vec())
        );
        assert_eq!(
            bank_store.get(b"test_key").unwrap(),
            Some(b"bank_value".to_vec())
        );

        // Verify the actual keys in base store
        let base = base_store.lock().unwrap();
        assert_eq!(
            base.get(b"/ante/test_key").unwrap(),
            Some(b"ante_value".to_vec())
        );
        assert_eq!(
            base.get(b"/bank/test_key").unwrap(),
            Some(b"bank_value".to_vec())
        );
    }

    #[test]
    fn test_kvstore_resource_host() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let host = KVStoreResourceHost::new(base_store.clone());

        // Register component prefixes
        host.register_component_prefix("ante-handler".to_string(), "/ante/".to_string())
            .unwrap();
        host.register_component_prefix("bank-module".to_string(), "/bank/".to_string())
            .unwrap();

        // Create resource table
        let mut table = wasmtime_wasi::ResourceTable::new();

        // Open stores for different components
        let ante_resource = host.open_store(&mut table, "ante-handler").unwrap();
        let bank_resource = host.open_store(&mut table, "bank-module").unwrap();

        // Get resources and use them
        {
            let ante_store = host.get_resource(&mut table, ante_resource).unwrap();
            ante_store.set(b"balance", b"100").unwrap();
            assert_eq!(ante_store.get(b"balance").unwrap(), Some(b"100".to_vec()));
        }

        {
            let bank_store = host.get_resource(&mut table, bank_resource).unwrap();
            bank_store.set(b"balance", b"200").unwrap();
            assert_eq!(bank_store.get(b"balance").unwrap(), Some(b"200".to_vec()));
        }

        // Verify in base store
        let base = base_store.lock().unwrap();
        assert_eq!(
            base.get(b"/ante/balance").unwrap(),
            Some(b"100".to_vec())
        );
        assert_eq!(
            base.get(b"/bank/balance").unwrap(),
            Some(b"200".to_vec())
        );
    }

    #[test]
    fn test_default_prefixes() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let host = KVStoreResourceHost::new(base_store.clone());
        
        // Register default prefixes that ComponentHost would register
        host.register_component_prefix("ante-handler".to_string(), "/ante/".to_string())
            .unwrap();
        host.register_component_prefix("begin-blocker".to_string(), "/begin/".to_string())
            .unwrap();
        host.register_component_prefix("end-blocker".to_string(), "/end/".to_string())
            .unwrap();
        host.register_component_prefix("tx-decoder".to_string(), "/decoder/".to_string())
            .unwrap();
        
        // Create resource table
        let mut table = wasmtime_wasi::ResourceTable::new();
        
        // Open stores for components
        let ante_resource = host.open_store(&mut table, "ante-handler").unwrap();
        let begin_resource = host.open_store(&mut table, "begin-blocker").unwrap();
        
        // Write to each store
        {
            let ante_store = host.get_resource(&mut table, ante_resource).unwrap();
            ante_store.set(b"test", b"ante_data").unwrap();
        }
        
        {
            let begin_store = host.get_resource(&mut table, begin_resource).unwrap();
            begin_store.set(b"test", b"begin_data").unwrap();
        }
        
        // Verify isolation
        let base = base_store.lock().unwrap();
        assert_eq!(
            base.get(b"/ante/test").unwrap(),
            Some(b"ante_data".to_vec())
        );
        assert_eq!(
            base.get(b"/begin/test").unwrap(),
            Some(b"begin_data".to_vec())
        );
    }

    #[test]
    fn test_sub_prefix_functionality() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let ante_store = PrefixedKVStore::new_from_str("/ante/", base_store.clone());

        // Create sub-prefixed stores
        let accounts_store = ante_store.sub_prefix("accounts/");
        let params_store = ante_store.sub_prefix("params/");

        // Write to sub-stores
        accounts_store.set(b"alice", b"1000").unwrap();
        params_store.set(b"min_fee", b"10").unwrap();

        // Read from sub-stores
        assert_eq!(
            accounts_store.get(b"alice").unwrap(),
            Some(b"1000".to_vec())
        );
        assert_eq!(
            params_store.get(b"min_fee").unwrap(),
            Some(b"10".to_vec())
        );

        // Verify full paths in base store
        let base = base_store.lock().unwrap();
        assert_eq!(
            base.get(b"/ante/accounts/alice").unwrap(),
            Some(b"1000".to_vec())
        );
        assert_eq!(
            base.get(b"/ante/params/min_fee").unwrap(),
            Some(b"10".to_vec())
        );
    }

    #[test]
    fn test_range_queries_with_prefix() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let ante_store = PrefixedKVStore::new_from_str("/ante/", base_store.clone());

        // Add some keys
        ante_store.set(b"key1", b"value1").unwrap();
        ante_store.set(b"key2", b"value2").unwrap();
        ante_store.set(b"key3", b"value3").unwrap();

        // Add a key in a different prefix to ensure isolation
        let bank_store = PrefixedKVStore::new_from_str("/bank/", base_store.clone());
        bank_store.set(b"key2", b"bank_value").unwrap();

        // Range query in ante store
        let results = ante_store.range(None, None, 10).unwrap();
        assert_eq!(results.len(), 3);
        
        // Verify keys are returned without prefix
        assert_eq!(results[0].0, b"key1");
        assert_eq!(results[1].0, b"key2");
        assert_eq!(results[2].0, b"key3");
        
        // Verify values
        assert_eq!(results[0].1, b"value1");
        assert_eq!(results[1].1, b"value2");
        assert_eq!(results[2].1, b"value3");

        // Limited range query
        let results = ante_store.range(Some(b"key2"), None, 2).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, b"key2");
        assert_eq!(results[1].0, b"key3");
    }
}