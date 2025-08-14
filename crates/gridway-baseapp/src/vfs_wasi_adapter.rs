//! VFS WASI Adapter
//!
//! This module implements WASI filesystem interfaces directly for our VFS,
//! routing all filesystem operations to our Merkle-backed VFS.
//!
//! This follows the solution pattern: Instead of trying to swap out wasmtime-wasi's
//! filesystem from the inside, we provide our own implementation of the
//! wasi:filesystem preview-2 interfaces directly.

use crate::vfs::VirtualFilesystem;
use crate::vfs_wasi_impl::VfsWasiContext;
use std::sync::{Arc, Mutex};
use wasmtime::component::Resource;
use wasmtime_wasi::p2::bindings::filesystem::{preopens, types as fs_types};
use wasmtime_wasi::p2::bindings::io::{poll as io_poll, streams as io_streams};
use wasmtime_wasi::p2::{IoView, WasiCtx, WasiView};
use wasmtime_wasi::ResourceTable;

/// VFS WASI Adapter that wraps our VfsWasiContext
pub struct VfsWasiAdapter {
    /// The actual VFS WASI implementation
    inner: VfsWasiContext,
}

impl VfsWasiAdapter {
    /// Create a new VFS WASI adapter
    pub fn new(vfs: Arc<Mutex<VirtualFilesystem>>) -> Self {
        Self {
            inner: VfsWasiContext::new(vfs),
        }
    }
}

impl WasiView for VfsWasiAdapter {
    fn ctx(&mut self) -> &mut WasiCtx {
        self.inner.ctx()
    }
}

impl wasmtime_wasi::p2::IoView for VfsWasiAdapter {
    fn table(&mut self) -> &mut ResourceTable {
        self.inner.table()
    }
}

// Forward all filesystem trait implementations to the inner VfsWasiContext
impl preopens::Host for VfsWasiAdapter {
    fn get_directories(
        &mut self,
    ) -> wasmtime::Result<Vec<(wasmtime::component::Resource<fs_types::Descriptor>, String)>> {
        self.inner.fs.get_directories()
    }
}

impl fs_types::Host for VfsWasiAdapter {
    fn filesystem_error_code(
        &mut self,
        err: Resource<fs_types::Error>,
    ) -> wasmtime::Result<Option<fs_types::ErrorCode>> {
        self.inner.fs.filesystem_error_code(err)
    }

    fn convert_error_code(
        &mut self,
        err: wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    ) -> wasmtime::Result<fs_types::ErrorCode> {
        self.inner.fs.convert_error_code(err)
    }
}

