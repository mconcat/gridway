//! VFS WASI Implementation
//!
//! This module implements WASI filesystem interfaces directly, routing all filesystem
//! operations to our Merkle-backed VFS. The key insight is to NOT use wasmtime-wasi's
//! filesystem implementation at all, but provide our own implementation of the WASI
//! filesystem interfaces.

use crate::vfs::{VfsError, VirtualFilesystem};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::component::Resource;
use wasmtime_wasi::p2::bindings::filesystem::{preopens, types as fs_types};
use wasmtime_wasi::p2::bindings::io::poll::{self as io_poll};
use wasmtime_wasi::p2::bindings::io::streams::{self as io_streams};
use wasmtime_wasi::p2::{IoView, WasiView};
use wasmtime_wasi::ResourceTable;

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
        let preopens: Vec<(Resource<fs_types::Descriptor>, String)> = Vec::new();

        // Create a descriptor for each mount point
        for (mount_id, mount) in self.mounts.iter().enumerate() {
            let descriptor_id = self.next_descriptor;
            self.next_descriptor += 1;

            self.descriptors
                .insert(descriptor_id, DescriptorKind::Dir { mount_id });

            // Skip creating descriptors for now - this needs proper implementation
            // when we have access to the actual Descriptor type from wasmtime-wasi
        }

        // Return empty preopens for now
        Ok(Vec::new())
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
        let desc = self.table.get(&descriptor)?;

        // For now, determine type based on descriptor kind
        // In a real implementation, we'd check the actual file type
        Ok(fs_types::DescriptorType::RegularFile)
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
        // TODO: Implement directory reading - for now return empty iterator
        // We need to create our own iterator that implements the required trait
        // Since ReaddirIterator is not public, we'll work around this
        // by creating a placeholder DirectoryEntryStream

        // For now, just create an empty resource
        // The actual implementation will need to properly handle directory reading
        Err(fs_types::ErrorCode::Unsupported.into())
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
        // TODO: Implement stat
        Ok(fs_types::DescriptorStat {
            type_: fs_types::DescriptorType::RegularFile,
            link_count: 1,
            size: 0,
            data_access_timestamp: None,
            data_modification_timestamp: None,
            status_change_timestamp: None,
        })
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
        // TODO: Implement open_at properly
        // For now, return not supported
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    fn drop(&mut self, descriptor: Resource<fs_types::Descriptor>) -> HostResult<()> {
        self.table.delete(descriptor)?;
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
        // TODO: Create proper VFS input stream
        let handle = FileHandle {
            vfs_fd: 0, // TODO: Get from descriptor
            position: offset,
            vfs: self.vfs.clone(),
            writable: false,
        };

        // For now, return Unsupported since we can't create proper stream resources
        // without implementing the full wasmtime-wasi-io traits
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
        // TODO: Create proper VFS output stream
        let handle = FileHandle {
            vfs_fd: 0, // TODO: Get from descriptor
            position: offset,
            vfs: self.vfs.clone(),
            writable: true,
        };

        // For now, return Unsupported since we can't create proper stream resources
        // without implementing the full wasmtime-wasi-io traits
        Err(fs_types::ErrorCode::Unsupported.into())
    }

    fn append_via_stream(
        &mut self,
        descriptor: Resource<fs_types::Descriptor>,
    ) -> Result<
        Resource<io_streams::OutputStream>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        // TODO: Get file size for append position
        self.write_via_stream(descriptor, 0)
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

/// Input stream type
pub struct VfsInputStream {
    handle: FileHandle,
}

/// Output stream type  
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

