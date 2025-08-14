#[cfg(test)]
mod tests {
    use super::super::vfs::*;
    use super::super::vfs_wasi_impl::*;
    use gridway_store::{KVStore, MemStore};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use wasmtime::component::Resource;
    use wasmtime_wasi::p2::bindings::filesystem::preopens::Host as PreopensHost;
    use wasmtime_wasi::p2::bindings::filesystem::types as fs_types;
    use wasmtime_wasi::p2::bindings::filesystem::types::HostDescriptor;

    #[tokio::test]
    async fn test_debug_file_operations() {
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));

        // Mount test store
        let state_store = Arc::new(Mutex::new(MemStore::new()));

        {
            let vfs_lock = vfs.lock().unwrap();
            vfs_lock
                .mount_store("state".to_string(), state_store.clone())
                .unwrap();
            vfs_lock
                .add_capability(Capability::Read(PathBuf::from("/")))
                .unwrap();
            vfs_lock
                .add_capability(Capability::Write(PathBuf::from("/")))
                .unwrap();
        }

        let mut fs = VfsFilesystem::new(vfs.clone());

        // Get preopened directories
        let dirs = fs.get_directories().unwrap();
        let (dir_desc, mount_path) = &dirs[0];
        println!("Mount path: {mount_path}");

        // Create a file
        let file_desc = fs
            .open_at(
                Resource::new_borrow(dir_desc.rep()),
                fs_types::PathFlags::empty(),
                "test_file.txt".to_string(),
                fs_types::OpenFlags::CREATE,
                fs_types::DescriptorFlags::WRITE,
            )
            .await
            .unwrap();

        println!("Created file descriptor");

        // Close the file
        fs.drop(file_desc).unwrap();
        println!("Closed file");

        // Check what's in the store
        {
            let store = state_store.lock().unwrap();
            let iter = store.prefix_iterator(&[]);
            println!("Keys in store after create:");
            for (key, value) in iter {
                println!(
                    "  Key: {:?}, Value len: {}",
                    String::from_utf8_lossy(&key),
                    value.len()
                );
            }
        }

        // Try to unlink the file
        let result = fs
            .unlink_file_at(
                Resource::new_borrow(dir_desc.rep()),
                "test_file.txt".to_string(),
            )
            .await;

        println!("Unlink result: {result:?}");

        // Check what's in the store after unlink
        {
            let store = state_store.lock().unwrap();
            let iter = store.prefix_iterator(&[]);
            println!("Keys in store after unlink:");
            for (key, value) in iter {
                println!(
                    "  Key: {:?}, Value len: {}",
                    String::from_utf8_lossy(&key),
                    value.len()
                );
            }
        }

        // Try to open the file again
        let open_result = fs
            .open_at(
                Resource::new_borrow(dir_desc.rep()),
                fs_types::PathFlags::empty(),
                "test_file.txt".to_string(),
                fs_types::OpenFlags::empty(),
                fs_types::DescriptorFlags::READ,
            )
            .await;

        println!("Open after unlink result: {:?}", open_result.is_err());
    }
}
