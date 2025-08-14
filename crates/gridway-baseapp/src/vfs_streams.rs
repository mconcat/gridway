//! VFS Stream Implementations for WASI I/O
//!
//! This module implements WASI I/O stream interfaces backed by our VFS,
//! enabling WASM components to use standard stream operations for reading
//! and writing VFS files.

use crate::vfs::{VfsError, VirtualFilesystem};
use bytes::{Bytes, BytesMut};
use std::sync::{Arc, Mutex};
use wasmtime_wasi::p2::{InputStream, OutputStream, StreamError};

/// File handle containing VFS file descriptor and position
/// Made public for use in vfs_wasi_impl
#[derive(Clone)]
pub struct FileHandle {
    /// VFS file descriptor
    pub vfs_fd: u64,
    /// Current position in file
    pub position: u64,
    /// Reference to VFS
    pub vfs: Arc<Mutex<VirtualFilesystem>>,
    /// Whether file is open for writing
    pub writable: bool,
}

/// VFS-backed input stream implementation
pub struct VfsInputStream {
    /// File handle containing VFS file descriptor and position
    handle: Arc<Mutex<FileHandle>>,
    /// Buffer for read-ahead operations
    buffer: BytesMut,
    /// Whether we've reached end of file
    eof: bool,
}

impl VfsInputStream {
    /// Create a new VFS input stream
    pub fn new(handle: FileHandle) -> Self {
        Self {
            handle: Arc::new(Mutex::new(handle)),
            buffer: BytesMut::new(),
            eof: false,
        }
    }

    /// Fill the internal buffer from VFS if needed
    fn fill_buffer(&mut self) -> Result<(), VfsError> {
        if self.buffer.is_empty() && !self.eof {
            let mut handle = self.handle.lock().unwrap();
            // Read from VFS at current position
            let vfs = handle.vfs.lock().unwrap();
            let mut temp_buffer = vec![0u8; 4096]; // Read in 4KB chunks
            
            let bytes_read = vfs.read(handle.vfs_fd as u32, &mut temp_buffer)?;
            drop(vfs); // Release lock before updating position
            
            if bytes_read == 0 {
                self.eof = true;
            } else {
                self.buffer.extend_from_slice(&temp_buffer[..bytes_read]);
                handle.position += bytes_read as u64;
            }
        }
        Ok(())
    }
}

// Implement the InputStream trait from wasmtime-wasi
impl InputStream for VfsInputStream {
    fn read(&mut self, len: usize) -> Result<Bytes, StreamError> {
        // Fill buffer if needed
        self.fill_buffer()
            .map_err(|e| StreamError::Trap(anyhow::anyhow!("VFS read error: {}", e)))?;
        
        // Read from buffer
        let available = self.buffer.len();
        let to_read = std::cmp::min(len, available);
        
        if to_read == 0 {
            // No data available - return empty
            return Ok(Bytes::new());
        }
        
        // Split off the requested bytes
        let data = self.buffer.split_to(to_read);
        Ok(data.freeze())
    }
}

/// VFS-backed output stream implementation
pub struct VfsOutputStream {
    /// File handle containing VFS file descriptor and position
    handle: Arc<Mutex<FileHandle>>,
    /// Write buffer for batching small writes
    buffer: BytesMut,
    /// Maximum buffer size before automatic flush
    buffer_limit: usize,
}

impl VfsOutputStream {
    /// Create a new VFS output stream
    pub fn new(handle: FileHandle) -> Self {
        Self {
            handle: Arc::new(Mutex::new(handle)),
            buffer: BytesMut::new(),
            buffer_limit: 8192, // 8KB buffer by default
        }
    }

    /// Flush buffered data to VFS
    fn flush_buffer(&mut self) -> Result<(), VfsError> {
        if !self.buffer.is_empty() {
            let mut handle = self.handle.lock().unwrap();
            let vfs = handle.vfs.lock().unwrap();
            let bytes_written = vfs.write(handle.vfs_fd as u32, &self.buffer)?;
            drop(vfs); // Release lock before updating position
            
            if bytes_written != self.buffer.len() {
                return Err(VfsError::IoError(format!(
                    "Partial write: wrote {} of {} bytes",
                    bytes_written,
                    self.buffer.len()
                )));
            }
            
            handle.position += bytes_written as u64;
            self.buffer.clear();
        }
        Ok(())
    }
}

// Implement the OutputStream trait from wasmtime-wasi
impl OutputStream for VfsOutputStream {
    fn write(&mut self, data: Bytes) -> Result<(), StreamError> {
        // Add to buffer
        self.buffer.extend_from_slice(&data);
        
        // Auto-flush if buffer exceeds limit
        if self.buffer.len() >= self.buffer_limit {
            self.flush_buffer()
                .map_err(|e| StreamError::Trap(anyhow::anyhow!("VFS write error: {}", e)))?;
        }
        
        Ok(())
    }
    
    fn flush(&mut self) -> Result<(), StreamError> {
        self.flush_buffer()
            .map_err(|e| StreamError::Trap(anyhow::anyhow!("VFS flush error: {}", e)))
    }
    
    fn check_write(&mut self) -> Result<usize, StreamError> {
        // VFS can always accept writes up to buffer limit
        Ok(self.buffer_limit - self.buffer.len())
    }
}

/// Pollable implementation that is always ready
/// This is used for VFS streams since VFS operations are synchronous
pub struct AlwaysReadyPollable;

// AlwaysReadyPollable doesn't need to implement Pollable trait directly
// since it's managed by wasmtime-wasi's resource system