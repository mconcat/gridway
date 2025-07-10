//! KVStore Component Bindings
//!
//! Generated bindings for the KVStore resource interface.
//!
//! NOTE: This is a placeholder implementation for KVStore resource bindings.
//! Full WIT-generated bindings will be implemented in a future version.

use crate::kvstore_resource::KVStoreResourceHost;
use wasmtime_wasi::ResourceTable;

// KVStore resource bindings for the host
pub struct KVStoreResourceBindings {
    pub host: KVStoreResourceHost,
}

impl KVStoreResourceBindings {
    pub fn new() -> Self {
        // Create a dummy MemStore for the legacy implementation
        // This will be replaced with proper implementation
        let dummy_store = std::sync::Arc::new(std::sync::Mutex::new(helium_store::MemStore::new()));
        Self {
            host: KVStoreResourceHost::new(dummy_store),
        }
    }

    /// Add KVStore resource functions to the component linker
    ///
    /// NOTE: This is a placeholder implementation. Full WIT-generated bindings
    /// will be implemented in a future version.
    pub fn add_to_linker<T>(_linker: &mut wasmtime::component::Linker<T>) -> wasmtime::Result<()>
    where
        T: AsRef<KVStoreResourceBindings> + AsMut<ResourceTable>,
    {
        // TODO: Implement proper WIT-generated bindings for KVStore resources
        // For now, this is a placeholder that doesn't add any functions

        /*
        // Add the open-store function
        linker.func_wrap(
            "helium:framework/kvstore",
            "open-store",
            |mut caller: wasmtime::Caller<'_, T>, name_ptr: i32, name_len: i32| -> wasmtime::Result<i32> {
                // Get the name from WASM memory
                let memory = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .ok_or_else(|| wasmtime::Error::msg("No memory export found"))?;

                let name_bytes = memory.data(&caller)[name_ptr as usize..(name_ptr + name_len) as usize].to_vec();
                let name = String::from_utf8(name_bytes)
                    .map_err(|e| wasmtime::Error::msg(format!("Invalid UTF-8: {}", e)))?;

                // Get the KVStore host and resource table
                let bindings = caller.data().as_ref();
                let table = caller.data_mut().as_mut();

                // Try to open the store
                match bindings.host.open_store(table, &name) {
                    Ok(resource) => {
                        // Return the resource handle as an i32
                        Ok(resource.rep() as i32)
                    }
                    Err(_) => {
                        // Return -1 for error
                        Ok(-1)
                    }
                }
            },
        )?;

        // Add store.get method
        linker.func_wrap(
            "helium:framework/kvstore",
            "[method]store.get",
            |mut caller: wasmtime::Caller<'_, T>, store_handle: i32, key_ptr: i32, key_len: i32| -> wasmtime::Result<i32> {
                // Get the key from WASM memory
                let memory = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .ok_or_else(|| wasmtime::Error::msg("No memory export found"))?;

                let key_bytes = memory.data(&caller)[key_ptr as usize..(key_ptr + key_len) as usize].to_vec();

                // Get the resource
                let bindings = caller.data().as_ref();
                let table = caller.data_mut().as_mut();
                let resource = Resource::<KVStoreResource>::new_own(store_handle as u32);

                match bindings.host.get_resource(table, resource) {
                    Ok(store_resource) => {
                        // Call get on the store
                        match store_resource.get(&key_bytes) {
                            Ok(Some(value)) => {
                                // Write value to memory and return pointer
                                // For now, return 1 for found, 0 for not found
                                Ok(1)
                            }
                            Ok(None) => Ok(0),
                            Err(_) => Ok(-1),
                        }
                    }
                    Err(_) => Ok(-1),
                }
            },
        )?;

        // Add store.set method
        linker.func_wrap(
            "helium:framework/kvstore",
            "[method]store.set",
            |mut caller: wasmtime::Caller<'_, T>, store_handle: i32, key_ptr: i32, key_len: i32, value_ptr: i32, value_len: i32| -> wasmtime::Result<i32> {
                // Get the key and value from WASM memory
                let memory = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .ok_or_else(|| wasmtime::Error::msg("No memory export found"))?;

                let key_bytes = memory.data(&caller)[key_ptr as usize..(key_ptr + key_len) as usize].to_vec();
                let value_bytes = memory.data(&caller)[value_ptr as usize..(value_ptr + value_len) as usize].to_vec();

                // Get the resource
                let bindings = caller.data().as_ref();
                let table = caller.data_mut().as_mut();
                let resource = Resource::<KVStoreResource>::new_own(store_handle as u32);

                match bindings.host.get_resource(table, resource) {
                    Ok(store_resource) => {
                        // Call set on the store
                        match store_resource.set(&key_bytes, &value_bytes) {
                            Ok(()) => Ok(0), // Success
                            Err(_) => Ok(-1), // Error
                        }
                    }
                    Err(_) => Ok(-1),
                }
            },
        )?;

        // Add store.delete method
        linker.func_wrap(
            "helium:framework/kvstore",
            "[method]store.delete",
            |mut caller: wasmtime::Caller<'_, T>, store_handle: i32, key_ptr: i32, key_len: i32| -> wasmtime::Result<i32> {
                // Get the key from WASM memory
                let memory = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .ok_or_else(|| wasmtime::Error::msg("No memory export found"))?;

                let key_bytes = memory.data(&caller)[key_ptr as usize..(key_ptr + key_len) as usize].to_vec();

                // Get the resource
                let bindings = caller.data().as_ref();
                let table = caller.data_mut().as_mut();
                let resource = Resource::<KVStoreResource>::new_own(store_handle as u32);

                match bindings.host.get_resource(table, resource) {
                    Ok(store_resource) => {
                        // Call delete on the store
                        match store_resource.delete(&key_bytes) {
                            Ok(()) => Ok(0), // Success
                            Err(_) => Ok(-1), // Error
                        }
                    }
                    Err(_) => Ok(-1),
                }
            },
        )?;

        // Add store.has method
        linker.func_wrap(
            "helium:framework/kvstore",
            "[method]store.has",
            |mut caller: wasmtime::Caller<'_, T>, store_handle: i32, key_ptr: i32, key_len: i32| -> wasmtime::Result<i32> {
                // Get the key from WASM memory
                let memory = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .ok_or_else(|| wasmtime::Error::msg("No memory export found"))?;

                let key_bytes = memory.data(&caller)[key_ptr as usize..(key_ptr + key_len) as usize].to_vec();

                // Get the resource
                let bindings = caller.data().as_ref();
                let table = caller.data_mut().as_mut();
                let resource = Resource::<KVStoreResource>::new_own(store_handle as u32);

                match bindings.host.get_resource(table, resource) {
                    Ok(store_resource) => {
                        // Call has on the store
                        match store_resource.has(&key_bytes) {
                            Ok(true) => Ok(1), // Found
                            Ok(false) => Ok(0), // Not found
                            Err(_) => Ok(-1), // Error
                        }
                    }
                    Err(_) => Ok(-1),
                }
            },
        )?;

        // Add store.range method (simplified)
        linker.func_wrap(
            "helium:framework/kvstore",
            "[method]store.range",
            |mut caller: wasmtime::Caller<'_, T>, store_handle: i32, start_ptr: i32, start_len: i32, end_ptr: i32, end_len: i32, limit: u32| -> wasmtime::Result<i32> {
                // Get start and end keys from WASM memory
                let memory = caller.get_export("memory")
                    .and_then(|e| e.into_memory())
                    .ok_or_else(|| wasmtime::Error::msg("No memory export found"))?;

                let start_key = if start_len > 0 {
                    Some(memory.data(&caller)[start_ptr as usize..(start_ptr + start_len) as usize].to_vec())
                } else {
                    None
                };

                let end_key = if end_len > 0 {
                    Some(memory.data(&caller)[end_ptr as usize..(end_ptr + end_len) as usize].to_vec())
                } else {
                    None
                };

                // Get the resource
                let bindings = caller.data().as_ref();
                let table = caller.data_mut().as_mut();
                let resource = Resource::<KVStoreResource>::new_own(store_handle as u32);

                match bindings.host.get_resource(table, resource) {
                    Ok(store_resource) => {
                        // Call range on the store
                        match store_resource.range(
                            start_key.as_deref(),
                            end_key.as_deref(),
                            limit,
                        ) {
                            Ok(results) => {
                                // Return the number of results found
                                Ok(results.len() as i32)
                            }
                            Err(_) => Ok(-1), // Error
                        }
                    }
                    Err(_) => Ok(-1),
                }
            },
        )?;

        */

        Ok(())
    }

    /// Mount a KVStore for component access with a prefix
    pub fn mount_store_with_prefix(&self, name: String, prefix: String) -> Result<(), String> {
        self.host.register_component_prefix(name, prefix)
    }
}

impl Default for KVStoreResourceBindings {
    fn default() -> Self {
        Self::new()
    }
}

impl AsRef<KVStoreResourceBindings> for KVStoreResourceBindings {
    fn as_ref(&self) -> &KVStoreResourceBindings {
        self
    }
}

// NOTE: Commented out until proper implementation is done
// impl AsMut<ResourceTable> for KVStoreResourceBindings {
//     fn as_mut(&mut self) -> &mut ResourceTable {
//         // This would need to be properly implemented with a combined state
//         // For now, this is a placeholder
//         todo!("Proper ResourceTable implementation needed")
//     }
// }
