//! Tests for ABI host function implementations
//!
//! These tests verify that the host functions properly connect to the VFS
//! and CapabilityManager for state access and permission checks.

#[cfg(test)]
mod tests {

    use crate::abi::*;
    use crate::capabilities::{CapabilityManager, CapabilityType};
    use crate::vfs::VirtualFilesystem;
    use helium_store::MemStore;
    use std::sync::{Arc, Mutex};

    /// Create a test WASM module with basic memory exports
    fn create_test_module() -> Vec<u8> {
        // Simple WASM module that exports memory
        wat::parse_str(
            r#"
            (module
                (memory (export "memory") 1)
                (func (export "test") (result i32)
                    i32.const 42
                )
            )
            "#,
        )
        .unwrap()
    }

    /// Set up test environment with VFS and CapabilityManager
    fn setup_test_context() -> (AbiContext, Arc<VirtualFilesystem>, Arc<CapabilityManager>) {
        let mut context = AbiContext::new("test_module".to_string(), vec![]);

        // Set up VFS
        let vfs = Arc::new(VirtualFilesystem::new());
        let test_store = Arc::new(Mutex::new(MemStore::new()));
        vfs.mount_store("test_module".to_string(), test_store)
            .unwrap();

        // Add VFS capabilities
        vfs.add_capability(crate::vfs::Capability::Read(
            "test_module".to_string().into(),
        ))
        .unwrap();
        vfs.add_capability(crate::vfs::Capability::Write(
            "test_module".to_string().into(),
        ))
        .unwrap();

        context.set_vfs(vfs.clone());

        // Set up CapabilityManager
        let cap_manager = Arc::new(CapabilityManager::new());

        // Grant capabilities to test module
        cap_manager
            .grant_capability(
                "test_module",
                CapabilityType::ReadState("test_module".to_string()),
                "system",
                true,
            )
            .unwrap();

        cap_manager
            .grant_capability(
                "test_module",
                CapabilityType::WriteState("test_module".to_string()),
                "system",
                true,
            )
            .unwrap();

        context.set_capability_manager(cap_manager.clone());

        (context, vfs, cap_manager)
    }

    #[test]
    #[ignore = "TODO: Fix VFS passing through WASM Store context"]
    fn test_host_state_get_set() {
        let (context, _vfs, _cap_manager) = setup_test_context();

        // Create WASM engine and store
        let engine = Engine::default();
        let mut store = Store::new(&engine, context);

        // Create module and instance
        let module_bytes = create_test_module();
        let module = Module::new(&engine, &module_bytes).unwrap();

        // Create linker and add host functions
        let mut linker = Linker::new(&engine);
        HostFunctions::add_to_linker(&mut linker).unwrap();

        // Instantiate module
        let instance = linker.instantiate(&mut store, &module).unwrap();

        // Get memory
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        // Test data
        let key = b"test_key";
        let value = b"test_value";

        // Write key and value to WASM memory
        memory.write(&mut store, 100, key).unwrap();
        memory.write(&mut store, 200, value).unwrap();

        // Allocate space for value length (4 bytes for u32)
        let len_bytes = (value.len() as u32).to_le_bytes();
        memory.write(&mut store, 300, &len_bytes).unwrap();

        // Get host functions through the linker
        let host_state_set = linker.get(&mut store, "env", "host_state_set").unwrap();
        let host_state_set_func = host_state_set.into_func().unwrap();

        // Call host_state_set
        let params = vec![
            Val::I32(100),                // key_ptr
            Val::I32(key.len() as i32),   // key_len
            Val::I32(200),                // value_ptr
            Val::I32(value.len() as i32), // value_len
        ];
        let mut results = vec![Val::I32(0)];
        host_state_set_func
            .call(&mut store, &params, &mut results)
            .unwrap();

        // Check result
        assert_eq!(results[0].unwrap_i32(), AbiResultCode::Success as i32);

        // Now test host_state_get
        let host_state_get = linker.get(&mut store, "env", "host_state_get").unwrap();
        let host_state_get_func = host_state_get.into_func().unwrap();

        // Allocate buffer for reading value
        let buffer_size = 1024;

        // Call host_state_get
        let params = vec![
            Val::I32(100),              // key_ptr
            Val::I32(key.len() as i32), // key_len
            Val::I32(400),              // value_ptr (where to write result)
            Val::I32(300),              // value_len_ptr (where to write length)
        ];
        let mut results = vec![Val::I32(0)];
        host_state_get_func
            .call(&mut store, &params, &mut results)
            .unwrap();

        // Check result
        assert_eq!(results[0].unwrap_i32(), AbiResultCode::Success as i32);

        // Read the length from memory
        let mut len_buffer = vec![0u8; 4];
        memory.read(&store, 300, &mut len_buffer).unwrap();
        let read_len =
            u32::from_le_bytes([len_buffer[0], len_buffer[1], len_buffer[2], len_buffer[3]])
                as usize;

        assert_eq!(read_len, value.len());

        // Read the value from memory
        let mut value_buffer = vec![0u8; read_len];
        memory.read(&store, 400, &mut value_buffer).unwrap();

        assert_eq!(&value_buffer, value);
    }

