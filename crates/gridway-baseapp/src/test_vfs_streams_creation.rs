//! Tests for VFS stream creation functionality

#[cfg(test)]
mod tests {
    use crate::vfs::{VirtualFilesystem, Capability};
    use crate::vfs_wasi_impl::{VfsFilesystem, FileHandle, DescriptorKind};
    use gridway_store::MemStore;
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;
    use wasmtime::component::Resource;
    use wasmtime_wasi::p2::bindings::filesystem::types as fs_types;
    use wasmtime_wasi::p2::bindings::filesystem::types::HostDescriptor;

    fn setup_test_vfs_with_file() -> (VfsFilesystem, u32) {
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        
        // Mount a test store
        let store = Arc::new(Mutex::new(MemStore::new()));
        vfs.lock().unwrap().mount_store("test".to_string(), store).unwrap();
        
        // Add a test capability
        vfs.lock().unwrap().add_capability(Capability::Write("/test".into())).unwrap();
        
        // Create a file descriptor in VFS
        let vfs_fd = vfs.lock().unwrap().open(&std::path::Path::new("/test/file.txt"), true)
            .unwrap_or(100); // Use 100 as fallback if open fails
        
        let mut vfs_fs = VfsFilesystem::new(vfs.clone());
        
        // Create a file handle
        let file_handle = FileHandle {
            vfs_fd: vfs_fd as u64,
            position: 0,
            vfs: vfs.clone(),
            writable: true,
        };
        
        // Add to descriptors map - simulate an open file
        let fd_id = 42u32;
        vfs_fs.descriptors.insert(fd_id, DescriptorKind::File { handle: file_handle });
        
        (vfs_fs, fd_id)
    }

    fn setup_test_vfs_with_readonly_file() -> (VfsFilesystem, u32) {
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        
        // Mount a test store
        let store = Arc::new(Mutex::new(MemStore::new()));
        vfs.lock().unwrap().mount_store("test".to_string(), store).unwrap();
        
        // Create a file descriptor in VFS
        let vfs_fd = vfs.lock().unwrap().open(&std::path::Path::new("/test/readonly.txt"), false)
            .unwrap_or(101); // Use 101 as fallback
        
        let mut vfs_fs = VfsFilesystem::new(vfs.clone());
        
        // Create a read-only file handle
        let file_handle = FileHandle {
            vfs_fd: vfs_fd as u64,
            position: 0,
            vfs: vfs.clone(),
            writable: false, // Read-only
        };
        
        let fd_id = 43u32;
        vfs_fs.descriptors.insert(fd_id, DescriptorKind::File { handle: file_handle });
        
        (vfs_fs, fd_id)
    }

    fn setup_test_vfs_with_directory() -> (VfsFilesystem, u32) {
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        
        // Mount a test store
        let store = Arc::new(Mutex::new(MemStore::new()));
        vfs.lock().unwrap().mount_store("test".to_string(), store).unwrap();
        
        let mut vfs_fs = VfsFilesystem::new(vfs.clone());
        
        // Create a directory descriptor
        let fd_id = 44u32;
        vfs_fs.descriptors.insert(fd_id, DescriptorKind::Dir { 
            path: "/test".into(),
            mount_id: 0
        });
        
        (vfs_fs, fd_id)
    }

    #[test]
    fn test_read_via_stream_creates_resource() {
        let (mut vfs_fs, fd_id) = setup_test_vfs_with_file();
        let fd = Resource::new_own(fd_id);
        
        // Create an input stream
        let result = vfs_fs.read_via_stream(fd, 0);
        
        // Should succeed and return a resource
        assert!(result.is_ok(), "Should create input stream resource");
        let _resource = result.unwrap();
        // Resource created successfully
    }

    #[test]
    fn test_read_via_stream_with_offset() {
        let (mut vfs_fs, fd_id) = setup_test_vfs_with_file();
        let fd = Resource::new_own(fd_id);
        
        // Create an input stream with offset
        let result = vfs_fs.read_via_stream(fd, 100);
        
        // Should succeed regardless of offset
        assert!(result.is_ok(), "Should create input stream with offset");
    }