impl fs_types::HostDescriptor for VfsWasiAdapter {
    async fn open_at(
        &mut self,
        dirfd: wasmtime::component::Resource<fs_types::Descriptor>,
        path_flags: fs_types::PathFlags,
        path: String,
        open_flags: fs_types::OpenFlags,
        flags: fs_types::DescriptorFlags,
    ) -> Result<
        wasmtime::component::Resource<fs_types::Descriptor>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        self.inner
            .fs
            .open_at(dirfd, path_flags, path, open_flags, flags)
            .await
    }

    fn read_via_stream(
        &mut self,
        fd: wasmtime::component::Resource<fs_types::Descriptor>,
        offset: fs_types::Filesize,
    ) -> Result<
        wasmtime::component::Resource<io_streams::InputStream>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        self.inner.fs.read_via_stream(fd, offset)
    }

    fn write_via_stream(
        &mut self,
        fd: wasmtime::component::Resource<fs_types::Descriptor>,
        offset: fs_types::Filesize,
    ) -> Result<
        wasmtime::component::Resource<io_streams::OutputStream>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        self.inner.fs.write_via_stream(fd, offset)
    }

    fn append_via_stream(
        &mut self,
        fd: wasmtime::component::Resource<fs_types::Descriptor>,
    ) -> Result<
        wasmtime::component::Resource<io_streams::OutputStream>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        self.inner.fs.append_via_stream(fd)
    }

    async fn get_type(
        &mut self,
        fd: wasmtime::component::Resource<fs_types::Descriptor>,
    ) -> Result<fs_types::DescriptorType, wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.get_type(fd).await
    }

    async fn stat(
        &mut self,
        fd: wasmtime::component::Resource<fs_types::Descriptor>,
    ) -> Result<fs_types::DescriptorStat, wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.stat(fd).await
    }

    async fn read_directory(
        &mut self,
        fd: wasmtime::component::Resource<fs_types::Descriptor>,
    ) -> Result<
        wasmtime::component::Resource<fs_types::DirectoryEntryStream>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        self.inner.fs.read_directory(fd).await
    }

    async fn sync(
        &mut self,
        fd: wasmtime::component::Resource<fs_types::Descriptor>,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.sync(fd).await
    }

    fn drop(
        &mut self,
        fd: wasmtime::component::Resource<fs_types::Descriptor>,
    ) -> wasmtime::Result<()> {
        self.inner.fs.drop(fd)
    }

    async fn advise(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        offset: fs_types::Filesize,
        length: fs_types::Filesize,
        advice: fs_types::Advice,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner
            .fs
            .advise(descriptor, offset, length, advice)
            .await
    }

    async fn sync_data(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.sync_data(descriptor).await
    }

    async fn get_flags(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
    ) -> Result<fs_types::DescriptorFlags, wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.get_flags(descriptor).await
    }

    async fn set_size(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        size: fs_types::Filesize,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.set_size(descriptor, size).await
    }

    async fn set_times(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        data_access_timestamp: fs_types::NewTimestamp,
        data_modification_timestamp: fs_types::NewTimestamp,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner
            .fs
            .set_times(
                descriptor,
                data_access_timestamp,
                data_modification_timestamp,
            )
            .await
    }

    async fn read(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        length: fs_types::Filesize,
        offset: fs_types::Filesize,
    ) -> Result<(Vec<u8>, bool), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.read(descriptor, length, offset).await
    }

    async fn write(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        buffer: Vec<u8>,
        offset: fs_types::Filesize,
    ) -> Result<fs_types::Filesize, wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.write(descriptor, buffer, offset).await
    }

    async fn create_directory_at(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.create_directory_at(descriptor, path).await
    }

    async fn stat_at(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        path_flags: fs_types::PathFlags,
        path: String,
    ) -> Result<fs_types::DescriptorStat, wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.stat_at(descriptor, path_flags, path).await
    }

    async fn set_times_at(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        path_flags: fs_types::PathFlags,
        path: String,
        data_access_timestamp: fs_types::NewTimestamp,
        data_modification_timestamp: fs_types::NewTimestamp,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner
            .fs
            .set_times_at(
                descriptor,
                path_flags,
                path,
                data_access_timestamp,
                data_modification_timestamp,
            )
            .await
    }

    async fn link_at(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        old_path_flags: fs_types::PathFlags,
        old_path: String,
        new_descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        new_path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner
            .fs
            .link_at(
                descriptor,
                old_path_flags,
                old_path,
                new_descriptor,
                new_path,
            )
            .await
    }

    async fn readlink_at(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        path: String,
    ) -> Result<String, wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.readlink_at(descriptor, path).await
    }

    async fn remove_directory_at(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.remove_directory_at(descriptor, path).await
    }

    async fn rename_at(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        old_path: String,
        new_descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        new_path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner
            .fs
            .rename_at(descriptor, old_path, new_descriptor, new_path)
            .await
    }

    async fn symlink_at(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        old_path: String,
        new_path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner
            .fs
            .symlink_at(descriptor, old_path, new_path)
            .await
    }

    async fn unlink_file_at(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        self.inner.fs.unlink_file_at(descriptor, path).await
    }

    async fn is_same_object(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        other: wasmtime::component::Resource<fs_types::Descriptor>,
    ) -> wasmtime::Result<bool> {
        self.inner.fs.is_same_object(descriptor, other).await
    }

    async fn metadata_hash(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
    ) -> Result<fs_types::MetadataHashValue, wasmtime_wasi::TrappableError<fs_types::ErrorCode>>
    {
        self.inner.fs.metadata_hash(descriptor).await
    }

    async fn metadata_hash_at(
        &mut self,
        descriptor: wasmtime::component::Resource<fs_types::Descriptor>,
        path_flags: fs_types::PathFlags,
        path: String,
    ) -> Result<fs_types::MetadataHashValue, wasmtime_wasi::TrappableError<fs_types::ErrorCode>>
    {
        self.inner
            .fs
            .metadata_hash_at(descriptor, path_flags, path)
            .await
    }
}

