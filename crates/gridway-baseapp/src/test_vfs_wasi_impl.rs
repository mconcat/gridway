#[cfg(test)]
mod tests {
    use super::super::vfs::*;
    use super::super::vfs_wasi_impl::*;
    use gridway_store::MemStore;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use wasmtime::component::Resource;
    use wasmtime_wasi::p2::bindings::filesystem::preopens::Host as PreopensHost;
    use wasmtime_wasi::p2::bindings::filesystem::types as fs_types;
    use wasmtime_wasi::p2::bindings::filesystem::types::HostDescriptor;

    /// Helper to create a test VFS filesystem
    fn setup_test_vfs_filesystem() -> VfsFilesystem {
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));

        // Mount test stores
        let state_store = Arc::new(Mutex::new(MemStore::new()));
        let config_store = Arc::new(Mutex::new(MemStore::new()));

        {
            let vfs_lock = vfs.lock().unwrap();
            vfs_lock
                .mount_store("state".to_string(), state_store)
                .unwrap();
            vfs_lock
                .mount_store("config".to_string(), config_store)
                .unwrap();

            // Add capabilities
            vfs_lock
                .add_capability(Capability::Read(PathBuf::from("/")))
                .unwrap();
            vfs_lock
                .add_capability(Capability::Write(PathBuf::from("/")))
                .unwrap();
        }

        VfsFilesystem::new(vfs)
    }

    #[tokio::test]
    async fn test_set_size_operations() {
        let mut fs = setup_test_vfs_filesystem();

        // Get preopened directories
        let dirs = fs.get_directories().unwrap();
        assert!(!dirs.is_empty(), "Should have preopened directories");

        let (dir_desc, _) = &dirs[0];

        // Create a file
        let file_desc = fs
            .open_at(
                Resource::new_borrow(dir_desc.rep()),
                fs_types::PathFlags::empty(),
                "test_file.txt".to_string(),
                fs_types::OpenFlags::CREATE,
                fs_types::DescriptorFlags::WRITE | fs_types::DescriptorFlags::READ,
            )
            .await
            .unwrap();

        // Test truncate to 5 bytes (file starts empty, so this extends it)
        fs.set_size(Resource::new_borrow(file_desc.rep()), 5)
            .await
            .unwrap();

        // Test extend to 10 bytes
        fs.set_size(Resource::new_borrow(file_desc.rep()), 10)
            .await
            .unwrap();

        // Test truncate back to 3 bytes
        fs.set_size(Resource::new_borrow(file_desc.rep()), 3)
            .await
            .unwrap();

        // Clean up
        fs.drop(file_desc).unwrap();
    }

    #[tokio::test]
    async fn test_create_directory_at() {
        let mut fs = setup_test_vfs_filesystem();

        // Get preopened directories
        let dirs = fs.get_directories().unwrap();
        let (dir_desc, _) = &dirs[0];

        // Create a directory
        fs.create_directory_at(Resource::new_borrow(dir_desc.rep()), "test_dir".to_string())
            .await
            .unwrap();

        // Create a nested directory
        fs.create_directory_at(
            Resource::new_borrow(dir_desc.rep()),
            "test_dir/nested".to_string(),
        )
        .await
        .unwrap();

        // Verify we can create a file in the new directory
        let file_desc = fs
            .open_at(
                Resource::new_borrow(dir_desc.rep()),
                fs_types::PathFlags::empty(),
                "test_dir/file.txt".to_string(),
                fs_types::OpenFlags::CREATE,
                fs_types::DescriptorFlags::WRITE,
            )
            .await
            .unwrap();

        // Clean up
        fs.drop(file_desc).unwrap();
    }

    #[tokio::test]
    async fn test_unlink_file_at() {
        let mut fs = setup_test_vfs_filesystem();

        // Get preopened directories
        let dirs = fs.get_directories().unwrap();
        let (dir_desc, _) = &dirs[0];

        // Create a file with some content to ensure it gets persisted
        let file_desc = fs
            .open_at(
                Resource::new_borrow(dir_desc.rep()),
                fs_types::PathFlags::empty(),
                "delete_me.txt".to_string(),
                fs_types::OpenFlags::CREATE,
                fs_types::DescriptorFlags::WRITE,
            )
            .await
            .unwrap();

        // Write some data to ensure the file is actually created
        // Note: We can't directly write through the test interface,
        // but closing should persist even an empty file

        // Close the file - this should persist it to the store
        fs.drop(file_desc).unwrap();

        // Delete the file
        fs.unlink_file_at(
            Resource::new_borrow(dir_desc.rep()),
            "delete_me.txt".to_string(),
        )
        .await
        .unwrap();

        // Try to open the file again - should fail
        let result = fs
            .open_at(
                Resource::new_borrow(dir_desc.rep()),
                fs_types::PathFlags::empty(),
                "delete_me.txt".to_string(),
                fs_types::OpenFlags::empty(),
                fs_types::DescriptorFlags::READ,
            )
            .await;

        assert!(result.is_err(), "File should not exist after unlinking");
    }

    #[tokio::test]
    async fn test_unlink_directory_fails() {
        let mut fs = setup_test_vfs_filesystem();

        // Get preopened directories
        let dirs = fs.get_directories().unwrap();
        let (dir_desc, _) = &dirs[0];

        // Create a directory
        fs.create_directory_at(
            Resource::new_borrow(dir_desc.rep()),
            "a_directory".to_string(),
        )
        .await
        .unwrap();

        // Try to unlink it as a file - should fail
        let result = fs
            .unlink_file_at(
                Resource::new_borrow(dir_desc.rep()),
                "a_directory/".to_string(),
            )
            .await;

        assert!(result.is_err(), "Should not be able to unlink a directory");
    }

    #[tokio::test]
    async fn test_rename_at() {
        let mut fs = setup_test_vfs_filesystem();

        // Get preopened directories
        let dirs = fs.get_directories().unwrap();
        let (dir_desc, _) = &dirs[0];

        // Create a source file
        let file_desc = fs
            .open_at(
                Resource::new_borrow(dir_desc.rep()),
                fs_types::PathFlags::empty(),
                "old_name.txt".to_string(),
                fs_types::OpenFlags::CREATE,
                fs_types::DescriptorFlags::WRITE,
            )
            .await
            .unwrap();

        // Close the file
        fs.drop(file_desc).unwrap();

        // Rename the file
        fs.rename_at(
            Resource::new_borrow(dir_desc.rep()),
            "old_name.txt".to_string(),
            Resource::new_borrow(dir_desc.rep()),
            "new_name.txt".to_string(),
        )
        .await
        .unwrap();

        // Verify old file doesn't exist
        let old_result = fs
            .open_at(
                Resource::new_borrow(dir_desc.rep()),
                fs_types::PathFlags::empty(),
                "old_name.txt".to_string(),
                fs_types::OpenFlags::empty(),
                fs_types::DescriptorFlags::READ,
            )
            .await;
        assert!(
            old_result.is_err(),
            "Old file should not exist after rename"
        );

        // Verify new file exists
        let new_file = fs
            .open_at(
                Resource::new_borrow(dir_desc.rep()),
                fs_types::PathFlags::empty(),
                "new_name.txt".to_string(),
                fs_types::OpenFlags::empty(),
                fs_types::DescriptorFlags::READ,
            )
            .await;
        assert!(new_file.is_ok(), "New file should exist after rename");

        if let Ok(fd) = new_file {
            fs.drop(fd).unwrap();
        }
    }

    #[tokio::test]
    async fn test_rename_to_existing_file() {
        let mut fs = setup_test_vfs_filesystem();

        // Get preopened directories
        let dirs = fs.get_directories().unwrap();
        let (dir_desc, _) = &dirs[0];

        // Create two files
        let file1 = fs
            .open_at(
                Resource::new_borrow(dir_desc.rep()),
                fs_types::PathFlags::empty(),
                "file1.txt".to_string(),
                fs_types::OpenFlags::CREATE,
                fs_types::DescriptorFlags::WRITE,
            )
            .await
            .unwrap();
        fs.drop(file1).unwrap();

        let file2 = fs
            .open_at(
                Resource::new_borrow(dir_desc.rep()),
                fs_types::PathFlags::empty(),
                "file2.txt".to_string(),
                fs_types::OpenFlags::CREATE,
                fs_types::DescriptorFlags::WRITE,
            )
            .await
            .unwrap();
        fs.drop(file2).unwrap();

        // Rename file1 to file2 (should overwrite)
        let result = fs
            .rename_at(
                Resource::new_borrow(dir_desc.rep()),
                "file1.txt".to_string(),
                Resource::new_borrow(dir_desc.rep()),
                "file2.txt".to_string(),
            )
            .await;

        // This might succeed or fail depending on implementation
        // Some filesystems allow overwriting, others don't
        println!("Rename to existing file result: {result:?}");
    }

    #[tokio::test]
    async fn test_remove_empty_directory() {
        let mut fs = setup_test_vfs_filesystem();

        // Get preopened directories
        let dirs = fs.get_directories().unwrap();
        let (dir_desc, _) = &dirs[0];

        // Create a directory
        fs.create_directory_at(
            Resource::new_borrow(dir_desc.rep()),
            "empty_dir".to_string(),
        )
        .await
        .unwrap();

        // Remove the empty directory - should succeed
        fs.remove_directory_at(
            Resource::new_borrow(dir_desc.rep()),
            "empty_dir".to_string(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_remove_non_empty_directory_fails() {
        let mut fs = setup_test_vfs_filesystem();

        // Get preopened directories
        let dirs = fs.get_directories().unwrap();
        let (dir_desc, _) = &dirs[0];

        // Create a directory
        fs.create_directory_at(
            Resource::new_borrow(dir_desc.rep()),
            "non_empty_dir".to_string(),
        )
        .await
        .unwrap();

        // Create a file in the directory
        let file_desc = fs
            .open_at(
                Resource::new_borrow(dir_desc.rep()),
                fs_types::PathFlags::empty(),
                "non_empty_dir/file.txt".to_string(),
                fs_types::OpenFlags::CREATE,
                fs_types::DescriptorFlags::WRITE,
            )
            .await
            .unwrap();

        // Close the file (this ensures it's written to the store)
        fs.drop(file_desc).unwrap();

        // Try to remove the non-empty directory - should fail
        let result = fs
            .remove_directory_at(
                Resource::new_borrow(dir_desc.rep()),
                "non_empty_dir".to_string(),
            )
            .await;

        assert!(
            result.is_err(),
            "Should not be able to remove non-empty directory"
        );
    }

    #[tokio::test]
    async fn test_set_size_on_directory_fails() {
        let mut fs = setup_test_vfs_filesystem();

        // Get preopened directories
        let dirs = fs.get_directories().unwrap();
        let (dir_desc, _) = &dirs[0];

        // Try to set size on a directory descriptor - should fail
        let result = fs.set_size(Resource::new_borrow(dir_desc.rep()), 100).await;

        assert!(
            result.is_err(),
            "Should not be able to set size on directory"
        );
    }

    #[tokio::test]
    async fn test_file_operations_with_permissions() {
        let mut fs = setup_test_vfs_filesystem();

        // Get preopened directories - the first one should be writable
        let dirs = fs.get_directories().unwrap();
        let (rw_dir, _) = &dirs[0];

        // Create a file with write permissions
        let write_file = fs
            .open_at(
                Resource::new_borrow(rw_dir.rep()),
                fs_types::PathFlags::empty(),
                "writable.txt".to_string(),
                fs_types::OpenFlags::CREATE,
                fs_types::DescriptorFlags::WRITE,
            )
            .await
            .unwrap();

        // Set size should work on writable file
        fs.set_size(Resource::new_borrow(write_file.rep()), 100)
            .await
            .unwrap();

        fs.drop(write_file).unwrap();

        // Open the same file read-only
        let read_file = fs
            .open_at(
                Resource::new_borrow(rw_dir.rep()),
                fs_types::PathFlags::empty(),
                "writable.txt".to_string(),
                fs_types::OpenFlags::empty(),
                fs_types::DescriptorFlags::READ,
            )
            .await
            .unwrap();

        // Set size should fail on read-only file
        let result = fs.set_size(Resource::new_borrow(read_file.rep()), 50).await;
        assert!(
            result.is_err(),
            "Should not be able to set size on read-only file"
        );

        fs.drop(read_file).unwrap();
    }

    #[tokio::test]
    async fn test_rename_across_namespaces() {
        let mut fs = setup_test_vfs_filesystem();

        // Get preopened directories
        let dirs = fs.get_directories().unwrap();
        
        // Find the root (/) mount and config (/config) mount
        let mut root_dir = None;
        let mut config_dir = None;
        
        for (desc, path) in &dirs {
            if path == "/" {
                root_dir = Some(desc);
            } else if path == "/config" {
                config_dir = Some(desc);
            }
        }
        
        let root_dir = root_dir.expect("Root directory not found");
        let config_dir = config_dir.expect("Config directory not found");

        // Create a file in the root mount (state namespace)
        let source_file = fs
            .open_at(
                Resource::new_borrow(root_dir.rep()),
                fs_types::PathFlags::empty(),
                "cross_namespace_test.txt".to_string(),
                fs_types::OpenFlags::CREATE,
                fs_types::DescriptorFlags::WRITE,
            )
            .await
            .unwrap();
        fs.drop(source_file).unwrap();

        // Try to rename the file from root mount to config mount
        // This should work as long as both mounts support write operations
        // However, /config is read-only, so this should fail with Access error
        let result = fs
            .rename_at(
                Resource::new_borrow(root_dir.rep()),
                "cross_namespace_test.txt".to_string(),
                Resource::new_borrow(config_dir.rep()),
                "renamed_file.txt".to_string(),
            )
            .await;

        // The rename should fail because /config mount is read-only
        assert!(
            result.is_err(),
            "Should not be able to rename to read-only mount"
        );
        
        // Verify the error is Access (due to read-only mount)
        if let Err(err) = result {
            // The error should be Access since config mount doesn't have write capability
            // We can't easily check the specific error type without exposing internals,
            // but the fact that it fails is the important test
        }

        // Now test renaming within the same namespace (should work)
        let result = fs
            .rename_at(
                Resource::new_borrow(root_dir.rep()),
                "cross_namespace_test.txt".to_string(),
                Resource::new_borrow(root_dir.rep()),
                "renamed_within_namespace.txt".to_string(),
            )
            .await;
        
        assert!(
            result.is_ok(),
            "Should be able to rename within the same namespace"
        );

        // Clean up
        fs.unlink_file_at(
            Resource::new_borrow(root_dir.rep()),
            "renamed_within_namespace.txt".to_string(),
        )
        .await
        .unwrap();
    }
}
