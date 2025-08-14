//! VFS WASI Implementation
//!
//! This module implements WASI filesystem interfaces directly, routing all filesystem
//! operations to our Merkle-backed VFS. The key insight is to NOT use wasmtime-wasi's
//! filesystem implementation at all, but provide our own implementation of the WASI
//! filesystem interfaces.

use crate::vfs::{Capability, VfsError, VirtualFilesystem};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use wasmtime::component::Resource;
use wasmtime_wasi::p2::bindings::filesystem::{preopens, types as fs_types};
use wasmtime_wasi::p2::bindings::io::poll::{self as io_poll};
use wasmtime_wasi::p2::bindings::io::streams::{self as io_streams};
use wasmtime_wasi::p2::{IoView, WasiView};
use wasmtime_wasi::ResourceTable;
// Stream support deferred; leave imports commented for future work
// use wasmtime_wasi_io::streams::{InputStream as IoInputStream, OutputStream as IoOutputStream, StreamError};
// use bytes::Bytes;

/// Host result type
type HostResult<T> = wasmtime::Result<T>;

/// Mount configuration
#[derive(Clone)]
struct Mount {
    /// Guest-visible path prefix (e.g., "/state")
    guest_prefix: String,
    /// VFS namespace (e.g., "state")
    vfs_namespace: String,
    /// Capabilities
    caps: MountCapabilities,
}

#[derive(Clone, Copy)]
struct MountCapabilities {
    read: bool,
    write: bool,
    append: bool,
}

/// Descriptor kinds in our filesystem
enum DescriptorKind {
    /// Directory descriptor
    Dir { mount_id: usize },
    /// File descriptor
    File { handle: FileHandle },
}

/// Directory entry stream for iterating directory contents
struct DirectoryStream {
    /// Path of the directory being iterated
    path: PathBuf,
    /// Mount associated with this directory
    mount_id: usize,
    /// Entries to be yielded
    entries: Vec<fs_types::DirectoryEntry>,
    /// Current position in the entries
    position: usize,
}

/// File handle for open files
#[derive(Clone)]
struct FileHandle {
    /// VFS file descriptor
    vfs_fd: u64,
    /// Current position in file
    position: u64,
    /// Reference to VFS
    vfs: Arc<Mutex<VirtualFilesystem>>,
    /// Whether file is open for writing
    writable: bool,
}

/// Our VFS-backed filesystem implementation
pub struct VfsFilesystem {
    /// Resource table for managing WASI resources
    table: ResourceTable,
    /// The underlying VFS
    vfs: Arc<Mutex<VirtualFilesystem>>,
    /// Configured mounts
    mounts: Vec<Mount>,
    /// Descriptor mapping
    descriptors: HashMap<u32, DescriptorKind>,
    /// Next descriptor ID
    next_descriptor: u32,
    /// Directory streams mapping
    directory_streams: HashMap<u32, DirectoryStream>,
    /// Next stream ID
    next_stream_id: u32,
}

impl VfsFilesystem {
    pub fn new(vfs: Arc<Mutex<VirtualFilesystem>>) -> Self {
        let mut fs = Self {
            table: ResourceTable::new(),
            vfs,
            mounts: Vec::new(),
            descriptors: HashMap::new(),
            next_descriptor: 10, // Start after standard descriptors
            directory_streams: HashMap::new(),
            next_stream_id: 1,
        };

        // Set up default mounts
        fs.setup_default_mounts();
        fs
    }

    fn setup_default_mounts(&mut self) {
        // Root mount - maps to "state" namespace
        self.mounts.push(Mount {
            guest_prefix: "/".to_string(),
            vfs_namespace: "state".to_string(),
            caps: MountCapabilities {
                read: true,
                write: true,
                append: true,
            },
        });

        // Config mount - read-only
        self.mounts.push(Mount {
            guest_prefix: "/config".to_string(),
            vfs_namespace: "config".to_string(),
            caps: MountCapabilities {
                read: true,
                write: false,
                append: false,
            },
        });

        // System mount - read-only
        self.mounts.push(Mount {
            guest_prefix: "/system".to_string(),
            vfs_namespace: "system".to_string(),
            caps: MountCapabilities {
                read: true,
                write: false,
                append: false,
            },
        });
    }

