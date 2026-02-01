//! Tests for ABI host function implementations
//!
//! These tests verify that the host functions properly connect to the VFS
//! and CapabilityManager for state access and permission checks.

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {

    use crate::abi::*;
    use crate::capabilities::{CapabilityManager, CapabilityType};
    use crate::vfs::VirtualFilesystem;
    use gridway_store::MemStore;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// Create a test WASM module that imports host functions and exports wrappers.
    /// Host functions must be called FROM WASM (not directly from Rust) so that
    /// `caller.get_export("memory")` can find the instance's memory.
    fn create_test_module() -> Vec<u8> {
        wat::parse_str(
            r#"
            (module
                ;; Import host functions
                (import "env" "host_state_get" (func $host_state_get (param i32 i32 i32 i32) (result i32)))
                (import "env" "host_state_set" (func $host_state_set (param i32 i32 i32 i32) (result i32)))
                (import "env" "host_capability_check" (func $host_capability_check (param i32 i32) (result i32)))
                (import "env" "host_ipc_send" (func $host_ipc_send (param i32 i32 i32 i32) (result i32)))

                (memory (export "memory") 1)

                ;; Passthrough wrappers — call host functions from within WASM context
                (func (export "call_state_set") (param i32 i32 i32 i32) (result i32)
                    local.get 0 local.get 1 local.get 2 local.get 3
                    call $host_state_set)

                (func (export "call_state_get") (param i32 i32 i32 i32) (result i32)
                    local.get 0 local.get 1 local.get 2 local.get 3
                    call $host_state_get)

                (func (export "call_capability_check") (param i32 i32) (result i32)
                    local.get 0 local.get 1
                    call $host_capability_check)

                (func (export "call_ipc_send") (param i32 i32 i32 i32) (result i32)
                    local.get 0 local.get 1 local.get 2 local.get 3
                    call $host_ipc_send)

                (func (export "test") (result i32)
                    i32.const 42)
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
            PathBuf::from("/test_module"),
        ))
        .unwrap();
        vfs.add_capability(crate::vfs::Capability::Write(
            PathBuf::from("/test_module"),
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

        // Call host functions through WASM wrappers (not directly) so that
        // caller.get_export("memory") works inside the host function.
        let call_state_set = instance
            .get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "call_state_set")
            .unwrap();

        // Call host_state_set via WASM wrapper
        let result = call_state_set
            .call(
                &mut store,
                (100, key.len() as i32, 200, value.len() as i32),
            )
            .unwrap();
        // Check result
        assert_eq!(result, AbiResultCode::Success as i32);

        // Now test host_state_get via WASM wrapper
        let call_state_get = instance
            .get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "call_state_get")
            .unwrap();

        let get_result = call_state_get
            .call(&mut store, (100, key.len() as i32, 400, 300))
            .unwrap();

        // Check result
        assert_eq!(get_result, AbiResultCode::Success as i32);

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

        // Call through WASM wrapper
        let call_capability_check = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "call_capability_check")
            .unwrap();

        // Check valid capability
        let result = call_capability_check
            .call(&mut store, (100, valid_cap.len() as i32))
            .unwrap();
        assert_eq!(result, AbiResultCode::Success as i32);

        // Check invalid capability
        let result = call_capability_check
            .call(&mut store, (200, invalid_cap.len() as i32))
            .unwrap();
        assert_eq!(result, AbiResultCode::PermissionDenied as i32);
    }

    #[test]
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

        // Call through WASM wrapper
        let call_ipc_send = instance
            .get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "call_ipc_send")
            .unwrap();

        let result = call_ipc_send
            .call(
                &mut store,
                (100, target_module.len() as i32, 200, message.len() as i32),
            )
            .unwrap();
        assert_eq!(result, AbiResultCode::Success as i32);
    }
}