    #[test]
    #[ignore = "TODO: Fix capability manager passing through WASM Store context"]
    fn test_host_capability_check() {
        let (context, _vfs, cap_manager) = setup_test_context();

        // Verify context has capability manager before creating store
        assert!(
            context.capability_manager.is_some(),
            "Context should have capability manager"
        );

        // Grant additional capability
        cap_manager
            .grant_capability(
                "test_module",
                CapabilityType::SendMessage("other_module".to_string()),
                "system",
                true,
            )
            .unwrap();

        // Create WASM engine and store
        let engine = Engine::default();
        let mut store = Store::new(&engine, context);

        // Create module and instance
        let module_bytes = create_test_module();
        let module = Module::new(&engine, &module_bytes).unwrap();

        // Create linker and add host functions
        let mut linker = Linker::new(&engine);
        HostFunctions::add_to_linker(&mut linker).unwrap();

        // Instantiate module
        let instance = linker.instantiate(&mut store, &module).unwrap();

        // Get memory
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        // Test capability strings
        let valid_cap = b"send_msg:other_module";
        let invalid_cap = b"send_msg:forbidden_module";

        // Write capability strings to memory
        memory.write(&mut store, 100, valid_cap).unwrap();
        memory.write(&mut store, 200, invalid_cap).unwrap();

        // Get host function
        let host_capability_check = linker
            .get(&mut store, "env", "host_capability_check")
            .unwrap();
        let host_capability_check_func = host_capability_check.into_func().unwrap();

        // Check valid capability
        let params = vec![
            Val::I32(100),                    // cap_ptr
            Val::I32(valid_cap.len() as i32), // cap_len
        ];
        let mut results = vec![Val::I32(0)];
        host_capability_check_func
            .call(&mut store, &params, &mut results)
            .unwrap();

        assert_eq!(results[0].unwrap_i32(), AbiResultCode::Success as i32);

        // Check invalid capability
        let params = vec![
            Val::I32(200),                      // cap_ptr
            Val::I32(invalid_cap.len() as i32), // cap_len
        ];
        let mut results = vec![Val::I32(0)];
        host_capability_check_func
            .call(&mut store, &params, &mut results)
            .unwrap();

        assert_eq!(
            results[0].unwrap_i32(),
            AbiResultCode::PermissionDenied as i32
        );
    }

    #[test]
    #[ignore = "TODO: Fix capability manager passing through WASM Store context"]
    fn test_host_ipc_send() {
        let (context, _vfs, cap_manager) = setup_test_context();

        // Grant IPC send capability
        cap_manager
            .grant_capability(
                "test_module",
                CapabilityType::SendMessage("target_module".to_string()),
                "system",
                true,
            )
            .unwrap();

        // Create WASM engine and store
        let engine = Engine::default();
        let mut store = Store::new(&engine, context);

        // Create module and instance
        let module_bytes = create_test_module();
        let module = Module::new(&engine, &module_bytes).unwrap();

        // Create linker and add host functions
        let mut linker = Linker::new(&engine);
        HostFunctions::add_to_linker(&mut linker).unwrap();

        // Instantiate module
        let instance = linker.instantiate(&mut store, &module).unwrap();

        // Get memory
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        // Test data
        let target_module = b"target_module";
        let message = b"Hello from test_module";

        // Write data to memory
        memory.write(&mut store, 100, target_module).unwrap();
        memory.write(&mut store, 200, message).unwrap();

        // Get host function
        let host_ipc_send = linker.get(&mut store, "env", "host_ipc_send").unwrap();
        let host_ipc_send_func = host_ipc_send.into_func().unwrap();

        // Call host_ipc_send
        let params = vec![
            Val::I32(100),                        // module_ptr
            Val::I32(target_module.len() as i32), // module_len
            Val::I32(200),                        // msg_ptr
            Val::I32(message.len() as i32),       // msg_len
        ];
        let mut results = vec![Val::I32(0)];
        host_ipc_send_func
            .call(&mut store, &params, &mut results)
            .unwrap();

        assert_eq!(results[0].unwrap_i32(), AbiResultCode::Success as i32);
    }
}
