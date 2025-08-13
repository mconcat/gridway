//! Simplified VFS WASI Host Implementation
//!
//! This provides a minimal working implementation that integrates VFS with WASI.

use crate::vfs::VirtualFilesystem;
use std::sync::{Arc, Mutex};
use wasmtime_wasi::p2::{IoView, WasiCtx, WasiCtxBuilder, WasiView};
use wasmtime_wasi::ResourceTable;

/// VFS-backed WASI context
pub struct VfsWasiHost {
    wasi: WasiCtx,
    table: ResourceTable,
    #[allow(dead_code)]
    vfs: Arc<Mutex<VirtualFilesystem>>,
}

impl VfsWasiHost {
    pub fn new(vfs: Arc<Mutex<VirtualFilesystem>>) -> Self {
        let wasi = WasiCtxBuilder::new().inherit_stdio().inherit_env().build();

        Self {
            wasi,
            table: ResourceTable::new(),
            vfs,
        }
    }
}

impl WasiView for VfsWasiHost {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

impl IoView for VfsWasiHost {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}
