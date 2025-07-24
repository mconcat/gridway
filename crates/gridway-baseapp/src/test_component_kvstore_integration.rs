#[cfg(test)]
mod tests {
    use crate::component_host::{ComponentHost, ComponentInfo, ComponentType};
    use crate::kvstore_resource::KVStoreResourceHost;
    use gridway_store::{KVStore, MemStore};
    use std::sync::{Arc, Mutex};

    #[test]
    #[ignore = "Requires kvstore interface which is being removed"]
    fn test_component_kvstore_isolation() {
        // Create a base store
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let host = ComponentHost::new(base_store.clone()).unwrap();

        // Load begin-blocker and end-blocker components
        let begin_blocker_path = std::env::current_dir()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("modules/begin_blocker_component.wasm");

        let end_blocker_path = std::env::current_dir()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("modules/end_blocker_component.wasm");

        if !begin_blocker_path.exists() || !end_blocker_path.exists() {
            eprintln!("Components not found. Run build-wasi-modules.sh first");
            return;
        }

        // Load begin-blocker
        let begin_bytes = std::fs::read(&begin_blocker_path).unwrap();
        let begin_info = ComponentInfo {
            name: "begin-blocker".to_string(),
            path: begin_blocker_path,
            component_type: ComponentType::BeginBlocker,
            gas_limit: 1_000_000,
        };
        host.load_component("begin-blocker", &begin_bytes, begin_info)
            .unwrap();

        // Load end-blocker
        let end_bytes = std::fs::read(&end_blocker_path).unwrap();
        let end_info = ComponentInfo {
            name: "end-blocker".to_string(),
            path: end_blocker_path,
            component_type: ComponentType::EndBlocker,
            gas_limit: 1_000_000,
        };
        host.load_component("end-blocker", &end_bytes, end_info)
            .unwrap();

        // Pre-populate the store with data for each component
        {
            let mut store = base_store.lock().unwrap();
            // Data for begin-blocker
            store
                .set(b"/begin/proposer_address", b"\x01\x02\x03\x04")
                .unwrap();
            // Data for end-blocker
            store
                .set(b"/end/inflation_rate", &0.05f64.to_le_bytes())
                .unwrap();
            store
                .set(b"/end/last_reward_height", &100u64.to_le_bytes())
                .unwrap();
            store
                .set(b"/end/total_power", &1000i64.to_le_bytes())
                .unwrap();
            store
                .set(b"/end/proposer_address", b"\x05\x06\x07\x08")
                .unwrap();
        }

        // Execute begin-blocker
        let begin_result = host
            .execute_begin_blocker(
                1000,         // block_height
                1234567890,   // block_time
                "test-chain", // chain_id
                1_000_000,    // gas_limit
                vec![],       // No byzantine validators
            )
            .unwrap();

        assert!(begin_result.gas_used > 0);
        assert!(begin_result.data.is_some());

        if let Some(data) = begin_result.data {
            // Verify the response data structure
            assert!(data.is_object());
            if let Some(success) = data.get("success") {
                assert_eq!(success, &serde_json::Value::Bool(true));
            }
        }

        // Execute end-blocker
        let end_result = host
            .execute_end_blocker(
                1000,         // block_height
                1234567890,   // block_time
                "test-chain", // chain_id
                1_000_000,    // gas_limit
            )
            .unwrap();

        assert!(end_result.gas_used > 0);

        // Verify that components can only access their own data
        // The begin-blocker should not have been able to see end-blocker's data
        // and vice versa (this is enforced by the prefix isolation)

        // Check that data written by components is properly prefixed
        {
            let store = base_store.lock().unwrap();

            // If end-blocker updated last_reward_height, it should be in /end/
            if let Some(data) = store.get(b"/end/last_reward_height").unwrap() {
                let height = u64::from_le_bytes(data.try_into().unwrap());
                // Should be updated if rewards were distributed
                assert!(height == 100 || height == 1000);
            }
        }
    }

    #[test]
    #[ignore = "Requires kvstore interface which is being removed"]
    fn test_component_kvstore_persistence() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let host = ComponentHost::new(base_store.clone()).unwrap();

        // Load begin-blocker component
        let begin_blocker_path = std::env::current_dir()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("modules/begin_blocker_component.wasm");

        if !begin_blocker_path.exists() {
            eprintln!("Component not found. Run build-wasi-modules.sh first");
            return;
        }

        let begin_bytes = std::fs::read(&begin_blocker_path).unwrap();
        let begin_info = ComponentInfo {
            name: "begin-blocker".to_string(),
            path: begin_blocker_path,
            component_type: ComponentType::BeginBlocker,
            gas_limit: 1_000_000,
        };
        host.load_component("begin-blocker", &begin_bytes, begin_info)
            .unwrap();

        // First execution - component might write some data
        let result1 = host
            .execute_begin_blocker(
                1000,         // block_height
                1234567890,   // block_time
                "test-chain", // chain_id
                1_000_000,    // gas_limit
                vec![],
            )
            .unwrap();
        assert!(result1.gas_used > 0);

        // Second execution - component should see persisted data
        let result2 = host
            .execute_begin_blocker(
                1001,         // block_height
                1234567891,   // block_time
                "test-chain", // chain_id
                1_000_000,    // gas_limit
                vec![],
            )
            .unwrap();
        assert!(result2.gas_used > 0);

