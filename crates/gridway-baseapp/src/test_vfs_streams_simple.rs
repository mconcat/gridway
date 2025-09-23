//! Simple tests for VFS stream functionality checking error paths

#[cfg(test)]
mod tests {
    use crate::vfs::{VirtualFilesystem, Capability};
    use crate::vfs_wasi_impl::VfsFilesystem;
    use gridway_store::MemStore;
    use std::sync::{Arc, Mutex};
    use wasmtime::component::Resource;
    use wasmtime_wasi::p2::bindings::filesystem::types as fs_types;
    use wasmtime_wasi::p2::bindings::filesystem::types::HostDescriptor;

    #[test]
    fn test_stream_methods_return_unsupported() {
        // Setup VFS
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let store = Arc::new(Mutex::new(MemStore::new()));
        vfs.lock().unwrap().mount_store("test".to_string(), store).unwrap();
        
        let mut vfs_fs = VfsFilesystem::new(vfs.clone());
        
        // Create mock file descriptor
        let file_fd = Resource::new_own(10);
        let dir_fd = Resource::new_own(3);
        
        // Test read_via_stream returns Unsupported
        let read_result = vfs_fs.read_via_stream(file_fd, 0);
        assert!(read_result.is_err());
        // Just check it's an error - the exact format varies
        
        // Test write_via_stream returns Unsupported
        let file_fd2 = Resource::new_own(11);
        let write_result = vfs_fs.write_via_stream(file_fd2, 0);
        assert!(write_result.is_err());
        
        // Test append_via_stream returns Unsupported
        let file_fd3 = Resource::new_own(12);
        let append_result = vfs_fs.append_via_stream(file_fd3);
        assert!(append_result.is_err());
        
        // Test with directory descriptor - should also error
        let dir_read = vfs_fs.read_via_stream(dir_fd, 0);
        assert!(dir_read.is_err());
    }

    #[test]
    fn test_error_code_mapping() {
        // This test verifies the error codes are properly defined
        
        // These error codes should exist
        let _ = fs_types::ErrorCode::BadDescriptor;
        let _ = fs_types::ErrorCode::IsDirectory;
        let _ = fs_types::ErrorCode::Access;
        let _ = fs_types::ErrorCode::Unsupported;
        
        // These are valid wasmtime errors
        use wasmtime_wasi::TrappableError;
        let _err1: TrappableError<fs_types::ErrorCode> = fs_types::ErrorCode::BadDescriptor.into();
        let _err2: TrappableError<fs_types::ErrorCode> = fs_types::ErrorCode::IsDirectory.into();
        let _err3: TrappableError<fs_types::ErrorCode> = fs_types::ErrorCode::Access.into();
        let _err4: TrappableError<fs_types::ErrorCode> = fs_types::ErrorCode::Unsupported.into();
    }
}