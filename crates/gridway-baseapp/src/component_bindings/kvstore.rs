//! KVStore Component Bindings
//!
//! NOTE: Legacy placeholder. The actual kvstore interface is now implemented via
//! the VFS↔WASI bridge in component_host.rs (kvstore::Host/HostStore for ComponentState).
//! This module is retained for structural compatibility but does not provide functionality.

/// Placeholder for legacy KVStore resource bindings.
/// All kvstore access now goes through ComponentState's VFS-backed implementation.
pub struct KVStoreResourceBindings;

impl KVStoreResourceBindings {
    pub fn new() -> Self {
        Self
    }

    /// No-op linker registration. Kvstore is registered via component_host.rs.
    pub fn add_to_linker<T>(_linker: &mut wasmtime::component::Linker<T>) -> wasmtime::Result<()> {
        Ok(())
    }
}

impl Default for KVStoreResourceBindings {
    fn default() -> Self {
        Self::new()
    }
}
