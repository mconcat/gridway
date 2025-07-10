#[cfg(test)]
mod tests {
    use crate::component_host::ComponentHost;
    use helium_store::{KVStore, MemStore};
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_kvstore_prefix_access_control() {
        // Create a base store
        let base_store = Arc::new(Mutex::new(MemStore::new()));

        // Pre-populate some data in the store
        {
            let mut store = base_store.lock().unwrap();
            // Data for ante-handler
            store.set(b"/ante/config", b"ante_config_value").unwrap();
            store.set(b"/ante/state", b"ante_state_value").unwrap();
            // Data for begin-blocker
            store.set(b"/begin/config", b"begin_config_value").unwrap();
            store.set(b"/begin/state", b"begin_state_value").unwrap();
            // Data for end-blocker
            store.set(b"/end/config", b"end_config_value").unwrap();
            store.set(b"/end/state", b"end_state_value").unwrap();
            // Data outside any prefix
            store.set(b"/other/data", b"other_value").unwrap();
        }

        // Create component host with the store
        let host = ComponentHost::new(base_store.clone()).unwrap();

        // Load the ante-handler component and test it can only access /ante/ prefix
        // This would require having a test ante-handler component that tries to access different keys
        // For now, we'll test the KVStore resource host directly

        // Test that components get proper prefixed access through the component host
        // We'll create a store and test the resource access directly
        let mut table = wasmtime_wasi::ResourceTable::new();
        let kvstore_host = crate::kvstore_resource::KVStoreResourceHost::new(base_store.clone());

        // Register the default prefixes
        kvstore_host
            .register_component_prefix("ante-handler".to_string(), "/ante/".to_string())
            .unwrap();
        kvstore_host
            .register_component_prefix("begin-blocker".to_string(), "/begin/".to_string())
            .unwrap();
        kvstore_host
            .register_component_prefix("end-blocker".to_string(), "/end/".to_string())
            .unwrap();

        // Open store for ante-handler
        let ante_store = kvstore_host.open_store(&mut table, "ante-handler").unwrap();

        // Should be able to read /ante/ keys (without the prefix in the key)
        {
            let ante_store_copy = wasmtime::component::Resource::<
                crate::kvstore_resource::KVStoreResource,
            >::new_own(ante_store.rep());
            let store_resource = kvstore_host
                .get_resource(&mut table, ante_store_copy)
                .unwrap();
            assert_eq!(
                store_resource.get(b"config").unwrap(),
                Some(b"ante_config_value".to_vec())
            );
            assert_eq!(
                store_resource.get(b"state").unwrap(),
                Some(b"ante_state_value".to_vec())
            );
        }

        // Should NOT be able to read keys from other prefixes
        {
            let ante_store_copy = wasmtime::component::Resource::<
                crate::kvstore_resource::KVStoreResource,
            >::new_own(ante_store.rep());
            let store_resource = kvstore_host
                .get_resource(&mut table, ante_store_copy)
                .unwrap();
            // These keys don't exist under /ante/ prefix
            assert_eq!(store_resource.get(b"/begin/config").unwrap(), None);
            assert_eq!(store_resource.get(b"/end/config").unwrap(), None);
            assert_eq!(store_resource.get(b"/other/data").unwrap(), None);
        }

        // Test write isolation
        {
            let ante_store_copy = wasmtime::component::Resource::<
                crate::kvstore_resource::KVStoreResource,
            >::new_own(ante_store.rep());
            let store_resource = kvstore_host
                .get_resource(&mut table, ante_store_copy)
                .unwrap();
            store_resource.set(b"new_key", b"new_value").unwrap();
        }

        // Verify the write went to the correct prefix
        {
            let store = base_store.lock().unwrap();
            assert_eq!(
                store.get(b"/ante/new_key").unwrap(),
                Some(b"new_value".to_vec())
            );
            // Should not exist without prefix
            assert_eq!(store.get(b"new_key").unwrap(), None);
        }

        // Test range queries respect prefix
        {
            let ante_store_copy = wasmtime::component::Resource::<
                crate::kvstore_resource::KVStoreResource,
            >::new_own(ante_store.rep());
            let store_resource = kvstore_host
                .get_resource(&mut table, ante_store_copy)
                .unwrap();
            let range_results = store_resource.range(None, None, 10).unwrap();

            // Should only see keys under /ante/ prefix (without the prefix in results)
            assert_eq!(range_results.len(), 3); // config, new_key, state

            let keys: Vec<String> = range_results
                .iter()
                .map(|(k, _)| String::from_utf8(k.clone()).unwrap())
                .collect();

            assert!(keys.contains(&"config".to_string()));
            assert!(keys.contains(&"state".to_string()));
            assert!(keys.contains(&"new_key".to_string()));
        }
    }

    #[test]
    fn test_kvstore_component_isolation() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let kvstore_host = crate::kvstore_resource::KVStoreResourceHost::new(base_store.clone());

        // Register the default prefixes
        kvstore_host
            .register_component_prefix("ante-handler".to_string(), "/ante/".to_string())
            .unwrap();
        kvstore_host
            .register_component_prefix("begin-blocker".to_string(), "/begin/".to_string())
            .unwrap();

        let mut table = wasmtime_wasi::ResourceTable::new();

        // Open stores for different components
        let ante_store = kvstore_host.open_store(&mut table, "ante-handler").unwrap();
        let begin_store = kvstore_host
            .open_store(&mut table, "begin-blocker")
            .unwrap();

        // Write to ante-handler's store
        {
            let ante_store_copy = wasmtime::component::Resource::<
                crate::kvstore_resource::KVStoreResource,
            >::new_own(ante_store.rep());
            let store_resource = kvstore_host
                .get_resource(&mut table, ante_store_copy)
                .unwrap();
            store_resource.set(b"shared_key", b"ante_value").unwrap();
        }

        // Write to begin-blocker's store with same key
        {
            let begin_store_copy = wasmtime::component::Resource::<
                crate::kvstore_resource::KVStoreResource,
            >::new_own(begin_store.rep());
            let store_resource = kvstore_host
                .get_resource(&mut table, begin_store_copy)
                .unwrap();
            store_resource.set(b"shared_key", b"begin_value").unwrap();
        }

        // Verify isolation - each component sees its own value
        {
            let ante_store_copy = wasmtime::component::Resource::<
                crate::kvstore_resource::KVStoreResource,
            >::new_own(ante_store.rep());
            let ante_resource = kvstore_host
                .get_resource(&mut table, ante_store_copy)
                .unwrap();
            assert_eq!(
                ante_resource.get(b"shared_key").unwrap(),
                Some(b"ante_value".to_vec())
            );

            let begin_store_copy = wasmtime::component::Resource::<
                crate::kvstore_resource::KVStoreResource,
            >::new_own(begin_store.rep());
            let begin_resource = kvstore_host
                .get_resource(&mut table, begin_store_copy)
                .unwrap();
            assert_eq!(
                begin_resource.get(b"shared_key").unwrap(),
                Some(b"begin_value".to_vec())
            );
        }

        // Verify in the underlying store
        {
            let store = base_store.lock().unwrap();
            assert_eq!(
                store.get(b"/ante/shared_key").unwrap(),
                Some(b"ante_value".to_vec())
            );
            assert_eq!(
                store.get(b"/begin/shared_key").unwrap(),
                Some(b"begin_value".to_vec())
            );
        }
    }
}
