//! VFS WASI Implementation
//!
//! This module implements WASI filesystem interfaces directly, routing all filesystem
//! operations to our Merkle-backed VFS. The key insight is to NOT use wasmtime-wasi's
//! filesystem implementation at all, but provide our own implementation of the WASI
//! filesystem interfaces.

use crate::vfs::{Capability, VfsError, VirtualFilesystem};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use wasmtime::component::Resource;
use wasmtime_wasi::p2::bindings::filesystem::{preopens, types as fs_types};
use wasmtime_wasi::p2::bindings::io::poll::{self as io_poll};
use wasmtime_wasi::p2::bindings::io::streams::{self as io_streams};
use wasmtime_wasi::p2::{IoView, WasiView};
use wasmtime_wasi::ResourceTable;
use crate::vfs_streams_simple::{FileHandle, AlwaysReadyPollable};

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

// FileHandle has been moved to vfs_streams.rs and made public

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
}

impl VfsFilesystem {
    pub fn new(vfs: Arc<Mutex<VirtualFilesystem>>) -> Self {
        let mut fs = Self {
            table: ResourceTable::new(),
            vfs,
            mounts: Vec::new(),
            descriptors: HashMap::new(),
            next_descriptor: 10, // Start after standard descriptors
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
        match self.descriptors.get(&fd) {
            Some(DescriptorKind::Dir { .. }) => {
                // Cannot construct a typed DirectoryEntryStream without internal types; return Unsupported
                Err(fs_types::ErrorCode::Unsupported.into())
            }
            _ => Err(fs_types::ErrorCode::NotDirectory.into()),
        }
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
                return Err(fs_types::ErrorCode::Access.into())
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
        _descriptor: Resource<fs_types::Descriptor>,
        _offset: fs_types::Filesize,
    ) -> Result<Resource<io_streams::InputStream>, wasmtime_wasi::TrappableError<fs_types::ErrorCode>>
    {
        // Stream support will be added in a future iteration
        // For now, return unsupported to allow the system to compile
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    fn write_via_stream(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
        _offset: fs_types::Filesize,
    ) -> Result<
        Resource<io_streams::OutputStream>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        // Stream support will be added in a future iteration
        // For now, return unsupported to allow the system to compile
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    fn append_via_stream(
        &mut self,
        _descriptor: Resource<fs_types::Descriptor>,
    ) -> Result<
        Resource<io_streams::OutputStream>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        // Stream support will be added in a future iteration
        // For now, return unsupported to allow the system to compile
        Err(fs_types::ErrorCode::Unsupported.into())
    }
}

impl fs_types::HostDirectoryEntryStream for VfsFilesystem {
    async fn read_directory_entry(
        &mut self,
        stream: Resource<fs_types::DirectoryEntryStream>,
    ) -> Result<Option<fs_types::DirectoryEntry>, wasmtime_wasi::TrappableError<fs_types::ErrorCode>>
    {
        // Since we can't create DirectoryEntryStream resources directly,
        // we return None for now
        Ok(None)
    }

    fn drop(&mut self, stream: Resource<fs_types::DirectoryEntryStream>) -> HostResult<()> {
        self.table.delete(stream)?;
        Ok(())
    }
}

// Stream implementations have been moved to vfs_streams.rs

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

// Stream host traits are automatically provided by wasmtime-wasi
// when we return boxed InputStream/OutputStream instances