    #[test]
    fn test_write_via_stream_creates_resource() {
        let (mut vfs_fs, fd_id) = setup_test_vfs_with_file();
        let fd = Resource::new_own(fd_id);
        
        // Create an output stream
        let result = vfs_fs.write_via_stream(fd, 0);
        
        // Should succeed and return a resource
        assert!(result.is_ok(), "Should create output stream resource");
    }

    #[test]
    fn test_append_via_stream_creates_resource() {
        let (mut vfs_fs, fd_id) = setup_test_vfs_with_file();
        let fd = Resource::new_own(fd_id);
        
        // Create an append stream
        let result = vfs_fs.append_via_stream(fd);
        
        // Should succeed and return a resource
        assert!(result.is_ok(), "Should create append stream resource");
    }

    #[test]
    fn test_write_stream_on_readonly_file_fails() {
        let (mut vfs_fs, fd_id) = setup_test_vfs_with_readonly_file();
        let fd = Resource::new_own(fd_id);
        
        // Try to create a write stream on read-only file
        let result = vfs_fs.write_via_stream(fd, 0);
        
        // Should fail with Access error
        assert!(result.is_err(), "Should fail to create write stream on read-only file");
        // Error generated - exact type may vary
    }

    #[test]
    fn test_append_stream_on_readonly_file_fails() {
        let (mut vfs_fs, fd_id) = setup_test_vfs_with_readonly_file();
        let fd = Resource::new_own(fd_id);
        
        // Try to create an append stream on read-only file
        let result = vfs_fs.append_via_stream(fd);
        
        // Should fail with Access error
        assert!(result.is_err(), "Should fail to create append stream on read-only file");
    }

    #[test]
    fn test_stream_on_directory_fails() {
        let (mut vfs_fs, fd_id) = setup_test_vfs_with_directory();
        
        // Try read_via_stream on directory
        let fd1 = Resource::new_own(fd_id);
        let result = vfs_fs.read_via_stream(fd1, 0);
        assert!(result.is_err(), "Should fail to create read stream on directory");
        
        // Try write_via_stream on directory
        let fd2 = Resource::new_own(fd_id);
        let result = vfs_fs.write_via_stream(fd2, 0);
        assert!(result.is_err(), "Should fail to create write stream on directory");
        
        // Try append_via_stream on directory
        let fd3 = Resource::new_own(fd_id);
        let result = vfs_fs.append_via_stream(fd3);
        assert!(result.is_err(), "Should fail to create append stream on directory");
    }

    #[test]
    fn test_stream_on_invalid_descriptor_fails() {
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let mut vfs_fs = VfsFilesystem::new(vfs);
        
        // Use descriptors that don't exist
        let invalid_fd1 = Resource::new_own(999);
        let invalid_fd2 = Resource::new_own(998);
        let invalid_fd3 = Resource::new_own(997);
        
        // All stream methods should fail
        let result = vfs_fs.read_via_stream(invalid_fd1, 0);
        assert!(result.is_err(), "Should fail with invalid descriptor");
        
        let result = vfs_fs.write_via_stream(invalid_fd2, 0);
        assert!(result.is_err(), "Should fail with invalid descriptor");
        
        let result = vfs_fs.append_via_stream(invalid_fd3);
        assert!(result.is_err(), "Should fail with invalid descriptor");
    }

    #[test]
    fn test_multiple_streams_can_be_created() {
        let (mut vfs_fs, fd_id) = setup_test_vfs_with_file();
        
        // Create multiple streams from the same file descriptor
        let fd1 = Resource::new_own(fd_id);
        let stream1 = vfs_fs.read_via_stream(fd1, 0);
        assert!(stream1.is_ok());
        
        let fd2 = Resource::new_own(fd_id);
        let stream2 = vfs_fs.read_via_stream(fd2, 100);
        assert!(stream2.is_ok());
        
        let fd3 = Resource::new_own(fd_id);
        let stream3 = vfs_fs.write_via_stream(fd3, 50);
        assert!(stream3.is_ok());
        
        // All streams created successfully
    }
}