// TODO: Stream support will be added later
// For now, streams are not supported and return Unsupported errors
/*
// The stream traits are implemented on the context, not the stream types
impl io_streams::HostInputStream for VfsWasiContext {
    fn read(&mut self, stream: Resource<io_streams::InputStream>, size: u64) -> wasmtime::Result<Vec<u8>, StreamError> {
        let stream_ref = self.fs.table.get(&stream)?;
        let vfs_stream = stream_ref
            .downcast_ref::<VfsInputStream>()
            .ok_or_else(|| anyhow::anyhow!("not a VFS input stream"))?;
        let mut handle = vfs_stream.handle.clone();

        let vfs = handle.vfs.lock().unwrap();

        // Seek to position
        match vfs.seek(handle.vfs_fd, SeekFrom::Start(handle.position)) {
            Ok(_) => {},
            Err(_) => return Err(StreamError::Closed),
        }

        // Read data
        let mut buffer = vec![0u8; size as usize];
        let bytes_read = match vfs.read(handle.vfs_fd, &mut buffer) {
            Ok(n) => n,
            Err(_) => return Err(StreamError::Closed),
        };

        buffer.truncate(bytes_read);
        drop(vfs);

        // Update position in the stream
        let stream_mut = self.fs.table.get_mut(&stream)?;
        let vfs_stream_mut = stream_mut
            .downcast_mut::<VfsInputStream>()
            .ok_or_else(|| anyhow::anyhow!("not a VFS input stream"))?;
        vfs_stream_mut.handle.position += bytes_read as u64;

        Ok(Ok(buffer))
    }

    async fn blocking_read(&mut self, stream: Resource<io_streams::InputStream>, size: u64) -> wasmtime::Result<Vec<u8>, StreamError> {
        self.read(stream, size)
    }

    fn skip(&mut self, stream: Resource<io_streams::InputStream>, amount: u64) -> wasmtime::Result<u64, StreamError> {
        let stream_mut = self.fs.table.get_mut(&stream)?;
        let vfs_stream = stream_mut
            .downcast_mut::<VfsInputStream>()
            .ok_or_else(|| anyhow::anyhow!("not a VFS input stream"))?;
        vfs_stream.handle.position += amount;
        Ok(amount)
    }

    async fn blocking_skip(&mut self, stream: Resource<io_streams::InputStream>, amount: u64) -> wasmtime::Result<u64, StreamError> {
        self.skip(stream, amount)
    }

    fn subscribe(&mut self, _stream: Resource<io_streams::InputStream>) -> wasmtime::Result<Resource<io_poll::Pollable>> {
        // For now, return an error as we don't have proper pollable implementation
        Err(anyhow::anyhow!("subscribe not implemented"))
    }

    async fn drop(&mut self, stream: Resource<io_streams::InputStream>) -> wasmtime::Result<()> {
        self.fs.table.delete(stream)?;
        Ok(())
    }
}

impl io_streams::HostOutputStream for VfsWasiContext {
    fn write(&mut self, stream: Resource<io_streams::OutputStream>, bytes: Vec<u8>) -> wasmtime::Result<(), StreamError> {
        let stream_ref = self.fs.table.get(&stream)?;
        let vfs_stream = stream_ref
            .downcast_ref::<VfsOutputStream>()
            .ok_or_else(|| anyhow::anyhow!("not a VFS output stream"))?;
        let mut handle = vfs_stream.handle.clone();

        let vfs = handle.vfs.lock().unwrap();

        // Seek to position
        match vfs.seek(handle.vfs_fd, SeekFrom::Start(handle.position)) {
            Ok(_) => {},
            Err(_) => return Err(StreamError::Closed),
        }

        // Write data
        let bytes_written = match vfs.write(handle.vfs_fd, &bytes) {
            Ok(n) => n,
            Err(_) => return Err(StreamError::Closed),
        };

        drop(vfs);

        // Update position in the stream
        let stream_mut = self.fs.table.get_mut(&stream)?;
        let vfs_stream_mut = stream_mut
            .downcast_mut::<VfsOutputStream>()
            .ok_or_else(|| anyhow::anyhow!("not a VFS output stream"))?;
        vfs_stream_mut.handle.position += bytes_written as u64;

        Ok(Ok(()))
    }

    async fn blocking_write_and_flush(&mut self, stream: Resource<io_streams::OutputStream>, bytes: Vec<u8>) -> wasmtime::Result<(), StreamError> {
        self.write(stream, bytes)
    }

    fn flush(&mut self, _stream: Resource<io_streams::OutputStream>) -> wasmtime::Result<(), StreamError> {
        // VFS operations are synchronous
        Ok(Ok(()))
    }

    async fn blocking_flush(&mut self, stream: Resource<io_streams::OutputStream>) -> wasmtime::Result<(), StreamError> {
        self.flush(stream)
    }

    fn check_write(&mut self, _stream: Resource<io_streams::OutputStream>) -> wasmtime::Result<u64, StreamError> {
        Ok(1024 * 1024) // 1MB write buffer
    }

    fn write_zeroes(&mut self, stream: Resource<io_streams::OutputStream>, len: u64) -> wasmtime::Result<(), StreamError> {
        let zeroes = vec![0u8; len as usize];
        self.write(stream, zeroes)
    }

    async fn blocking_write_zeroes_and_flush(&mut self, stream: Resource<io_streams::OutputStream>, len: u64) -> wasmtime::Result<(), StreamError> {
        self.write_zeroes(stream, len)
    }

    fn splice(&mut self, _dst: Resource<io_streams::OutputStream>, _src: Resource<io_streams::InputStream>, _len: u64) -> wasmtime::Result<u64, StreamError> {
        Err(StreamError::Closed)
    }

    async fn blocking_splice(&mut self, dst: Resource<io_streams::OutputStream>, src: Resource<io_streams::InputStream>, len: u64) -> wasmtime::Result<u64, StreamError> {
        self.splice(dst, src, len)
    }

    fn subscribe(&mut self, _stream: Resource<io_streams::OutputStream>) -> wasmtime::Result<Resource<io_poll::Pollable>> {
        // For now, return an error as we don't have proper pollable implementation
        Err(anyhow::anyhow!("subscribe not implemented"))
    }

    async fn drop(&mut self, stream: Resource<io_streams::OutputStream>) -> wasmtime::Result<()> {
        self.fs.table.delete(stream)?;
        Ok(())
    }
}
*/

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