impl fs_types::HostDirectoryEntryStream for VfsWasiAdapter {
    async fn read_directory_entry(
        &mut self,
        stream: wasmtime::component::Resource<fs_types::DirectoryEntryStream>,
    ) -> Result<Option<fs_types::DirectoryEntry>, wasmtime_wasi::TrappableError<fs_types::ErrorCode>>
    {
        self.inner.fs.read_directory_entry(stream).await
    }

    fn drop(
        &mut self,
        stream: wasmtime::component::Resource<fs_types::DirectoryEntryStream>,
    ) -> wasmtime::Result<()> {
        self.inner.fs.drop(stream)
    }
}

// Forward io::poll implementations
impl io_poll::Host for VfsWasiAdapter {
    async fn poll(
        &mut self,
        pollables: Vec<Resource<io_poll::Pollable>>,
    ) -> wasmtime::Result<Vec<u32>> {
        self.inner.poll(pollables).await
    }
}

impl io_poll::HostPollable for VfsWasiAdapter {
    async fn ready(&mut self, pollable: Resource<io_poll::Pollable>) -> wasmtime::Result<bool> {
        self.inner.ready(pollable).await
    }

    async fn block(&mut self, pollable: Resource<io_poll::Pollable>) -> wasmtime::Result<()> {
        self.inner.block(pollable).await
    }

    fn drop(&mut self, pollable: Resource<io_poll::Pollable>) -> wasmtime::Result<()> {
        self.inner.drop(pollable)
    }
}

// Forward io::error trait implementation
impl wasmtime_wasi::p2::bindings::io::error::Host for VfsWasiAdapter {}

// Forward io::error::HostError trait implementation
impl wasmtime_wasi::p2::bindings::io::error::HostError for VfsWasiAdapter {
    fn drop(&mut self, error: Resource<io_streams::Error>) -> wasmtime::Result<()> {
        self.inner.table().delete(error)?;
        Ok(())
    }
    
    fn to_debug_string(&mut self, error: Resource<io_streams::Error>) -> wasmtime::Result<String> {
        Ok(format!("{:?}", self.inner.table().get(&error)?))
    }
}

// Stream support will be properly implemented in a future iteration
// For now, the stream methods in VfsFilesystem return Unsupported
impl io_streams::Host for VfsWasiAdapter {
    fn convert_stream_error(
        &mut self,
        err: wasmtime_wasi::p2::StreamError,
    ) -> wasmtime::Result<io_streams::StreamError> {
        // Convert the stream error to the expected type
        match err {
            wasmtime_wasi::p2::StreamError::Closed => Ok(io_streams::StreamError::Closed),
            wasmtime_wasi::p2::StreamError::LastOperationFailed(e) => {
                Ok(io_streams::StreamError::LastOperationFailed(
                    self.inner.table().push(e).unwrap()
                ))
            }
            wasmtime_wasi::p2::StreamError::Trap(e) => {
                Err(e)
            }
        }
    }
}