    /// Resolve guest path to VFS namespace and path
    fn resolve_path(&self, guest_path: &str) -> Result<(String, String), VfsError> {
        // Find the mount with the longest matching prefix
        let mut best_mount = None;
        let mut best_prefix_len = 0;

        for mount in &self.mounts {
            if guest_path.starts_with(&mount.guest_prefix)
                && mount.guest_prefix.len() > best_prefix_len
            {
                best_mount = Some(mount);
                best_prefix_len = mount.guest_prefix.len();
            }
        }

        match best_mount {
            Some(mount) => {
                let relative_path = &guest_path[mount.guest_prefix.len()..];
                let relative_path = relative_path.trim_start_matches('/');
                Ok((mount.vfs_namespace.clone(), relative_path.to_string()))
            }
            None => Err(VfsError::PathNotFound(
                "No mount found for path".to_string(),
            )),
        }
    }

    /// Get mount capabilities for a path
    fn get_mount_caps(&self, guest_path: &str) -> Option<MountCapabilities> {
        for mount in &self.mounts {
            if guest_path.starts_with(&mount.guest_prefix) {
                return Some(mount.caps);
            }
        }
        None
    }

    /// Convert VFS error to WASI error
    fn convert_vfs_error(err: VfsError) -> fs_types::ErrorCode {
        match err {
            VfsError::PathNotFound(_) => fs_types::ErrorCode::NoEntry,
            VfsError::FileExists(_) => fs_types::ErrorCode::Exist,
            VfsError::InvalidPath(_) => fs_types::ErrorCode::NotDirectory,
            VfsError::AccessDenied(_) => fs_types::ErrorCode::Access,
            VfsError::FdNotFound(_) => fs_types::ErrorCode::BadDescriptor,
            VfsError::InvalidOperation(_) => fs_types::ErrorCode::Unsupported,
            VfsError::StoreError(_) => fs_types::ErrorCode::Io,
            VfsError::IoError(_) => fs_types::ErrorCode::Io,
            VfsError::SerializationError(_) => fs_types::ErrorCode::Io,
            VfsError::DirectoryNotEmpty(_) => fs_types::ErrorCode::Io,
            _ => fs_types::ErrorCode::Io,
        }
    }
}

impl preopens::Host for VfsFilesystem {
    fn get_directories(&mut self) -> HostResult<Vec<(Resource<fs_types::Descriptor>, String)>> {
        let mut preopens: Vec<(Resource<fs_types::Descriptor>, String)> = Vec::new();

        // Create a descriptor for each mount point
        for (mount_id, mount) in self.mounts.iter().enumerate() {
            let descriptor_id = self.next_descriptor;
            self.next_descriptor += 1;

            self.descriptors
                .insert(descriptor_id, DescriptorKind::Dir { mount_id });

            // Hand back a handle with our descriptor id as the representation
            preopens.push((Resource::new_own(descriptor_id), mount.guest_prefix.clone()));
        }

        Ok(preopens)
    }
}

impl fs_types::Host for VfsFilesystem {
    fn filesystem_error_code(
        &mut self,
        err: Resource<fs_types::Error>,
    ) -> HostResult<Option<fs_types::ErrorCode>> {
        // For now, just return a generic error
        // In a real implementation, we'd store error details in the resource table
        Ok(Some(fs_types::ErrorCode::Io))
    }

    fn convert_error_code(
        &mut self,
        err: wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    ) -> HostResult<fs_types::ErrorCode> {
        // Extract the error code from TrappableError
        match err.downcast() {
            Ok(error_code) => Ok(error_code),
            Err(_) => Ok(fs_types::ErrorCode::Io),
        }
    }
}