        // Both executions should succeed
        // In a real scenario, we'd check that the component could read
        // data it wrote in the first execution
    }

    #[test]
    #[ignore = "Requires kvstore interface which is being removed"]
    fn test_kvstore_prefix_enforcement() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let kvstore_host = KVStoreResourceHost::new(base_store.clone());

        // Register a custom component with a specific prefix
        kvstore_host
            .register_component_prefix("custom-component".to_string(), "/custom/".to_string())
            .unwrap();

        // Open a store for the custom component
        let mut table = wasmtime_wasi::ResourceTable::new();
        let store_handle = kvstore_host
            .open_store(&mut table, "custom-component")
            .unwrap();
        let store = kvstore_host.get_resource(&mut table, store_handle).unwrap();

        // Write data through the prefixed store
        store.set(b"key1", b"value1").unwrap();
        store.set(b"nested/key2", b"value2").unwrap();

        // Verify data is stored with correct prefix
        {
            let base = base_store.lock().unwrap();
            assert_eq!(base.get(b"/custom/key1").unwrap(), Some(b"value1".to_vec()));
            assert_eq!(
                base.get(b"/custom/nested/key2").unwrap(),
                Some(b"value2".to_vec())
            );
            // Should not exist without prefix
            assert_eq!(base.get(b"key1").unwrap(), None);
        }

        // Try to access data outside the prefix (should fail)
        assert_eq!(store.get(b"/other/key").unwrap(), None);
        assert_eq!(store.get(b"../../../etc/passwd").unwrap(), None);
    }

    #[test]
    #[ignore = "Requires kvstore interface which is being removed"]
    fn test_kvstore_edge_cases() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let kvstore_host = KVStoreResourceHost::new(base_store.clone());

        // Register a test component
        kvstore_host
            .register_component_prefix("test-component".to_string(), "/test/".to_string())
            .unwrap();

        let mut table = wasmtime_wasi::ResourceTable::new();
        let store_handle = kvstore_host
            .open_store(&mut table, "test-component")
            .unwrap();
        let store = kvstore_host.get_resource(&mut table, store_handle).unwrap();

        // Test empty value
        store.set(b"empty", b"").unwrap();
        assert_eq!(store.get(b"empty").unwrap(), Some(vec![]));

        // Test large key
        let large_key = vec![b'a'; 1024];
        store.set(&large_key, b"large_key_value").unwrap();
        assert_eq!(
            store.get(&large_key).unwrap(),
            Some(b"large_key_value".to_vec())
        );

        // Test large value
        let large_value = vec![b'x'; 10_000];
        store.set(b"large_value_key", &large_value).unwrap();
        assert_eq!(store.get(b"large_value_key").unwrap(), Some(large_value));

        // Test delete
        store.set(b"to_delete", b"value").unwrap();
        assert_eq!(store.get(b"to_delete").unwrap(), Some(b"value".to_vec()));
        store.delete(b"to_delete").unwrap();
        assert_eq!(store.get(b"to_delete").unwrap(), None);

        // Test has
        assert!(!store.has(b"nonexistent").unwrap());
        store.set(b"exists", b"yes").unwrap();
        assert!(store.has(b"exists").unwrap());

        // Test range query
        store.set(b"key1", b"value1").unwrap();
        store.set(b"key2", b"value2").unwrap();
        store.set(b"key3", b"value3").unwrap();

        let range_results = store.range(Some(b"key1"), Some(b"key3"), 10).unwrap();
        assert_eq!(range_results.len(), 2); // key1 and key2 (key3 is exclusive)
        assert_eq!(range_results[0].0, b"key1");
        assert_eq!(range_results[0].1, b"value1");
        assert_eq!(range_results[1].0, b"key2");
        assert_eq!(range_results[1].1, b"value2");

        // Test range with limit
        let limited_results = store.range(None, None, 2).unwrap();
        assert_eq!(limited_results.len(), 2);
    }

    #[test]
    #[ignore = "Requires kvstore interface which is being removed"]
    fn test_multiple_components_isolation() {
        let base_store = Arc::new(Mutex::new(MemStore::new()));
        let kvstore_host = KVStoreResourceHost::new(base_store.clone());

        // Register multiple components with different prefixes
        kvstore_host
            .register_component_prefix("component-a".to_string(), "/a/".to_string())
            .unwrap();

        kvstore_host
            .register_component_prefix("component-b".to_string(), "/b/".to_string())
            .unwrap();

        // Open stores for both components
        let mut table_a = wasmtime_wasi::ResourceTable::new();
        let store_a_handle = kvstore_host
            .open_store(&mut table_a, "component-a")
            .unwrap();
        let store_a = kvstore_host
            .get_resource(&mut table_a, store_a_handle)
            .unwrap();

        let mut table_b = wasmtime_wasi::ResourceTable::new();
        let store_b_handle = kvstore_host
            .open_store(&mut table_b, "component-b")
            .unwrap();
        let store_b = kvstore_host
            .get_resource(&mut table_b, store_b_handle)
            .unwrap();

        // Write data to both stores
        store_a.set(b"shared_key", b"value_from_a").unwrap();
        store_b.set(b"shared_key", b"value_from_b").unwrap();

        // Verify isolation - each component sees its own data
        assert_eq!(
            store_a.get(b"shared_key").unwrap(),
            Some(b"value_from_a".to_vec())
        );
        assert_eq!(
            store_b.get(b"shared_key").unwrap(),
            Some(b"value_from_b".to_vec())
        );

        // Verify data is stored with correct prefixes in underlying store
        {
            let base = base_store.lock().unwrap();
            assert_eq!(
                base.get(b"/a/shared_key").unwrap(),
                Some(b"value_from_a".to_vec())
            );
            assert_eq!(
                base.get(b"/b/shared_key").unwrap(),
                Some(b"value_from_b".to_vec())
            );
        }
    }
}