// Minimal stream trait implementations that return explicit errors
// These satisfy Issue #66's requirement for trait implementations while deferring full functionality
impl io_streams::HostInputStream for VfsWasiAdapter {
    fn read(
        &mut self,
        _stream: Resource<io_streams::InputStream>,
        _len: u64,
    ) -> Result<Vec<u8>, wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    async fn blocking_read(
        &mut self,
        _stream: Resource<io_streams::InputStream>,
        _len: u64,
    ) -> Result<Vec<u8>, wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    fn skip(
        &mut self,
        _stream: Resource<io_streams::InputStream>,
        _len: u64,
    ) -> Result<u64, wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    async fn blocking_skip(
        &mut self,
        _stream: Resource<io_streams::InputStream>,
        _len: u64,
    ) -> Result<u64, wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    fn subscribe(
        &mut self,
        _stream: Resource<io_streams::InputStream>,
    ) -> Result<Resource<io_poll::Pollable>, anyhow::Error> {
        Err(anyhow::anyhow!("VFS stream subscribe not yet implemented (Issue #66)"))
    }

    async fn drop(&mut self, stream: Resource<io_streams::InputStream>) -> wasmtime::Result<()> {
        // Allow drop to succeed to prevent resource leaks
        use wasmtime_wasi::p2::IoView;
        self.inner.table().delete(stream)?;
        Ok(())
    }
}

impl io_streams::HostOutputStream for VfsWasiAdapter {
    fn write(
        &mut self,
        _stream: Resource<io_streams::OutputStream>,
        _contents: Vec<u8>,
    ) -> Result<(), wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    async fn blocking_write_and_flush(
        &mut self,
        _stream: Resource<io_streams::OutputStream>,
        _contents: Vec<u8>,
    ) -> Result<(), wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    fn flush(
        &mut self,
        _stream: Resource<io_streams::OutputStream>,
    ) -> Result<(), wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    async fn blocking_flush(
        &mut self,
        _stream: Resource<io_streams::OutputStream>,
    ) -> Result<(), wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    fn check_write(
        &mut self,
        _stream: Resource<io_streams::OutputStream>,
    ) -> Result<u64, wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    fn subscribe(
        &mut self,
        _stream: Resource<io_streams::OutputStream>,
    ) -> wasmtime::Result<Resource<wasmtime_wasi_io::poll::DynPollable>> {
        // Create a pollable that's always ready
        struct AlwaysReadyPollable;
        #[async_trait::async_trait]
        impl wasmtime_wasi_io::poll::Pollable for AlwaysReadyPollable {
            async fn ready(&mut self) {}
        }
        let pollable = AlwaysReadyPollable;
        let resource = self.inner.table().push(pollable)?;
        wasmtime_wasi_io::poll::subscribe(self.inner.table(), resource)
    }

    fn write_zeroes(
        &mut self,
        _stream: Resource<io_streams::OutputStream>,
        _len: u64,
    ) -> Result<(), wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    async fn blocking_write_zeroes_and_flush(
        &mut self,
        _stream: Resource<io_streams::OutputStream>,
        _len: u64,
    ) -> Result<(), wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    fn splice(
        &mut self,
        _dest: Resource<io_streams::OutputStream>,
        _src: Resource<io_streams::InputStream>,
        _len: u64,
    ) -> Result<u64, wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    async fn blocking_splice(
        &mut self,
        _dest: Resource<io_streams::OutputStream>,
        _src: Resource<io_streams::InputStream>,
        _len: u64,
    ) -> Result<u64, wasmtime_wasi::p2::StreamError> {
        // Return Closed to indicate stream is not available (less noisy than trap)
        Err(wasmtime_wasi::p2::StreamError::Closed)
    }

    async fn drop(&mut self, stream: Resource<io_streams::OutputStream>) -> wasmtime::Result<()> {
        // Allow drop to succeed to prevent resource leaks
        use wasmtime_wasi::p2::IoView;
        self.inner.table().delete(stream)?;
        Ok(())
    }
}
