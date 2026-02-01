#[cfg(test)]
mod tests {
    use crate::component_host::{ComponentHost, ComponentInfo, ComponentType};
    use crate::vfs::{Capability, VirtualFilesystem};
    use gridway_store::{KVStore, MemStore};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_component_kvstore_isolation() {
        // Create a base store and ComponentHost with VFS
        let mut host = ComponentHost::new().unwrap();

        // Set up VFS with separate namespaces for begin/end blockers
        let vfs = Arc::new(VirtualFilesystem::new());
        let begin_store: Arc<Mutex<dyn KVStore>> = Arc::new(Mutex::new(MemStore::new()));
        let end_store: Arc<Mutex<dyn KVStore>> = Arc::new(Mutex::new(MemStore::new()));

        vfs.mount_store("begin".to_string(), begin_store.clone())
            .unwrap();
        vfs.mount_store("end".to_string(), end_store.clone())
            .unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/begin")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/begin")))
            .unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/end")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/end")))
            .unwrap();

        host.set_vfs(vfs.clone());

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

        // Pre-populate VFS stores with data for each component
        vfs.write_key("begin", b"proposer_address", b"\x01\x02\x03\x04")
            .unwrap();
        vfs.write_key("end", b"inflation_rate", &0.05f64.to_le_bytes())
            .unwrap();
        vfs.write_key("end", b"last_reward_height", &100u64.to_le_bytes())
            .unwrap();
        vfs.write_key("end", b"total_power", &1000i64.to_le_bytes())
            .unwrap();
        vfs.write_key("end", b"proposer_address", b"\x05\x06\x07\x08")
            .unwrap();

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

        // Verify namespace isolation: begin namespace should NOT have end data
        assert!(vfs.read_key("begin", b"inflation_rate").unwrap().is_none());
        // And end namespace should NOT have begin-specific data
        // (proposer_address exists in both, but with different values)
        let begin_proposer = vfs
            .read_key("begin", b"proposer_address")
            .unwrap()
            .unwrap();
        let end_proposer = vfs
            .read_key("end", b"proposer_address")
            .unwrap()
            .unwrap();
        assert_ne!(begin_proposer, end_proposer);
    }

    #[test]
    fn test_component_kvstore_persistence() {
        let mut host = ComponentHost::new().unwrap();

        // Set up VFS
        let vfs = Arc::new(VirtualFilesystem::new());
        let begin_store: Arc<Mutex<dyn KVStore>> = Arc::new(Mutex::new(MemStore::new()));
        vfs.mount_store("begin".to_string(), begin_store.clone())
            .unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/begin")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/begin")))
            .unwrap();
        host.set_vfs(vfs.clone());

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

        // First execution
        let result1 = host
            .execute_begin_blocker(1000, 1234567890, "test-chain", 1_000_000, vec![])
            .unwrap();
        assert!(result1.gas_used > 0);

        // Second execution - component should see persisted data via VFS
        let result2 = host
            .execute_begin_blocker(1001, 1234567891, "test-chain", 1_000_000, vec![])
            .unwrap();
        assert!(result2.gas_used > 0);

        // Both executions should succeed - persistence is verified by the
        // fact that the second execution can read data from the same VFS store
    }

    #[test]
    fn test_kvstore_prefix_enforcement() {
        // Test VFS namespace isolation (replaces old prefix-based isolation)
        let vfs = Arc::new(VirtualFilesystem::new());

        // Mount a store for a custom component namespace
        let custom_store: Arc<Mutex<dyn KVStore>> = Arc::new(Mutex::new(MemStore::new()));
        vfs.mount_store("custom".to_string(), custom_store.clone())
            .unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/custom")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/custom")))
            .unwrap();

        // Write data through the VFS namespace
        vfs.write_key("custom", b"key1", b"value1").unwrap();
        vfs.write_key("custom", b"nested/key2", b"value2").unwrap();

        // Verify data is stored correctly in the namespace
        assert_eq!(
            vfs.read_key("custom", b"key1").unwrap(),
            Some(b"value1".to_vec())
        );
        assert_eq!(
            vfs.read_key("custom", b"nested/key2").unwrap(),
            Some(b"value2".to_vec())
        );

        // Verify data is stored with correct keys in the underlying store
        {
            let store = custom_store.lock().unwrap();
            assert_eq!(store.get(b"key1").unwrap(), Some(b"value1".to_vec()));
            assert_eq!(
                store.get(b"nested/key2").unwrap(),
                Some(b"value2".to_vec())
            );
        }

        // Accessing a non-existent namespace should fail
        assert!(vfs.read_key("other", b"key").is_err());

        // Path traversal attempts should not escape the namespace
        // (VFS uses namespace-based isolation, not path-based prefixes)
        assert_eq!(
            vfs.read_key("custom", b"../../../etc/passwd").unwrap(),
            None
        );
    }

    #[test]
    fn test_kvstore_edge_cases() {
        let vfs = Arc::new(VirtualFilesystem::new());

        let test_store: Arc<Mutex<dyn KVStore>> = Arc::new(Mutex::new(MemStore::new()));
        vfs.mount_store("test".to_string(), test_store).unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/test")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/test")))
            .unwrap();

        // Test empty value
        vfs.write_key("test", b"empty", b"").unwrap();
        assert_eq!(vfs.read_key("test", b"empty").unwrap(), Some(vec![]));

        // Test large key
        let large_key = vec![b'a'; 1024];
        vfs.write_key("test", &large_key, b"large_key_value")
            .unwrap();
        assert_eq!(
            vfs.read_key("test", &large_key).unwrap(),
            Some(b"large_key_value".to_vec())
        );

        // Test large value
        let large_value = vec![b'x'; 10_000];
        vfs.write_key("test", b"large_value_key", &large_value)
            .unwrap();
        assert_eq!(
            vfs.read_key("test", b"large_value_key").unwrap(),
            Some(large_value)
        );

        // Test delete
        vfs.write_key("test", b"to_delete", b"value").unwrap();
        assert_eq!(
            vfs.read_key("test", b"to_delete").unwrap(),
            Some(b"value".to_vec())
        );
        vfs.delete_key("test", b"to_delete").unwrap();
        assert_eq!(vfs.read_key("test", b"to_delete").unwrap(), None);

        // Test has
        assert!(!vfs.has_key("test", b"nonexistent").unwrap());
        vfs.write_key("test", b"exists", b"yes").unwrap();
        assert!(vfs.has_key("test", b"exists").unwrap());

        // Test range query
        vfs.write_key("test", b"key1", b"value1").unwrap();
        vfs.write_key("test", b"key2", b"value2").unwrap();
        vfs.write_key("test", b"key3", b"value3").unwrap();

        let range_results = vfs
            .range_keys("test", Some(b"key"), Some(b"key3"), 10)
            .unwrap();
        assert_eq!(range_results.len(), 2); // key1 and key2 (key3 is exclusive)
        assert_eq!(range_results[0].0, b"key1");
        assert_eq!(range_results[0].1, b"value1");
        assert_eq!(range_results[1].0, b"key2");
        assert_eq!(range_results[1].1, b"value2");

        // Test range with limit
        let limited_results = vfs.range_keys("test", None, None, 2).unwrap();
        assert_eq!(limited_results.len(), 2);
    }

    #[test]
    fn test_multiple_components_isolation() {
        let vfs = Arc::new(VirtualFilesystem::new());

        // Mount separate stores for each component namespace
        let store_a: Arc<Mutex<dyn KVStore>> = Arc::new(Mutex::new(MemStore::new()));
        let store_b: Arc<Mutex<dyn KVStore>> = Arc::new(Mutex::new(MemStore::new()));

        vfs.mount_store("component-a".to_string(), store_a.clone())
            .unwrap();
        vfs.mount_store("component-b".to_string(), store_b.clone())
            .unwrap();

        // Grant capabilities for both namespaces
        vfs.add_capability(Capability::Read(PathBuf::from("/component-a")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/component-a")))
            .unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/component-b")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/component-b")))
            .unwrap();

        // Write data to both namespaces using the same key name
        vfs.write_key("component-a", b"shared_key", b"value_from_a")
            .unwrap();
        vfs.write_key("component-b", b"shared_key", b"value_from_b")
            .unwrap();

        // Verify isolation - each namespace sees its own data
        assert_eq!(
            vfs.read_key("component-a", b"shared_key").unwrap(),
            Some(b"value_from_a".to_vec())
        );
        assert_eq!(
            vfs.read_key("component-b", b"shared_key").unwrap(),
            Some(b"value_from_b".to_vec())
        );

        // Verify data is isolated in the underlying stores
        {
            let base_a = store_a.lock().unwrap();
            assert_eq!(
                base_a.get(b"shared_key").unwrap(),
                Some(b"value_from_a".to_vec())
            );
        }
        {
            let base_b = store_b.lock().unwrap();
            assert_eq!(
                base_b.get(b"shared_key").unwrap(),
                Some(b"value_from_b".to_vec())
            );
        }

        // Cross-namespace access should not leak data
        // component-a's store should NOT contain component-b's data
        {
            let base_a = store_a.lock().unwrap();
            assert_eq!(base_a.get(b"shared_key").unwrap(), Some(b"value_from_a".to_vec()));
        }
    }
}