impl fs_types::HostDescriptor for VfsFilesystem {
    async fn advise(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _offset: fs_types::Filesize,
        _length: fs_types::Filesize,
        _advice: fs_types::Advice,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // Advisory operation - can be ignored
        Ok(())
    }

    async fn sync_data(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // VFS operations are synchronous
        Ok(())
    }

    async fn get_flags(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
    ) -> Result<fs_types::DescriptorFlags, wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // Return default flags
        Ok(fs_types::DescriptorFlags::empty())
    }

    async fn get_type(
        &mut self,
        descriptor: Resource<fs_types::Descriptor>,
    ) -> Result<fs_types::DescriptorType, wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        let fd = descriptor.rep();
        match self.descriptors.get(&fd) {
            Some(DescriptorKind::Dir { .. }) => Ok(fs_types::DescriptorType::Directory),
            Some(DescriptorKind::File { .. }) => Ok(fs_types::DescriptorType::RegularFile),
            None => Err(fs_types::ErrorCode::BadDescriptor.into()),
        }
    }

    async fn set_size(
        &mut self,
        descriptor: Resource<fs_types::Descriptor>,
        size: fs_types::Filesize,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // TODO: Implement set_size
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn set_times(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _data_access_timestamp: fs_types::NewTimestamp,
        _data_modification_timestamp: fs_types::NewTimestamp,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // Timestamps not supported in VFS
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn read(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _length: fs_types::Filesize,
        _offset: fs_types::Filesize,
    ) -> Result<(Vec<u8>, bool), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // Direct read not implemented - use streams
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn write(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _buffer: Vec<u8>,
        _offset: fs_types::Filesize,
    ) -> Result<fs_types::Filesize, wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // Direct write not implemented - use streams
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn read_directory(
        &mut self,
        descriptor: Resource<fs_types::Descriptor>,
    ) -> Result<
        Resource<fs_types::DirectoryEntryStream>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        let fd = descriptor.rep();
        let (mount_id, dir_path) = match self.descriptors.get(&fd) {
            Some(DescriptorKind::Dir { mount_id }) => {
                let mount = &self.mounts[*mount_id];
                (*mount_id, PathBuf::from(&mount.guest_prefix))
            }
            _ => return Err(fs_types::ErrorCode::NotDirectory.into()),
        };

        // Get the mount and namespace
        let mount = &self.mounts[mount_id];
        let namespace = &mount.vfs_namespace;

        // List all entries in this directory
        let mut entries = Vec::new();
        
        // Create VFS path for listing
        let vfs_path = PathBuf::from(format!("/{}/", namespace));
        
        // Get entries from VFS
        let vfs = self.vfs.lock().unwrap();
        
        // Use the VFS to list keys with the namespace prefix
        if let Some(store) = vfs.get_store(namespace) {
            let store = store.lock().unwrap();
            let iter = store.prefix_iterator(&[]);
            
            for (key, _) in iter {
                if let Ok(key_str) = String::from_utf8(key.clone()) {
                    // Create a directory entry for each key
                    let entry = fs_types::DirectoryEntry {
                        type_: fs_types::DescriptorType::RegularFile,
                        name: key_str,
                    };
                    entries.push(entry);
                }
            }
        }
        
        // Create the directory stream
        let stream = DirectoryStream {
            path: dir_path,
            mount_id,
            entries,
            position: 0,
        };
        
        // Store the stream in our HashMap and return a resource handle
        let stream_id = self.next_stream_id;
        self.next_stream_id += 1;
        self.directory_streams.insert(stream_id, stream);
        
        // Create a resource handle with our stream ID
        Ok(Resource::new_own(stream_id))
    }

    async fn sync(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // VFS operations are synchronous
        Ok(())
    }

    async fn create_directory_at(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // TODO: Implement directory creation
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn stat(
        &mut self,
        descriptor: Resource<fs_types::Descriptor>,
    ) -> Result<fs_types::DescriptorStat, wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        let fd = descriptor.rep();
        match self.descriptors.get(&fd) {
            Some(DescriptorKind::Dir { .. }) => Ok(fs_types::DescriptorStat {
                type_: fs_types::DescriptorType::Directory,
                link_count: 1,
                size: 0,
                data_access_timestamp: None,
                data_modification_timestamp: None,
                status_change_timestamp: None,
            }),
            Some(DescriptorKind::File { .. }) => Ok(fs_types::DescriptorStat {
                type_: fs_types::DescriptorType::RegularFile,
                link_count: 1,
                size: 0,
                data_access_timestamp: None,
                data_modification_timestamp: None,
                status_change_timestamp: None,
            }),
            None => Err(fs_types::ErrorCode::BadDescriptor.into()),
        }
    }

    async fn stat_at(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _path_flags: fs_types::PathFlags,
        _path: String,
    ) -> Result<fs_types::DescriptorStat, wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // TODO: Implement stat_at
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn set_times_at(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _path_flags: fs_types::PathFlags,
        _path: String,
        _data_access_timestamp: fs_types::NewTimestamp,
        _data_modification_timestamp: fs_types::NewTimestamp,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // Timestamps not supported
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn link_at(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _old_path_flags: fs_types::PathFlags,
        _old_path: String,
        _new_descriptor: Resource<fs_types::Descriptor>,
        _new_path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // Links not supported
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn open_at(
        &mut self,
        descriptor: Resource<fs_types::Descriptor>,
        path_flags: fs_types::PathFlags,
        path: String,
        open_flags: fs_types::OpenFlags,
        flags: fs_types::DescriptorFlags,
    ) -> Result<Resource<fs_types::Descriptor>, wasmtime_wasi::TrappableError<fs_types::ErrorCode>>
    {
        let dir_fd = descriptor.rep();
        // Validate directory descriptor
        let mount_id = match self.descriptors.get(&dir_fd) {
            Some(DescriptorKind::Dir { mount_id }) => *mount_id,
            Some(DescriptorKind::File { .. }) => {
                return Err(fs_types::ErrorCode::NotDirectory.into())
            }
            None => return Err(fs_types::ErrorCode::BadDescriptor.into()),
        };

        let mount = &self.mounts[mount_id];
        let writable = flags.contains(fs_types::DescriptorFlags::WRITE)
            || open_flags.contains(fs_types::OpenFlags::CREATE)
            || open_flags.contains(fs_types::OpenFlags::TRUNCATE);

        if writable && !mount.caps.write {
            return Err(fs_types::ErrorCode::Access.into());
        }

        // Build VFS path: /{namespace}/{path}
        let vfs_path = if path.is_empty() {
            PathBuf::from(format!("/{}/", mount.vfs_namespace))
        } else {
            PathBuf::from(format!("/{}/{}", mount.vfs_namespace, path))
        };

        // Ensure capabilities for this path (coarse-grained)
        {
            let vfs = self.vfs.lock().unwrap();
            let _ = vfs.add_capability(Capability::Read(vfs_path.clone()));
            if writable {
                let _ = vfs.add_capability(Capability::Write(vfs_path.clone()));
            }
        }

        // Open or create in VFS
        let vfs_fd = {
            let vfs = self.vfs.lock().unwrap();
            if open_flags.contains(fs_types::OpenFlags::CREATE) {
                vfs.create(&vfs_path)
                    .map_err(|e| wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e)))?
            } else {
                vfs.open(&vfs_path, writable)
                    .map_err(|e| wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e)))?
            }
        } as u64;

        // Register descriptor
        let new_fd = self.next_descriptor;
        self.next_descriptor += 1;
        self.descriptors.insert(
            new_fd,
            DescriptorKind::File {
                handle: FileHandle {
                    vfs_fd,
                    position: 0,
                    vfs: self.vfs.clone(),
                    writable,
                },
            },
        );

        Ok(Resource::new_own(new_fd))
    }

    fn drop(&mut self, descriptor: Resource<fs_types::Descriptor>) -> HostResult<()> {
        let fd = descriptor.rep();
        // If this was a file descriptor, close underlying VFS fd
        if let Some(kind) = self.descriptors.remove(&fd) {
            if let DescriptorKind::File { handle } = kind {
                let _ = self.vfs.lock().unwrap().close(handle.vfs_fd as u32);
            }
        }
        // Do not call table.delete for descriptors; they are tracked in descriptors map
        Ok(())
    }

    async fn readlink_at(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _path: String,
    ) -> Result<String, wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // Symlinks not supported
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn remove_directory_at(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // TODO: Implement directory removal
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn rename_at(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _old_path: String,
        _new_descriptor: Resource<fs_types::Descriptor>,
        _new_path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // TODO: Implement rename
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn symlink_at(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _old_path: String,
        _new_path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // Symlinks not supported
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn unlink_file_at(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        // TODO: Implement file unlinking
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn is_same_object(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _other: Resource<fs_types::Descriptor>,
    ) -> HostResult<bool> {
        // TODO: Implement same object check
        Ok(false)
    }

    async fn metadata_hash(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
    ) -> Result<fs_types::MetadataHashValue, wasmtime_wasi::TrappableError<fs_types::ErrorCode>>
    {
        // TODO: Implement metadata hash
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    async fn metadata_hash_at(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _path_flags: fs_types::PathFlags,
        _path: String,
    ) -> Result<fs_types::MetadataHashValue, wasmtime_wasi::TrappableError<fs_types::ErrorCode>>
    {
        // TODO: Implement metadata hash at path
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    fn read_via_stream(
        &mut self,
        descriptor: Resource<fs_types::Descriptor>,
        offset: fs_types::Filesize,
    ) -> Result<Resource<io_streams::InputStream>, wasmtime_wasi::TrappableError<fs_types::ErrorCode>>
    {
        // Stream support deferred; use Unsupported for now
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    fn write_via_stream(
        &mut self,
        descriptor: Resource<fs_types::Descriptor>,
        offset: fs_types::Filesize,
    ) -> Result<
        Resource<io_streams::OutputStream>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        // Stream support deferred; use Unsupported for now
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    fn append_via_stream(
        &mut self,
        descriptor: Resource<fs_types::Descriptor>,
    ) -> Result<
        Resource<io_streams::OutputStream>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        // Stream support deferred; use Unsupported for now
        Err(fs_types::ErrorCode::Unsupported.into())
    }
}

impl fs_types::HostDirectoryEntryStream for VfsFilesystem {
    async fn read_directory_entry(
        &mut self,
        stream: Resource<fs_types::DirectoryEntryStream>,
    ) -> Result<Option<fs_types::DirectoryEntry>, wasmtime_wasi::TrappableError<fs_types::ErrorCode>>
    {
        // Get the stream ID from the resource
        let stream_id = stream.rep();
        
        // Get the stream from our HashMap
        let dir_stream = self.directory_streams.get_mut(&stream_id)
            .ok_or_else(|| wasmtime_wasi::TrappableError::from(fs_types::ErrorCode::BadDescriptor))?;
        
        // Check if we have more entries to yield
        if dir_stream.position < dir_stream.entries.len() {
            let entry = dir_stream.entries[dir_stream.position].clone();
            dir_stream.position += 1;
            Ok(Some(entry))
        } else {
            // No more entries
            Ok(None)
        }
    }

    fn drop(&mut self, stream: Resource<fs_types::DirectoryEntryStream>) -> HostResult<()> {
        // Remove the stream from our HashMap
        let stream_id = stream.rep();
        self.directory_streams.remove(&stream_id);
        Ok(())
    }
}

/// Input stream type (placeholder for future stream support)
pub struct VfsInputStream {
    handle: FileHandle,
}

/// Output stream type (placeholder for future stream support)
pub struct VfsOutputStream {
    handle: FileHandle,
}

/// Always-ready pollable for streams
pub struct AlwaysReadyPollable;

impl VfsInputStream {
    fn new(handle: FileHandle) -> Self {
        Self { handle }
    }
}

impl VfsOutputStream {
    fn new(handle: FileHandle) -> Self {
        Self { handle }
    }
}

// Stream trait implementations will be added in a future phase

#[cfg(test)]
mod tests {
    use super::*;
    use gridway_store::{KVStore, MemStore};
    use wasmtime_wasi::p2::bindings::filesystem::{preopens, types as fs_types};

    #[tokio::test]
    async fn test_directory_iteration() {
        // Create a VFS with some test data
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let store = Arc::new(Mutex::new(MemStore::new()));
        
        // Add some test entries to the store
        {
            let mut store = store.lock().unwrap();
            store.set(b"file1.txt", b"content1").unwrap();
            store.set(b"file2.txt", b"content2").unwrap();
            store.set(b"subdir/file3.txt", b"content3").unwrap();
        }
        
        // Mount the store
        vfs.lock().unwrap().mount_store("state".to_string(), store.clone()).unwrap();
        
        // Create VFS filesystem
        let mut fs = VfsFilesystem::new(vfs);
        
        // Get the preopened directory descriptor for root using the trait
        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        assert!(!preopens.is_empty());
        
        let (root_descriptor, _path) = preopens.remove(0);
        
        // Read the directory using the trait
        let stream = <VfsFilesystem as fs_types::HostDescriptor>::read_directory(&mut fs, root_descriptor).await.unwrap();
        
        // Read entries from the stream using the trait
        let mut entries = Vec::new();
        let stream_id = stream.rep();  // Get the stream ID for repeated use
        loop {
            // Use the stream ID to create a new resource handle for each call
            let stream_ref = Resource::new_borrow(stream_id);
            match <VfsFilesystem as fs_types::HostDirectoryEntryStream>::read_directory_entry(&mut fs, stream_ref).await.unwrap() {
                Some(entry) => entries.push(entry.name),
                None => break,
            }
        }
        
        // Verify we got the expected entries
        assert!(entries.contains(&"file1.txt".to_string()));
        assert!(entries.contains(&"file2.txt".to_string()));
        assert!(entries.contains(&"subdir/file3.txt".to_string()));
        
        // Clean up using the trait
        <VfsFilesystem as fs_types::HostDirectoryEntryStream>::drop(&mut fs, stream).unwrap();
    }
}

/// Custom context that uses our VFS filesystem
pub struct VfsWasiContext {
    /// Standard WASI context for non-filesystem operations
    wasi: wasmtime_wasi::p2::WasiCtx,
    /// Our VFS filesystem
    pub fs: VfsFilesystem,
}

impl VfsWasiContext {
    pub fn new(vfs: Arc<Mutex<VirtualFilesystem>>) -> Self {
        let wasi = wasmtime_wasi::p2::WasiCtxBuilder::new()
            .inherit_stdio()
            .inherit_env()
            .build();

        let fs = VfsFilesystem::new(vfs);

        Self { wasi, fs }
    }
}

impl WasiView for VfsWasiContext {
    fn ctx(&mut self) -> &mut wasmtime_wasi::p2::WasiCtx {
        &mut self.wasi
    }
}

impl IoView for VfsWasiContext {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.fs.table
    }
}

// Implement poll host
impl io_poll::Host for VfsWasiContext {
    async fn poll(
        &mut self,
        pollables: Vec<Resource<io_poll::Pollable>>,
    ) -> wasmtime::Result<Vec<u32>> {
        // All our pollables are always ready
        Ok((0..pollables.len() as u32).collect())
    }
}

impl io_poll::HostPollable for VfsWasiContext {
    async fn ready(&mut self, _pollable: Resource<io_poll::Pollable>) -> wasmtime::Result<bool> {
        Ok(true)
    }

    async fn block(&mut self, _pollable: Resource<io_poll::Pollable>) -> wasmtime::Result<()> {
        Ok(())
    }

    fn drop(&mut self, pollable: Resource<io_poll::Pollable>) -> wasmtime::Result<()> {
        self.fs.table.delete(pollable)?;
        Ok(())
    }
}
