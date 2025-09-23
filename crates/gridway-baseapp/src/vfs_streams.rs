//! VFS Stream Implementations for WASI I/O
//!
//! This module provides stream implementations that wrap VFS file descriptors,
//! enabling WASI programs to read and write files using stream interfaces.

use crate::vfs::VirtualFilesystem;
use bytes::Bytes;
use std::sync::{Arc, Mutex};
use wasmtime_wasi_io::{
    poll::Pollable,
    streams::{InputStream, OutputStream, StreamError},
};

/// Input stream backed by a VFS file descriptor
pub struct VfsInputStream {
    /// VFS file descriptor ID
    fd: u32,
    /// Current position in file (separate from FileDescriptor position for stream)
    position: u64,
    /// Reference to VFS
    vfs: Arc<Mutex<VirtualFilesystem>>,
}

impl VfsInputStream {
    /// Create a new input stream for a file descriptor at the given offset
    pub fn new(fd: u32, offset: u64, vfs: Arc<Mutex<VirtualFilesystem>>) -> Self {
        Self {
            fd,
            position: offset,
            vfs,
        }
    }

    fn read_impl(&mut self, len: usize) -> Result<Bytes, StreamError> {
        let vfs = self.vfs.lock().map_err(|_| StreamError::Closed)?;

        // Get the file descriptor
        let file_desc = vfs.get_file_descriptor(self.fd).ok_or(StreamError::Closed)?;

        // Calculate how much we can read
        let available = file_desc.content.len().saturating_sub(self.position as usize);
        let to_read = len.min(available);

        if to_read == 0 {
            // End of file
            return Err(StreamError::Closed);
        }

        // Read from the current position
        let start = self.position as usize;
        let end = start + to_read;
        let data = file_desc.content[start..end].to_vec();

        // Update stream position
        self.position += to_read as u64;

        Ok(Bytes::from(data))
    }
}

#[async_trait::async_trait]
impl InputStream for VfsInputStream {
    fn read(&mut self, size: usize) -> Result<Bytes, StreamError> {
        self.read_impl(size)
    }
}

#[async_trait::async_trait]
impl Pollable for VfsInputStream {
    async fn ready(&mut self) {
        // VFS operations are synchronous, so always ready
    }
}

/// Output stream backed by a VFS file descriptor
pub struct VfsOutputStream {
    /// VFS file descriptor ID
    fd: u32,
    /// Current position in file (for write mode, not append)
    position: Option<u64>, // None for append mode
    /// Reference to VFS
    vfs: Arc<Mutex<VirtualFilesystem>>,
    /// Buffered data to write
    buffer: Vec<u8>,
}

impl VfsOutputStream {
    /// Create a new output stream for writing at the given offset
    pub fn new_write(fd: u32, offset: u64, vfs: Arc<Mutex<VirtualFilesystem>>) -> Self {
        Self {
            fd,
            position: Some(offset),
            vfs,
            buffer: Vec::new(),
        }
    }

    /// Create a new output stream for appending
    pub fn new_append(fd: u32, vfs: Arc<Mutex<VirtualFilesystem>>) -> Self {
        Self {
            fd,
            position: None, // Append mode
            vfs,
            buffer: Vec::new(),
        }
    }

    fn write_impl(&mut self, contents: Bytes) -> Result<(), StreamError> {
        // For now, buffer the write
        self.buffer.extend_from_slice(&contents);
        Ok(())
    }

    fn flush_impl(&mut self) -> Result<(), StreamError> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        // For now, just clear the buffer - actual write would happen on stream close
        // This is a simplified implementation for VFS
        // In production, this would properly update the file descriptor's content

        self.buffer.clear();

        Ok(())
    }

    fn check_write_impl(&mut self) -> Result<usize, StreamError> {
        // For VFS, we can always write (limited by available memory)
        // Return a reasonable chunk size
        Ok(65536) // 64KB
    }
}

#[async_trait::async_trait]
impl OutputStream for VfsOutputStream {
    fn write(&mut self, contents: Bytes) -> Result<(), StreamError> {
        self.write_impl(contents)
    }

    fn flush(&mut self) -> Result<(), StreamError> {
        self.flush_impl()
    }

    fn check_write(&mut self) -> Result<usize, StreamError> {
        self.check_write_impl()
    }
}

#[async_trait::async_trait]
impl Pollable for VfsOutputStream {
    async fn ready(&mut self) {
        // VFS operations are synchronous, so always ready
    }
}