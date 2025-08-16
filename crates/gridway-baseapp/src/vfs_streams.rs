//! VFS Stream Implementations for WASI I/O
//!
//! This module provides VFS-backed implementations of InputStream and OutputStream
//! for WASI Preview 2 support. These types handle file I/O operations with internal
//! offset tracking, enabling proper stream semantics for VFS file access.

use crate::vfs::{VfsError, VirtualFilesystem};
use std::sync::{Mutex, Weak};
use wasmtime_wasi::p2::StreamError;

/// VFS-backed input stream for reading from VFS files
///
/// This struct implements reading from VFS files with internal offset tracking.
/// It holds a weak reference to the VFS context to avoid circular references.
pub struct VfsInputStream {
    /// File descriptor in the VFS
    fd: u32,
    /// Current read offset in the file
    offset: u64,
    /// Weak reference to the VFS context
    ctx: Weak<Mutex<VirtualFilesystem>>,
}

impl VfsInputStream {
    /// Create a new VFS input stream
    pub fn new(fd: u32, offset: u64, ctx: Weak<Mutex<VirtualFilesystem>>) -> Self {
        Self { fd, offset, ctx }
    }

    /// Read bytes from the stream
    pub fn read(&mut self, len: u64) -> Result<Vec<u8>, StreamError> {
        let ctx = self.ctx.upgrade().ok_or(StreamError::Closed)?;

        let vfs = ctx.lock().map_err(|_| StreamError::Closed)?;

        // Create a buffer for reading
        let mut buffer = vec![0u8; len.min(usize::MAX as u64) as usize];

        // Read from the VFS at the current offset
        let bytes_read = vfs.read(self.fd, &mut buffer).map_err(|e| match e {
            VfsError::FdNotFound(_) => StreamError::Closed,
            _ => StreamError::LastOperationFailed(anyhow::anyhow!("{}", e)),
        })?;

        // Truncate buffer to actual bytes read
        buffer.truncate(bytes_read);

        // Update offset
        self.offset += bytes_read as u64;

        Ok(buffer)
    }

    /// Skip bytes in the stream
    pub fn skip(&mut self, len: u64) -> Result<u64, StreamError> {
        let ctx = self.ctx.upgrade().ok_or(StreamError::Closed)?;

        let vfs = ctx.lock().map_err(|_| StreamError::Closed)?;

        // Seek forward from current position
        use std::io::SeekFrom;
        let new_offset = vfs
            .seek(self.fd, SeekFrom::Current(len as i64))
            .map_err(|e| match e {
                VfsError::FdNotFound(_) => StreamError::Closed,
                _ => StreamError::LastOperationFailed(anyhow::anyhow!("{}", e)),
            })?;

        let bytes_skipped = new_offset.saturating_sub(self.offset);
        self.offset = new_offset;

        Ok(bytes_skipped)
    }

    /// Check if we've reached end of file
    pub fn is_eof(&self) -> Result<bool, StreamError> {
        let ctx = self.ctx.upgrade().ok_or(StreamError::Closed)?;

        let vfs = ctx.lock().map_err(|_| StreamError::Closed)?;

        // Check if current offset is at or beyond file size
        let file_size = vfs.get_size(self.fd).map_err(|e| match e {
            VfsError::FdNotFound(_) => StreamError::Closed,
            _ => StreamError::LastOperationFailed(anyhow::anyhow!("{}", e)),
        })?;

        Ok(self.offset >= file_size)
    }

    /// Get the current offset
    pub fn offset(&self) -> u64 {
        self.offset
    }
}

/// VFS-backed output stream for writing to VFS files
///
/// This struct implements writing to VFS files with internal offset tracking.
/// It supports both regular write and append modes.
pub struct VfsOutputStream {
    /// File descriptor in the VFS
    fd: u32,
    /// Current write offset in the file
    offset: u64,
    /// Weak reference to the VFS context
    ctx: Weak<Mutex<VirtualFilesystem>>,
    /// Whether to append (always write at end)
    append: bool,
}

impl VfsOutputStream {
    /// Create a new VFS output stream
    pub fn new(fd: u32, offset: u64, ctx: Weak<Mutex<VirtualFilesystem>>, append: bool) -> Self {
        Self {
            fd,
            offset,
            ctx,
            append,
        }
    }

    /// Write bytes to the stream
    pub fn write(&mut self, data: &[u8]) -> Result<(), StreamError> {
        let ctx = self.ctx.upgrade().ok_or(StreamError::Closed)?;

        let vfs = ctx.lock().map_err(|_| StreamError::Closed)?;

        // If in append mode, seek to end first
        if self.append {
            let file_size = vfs.get_size(self.fd).map_err(|e| match e {
                VfsError::FdNotFound(_) => StreamError::Closed,
                _ => StreamError::LastOperationFailed(anyhow::anyhow!("{}", e)),
            })?;
            self.offset = file_size;
        }

        // Write data to VFS
        let bytes_written = vfs.write(self.fd, data).map_err(|e| match e {
            VfsError::FdNotFound(_) => StreamError::Closed,
            VfsError::AccessDenied(_) => {
                StreamError::LastOperationFailed(anyhow::anyhow!("Write access denied"))
            }
            _ => StreamError::LastOperationFailed(anyhow::anyhow!("{}", e)),
        })?;

        // Update offset
        self.offset += bytes_written as u64;

        Ok(())
    }

    /// Write zeroes to the stream
    pub fn write_zeroes(&mut self, len: u64) -> Result<(), StreamError> {
        // Create a buffer of zeroes and write it
        let zeroes = vec![0u8; len.min(8192) as usize]; // Cap at 8KB chunks
        let mut remaining = len;

        while remaining > 0 {
            let chunk_size = remaining.min(8192);
            self.write(&zeroes[..chunk_size as usize])?;
            remaining -= chunk_size;
        }

        Ok(())
    }

    /// Flush any buffered data
    pub fn flush(&mut self) -> Result<(), StreamError> {
        // VFS operations are synchronous, so flush is a no-op
        // We just verify the context is still valid
        self.ctx.upgrade().ok_or(StreamError::Closed)?;
        Ok(())
    }

    /// Check how many bytes can be written
    pub fn check_write(&self) -> Result<u64, StreamError> {
        // VFS doesn't have a write buffer limit, so we return a large value
        // This matches the behavior of unbuffered streams
        Ok(u64::MAX)
    }

    /// Get the current offset
    pub fn offset(&self) -> u64 {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridway_store::MemStore;
    use std::path::Path;
    use std::sync::Arc;

    fn setup_test_vfs() -> Arc<Mutex<VirtualFilesystem>> {
        let vfs = VirtualFilesystem::new();
        let store = Arc::new(Mutex::new(MemStore::new()));
        vfs.mount_store("test".to_string(), store).unwrap();

        // Add capabilities for test paths
        use crate::vfs::Capability;
        use std::path::PathBuf;
        vfs.add_capability(Capability::Read(PathBuf::from("/test")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/test")))
            .unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/test/file.txt")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/test/file.txt")))
            .unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/test/output.txt")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/test/output.txt")))
            .unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/test/append.txt")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/test/append.txt")))
            .unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/test/small.txt")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/test/small.txt")))
            .unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/test/closed.txt")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/test/closed.txt")))
            .unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/test/temp.txt")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/test/temp.txt")))
            .unwrap();

        Arc::new(Mutex::new(vfs))
    }

    #[test]
    fn test_input_stream_offset_tracking() {
        let vfs_arc = setup_test_vfs();
        let weak_vfs = Arc::downgrade(&vfs_arc);

        // Create a test file with some content
        let test_data = b"Hello, WASI streams!";
        {
            let vfs = vfs_arc.lock().unwrap();
            let fd = vfs.open(Path::new("/test/file.txt"), true).unwrap();
            vfs.write(fd, test_data).unwrap();
            vfs.close(fd).unwrap();
        }

        // Open file for reading
        let fd = {
            let vfs = vfs_arc.lock().unwrap();
            vfs.open(Path::new("/test/file.txt"), false).unwrap()
        };

        // Create input stream
        let mut stream = VfsInputStream::new(fd, 0, weak_vfs);

        // Test initial offset
        assert_eq!(stream.offset(), 0);

        // Read 5 bytes
        let data = stream.read(5).unwrap();
        assert_eq!(data, b"Hello");
        assert_eq!(stream.offset(), 5);

        // Skip 2 bytes
        let skipped = stream.skip(2).unwrap();
        assert_eq!(skipped, 2);
        assert_eq!(stream.offset(), 7);

        // Read more
        let data = stream.read(4).unwrap();
        assert_eq!(data, b"WASI");
        assert_eq!(stream.offset(), 11);
    }

    #[test]
    fn test_output_stream_offset_tracking() {
        let vfs_arc = setup_test_vfs();
        let weak_vfs = Arc::downgrade(&vfs_arc);

        // Open file for writing
        let fd = {
            let vfs = vfs_arc.lock().unwrap();
            vfs.open(Path::new("/test/output.txt"), true).unwrap()
        };

        // Create output stream
        let mut stream = VfsOutputStream::new(fd, 0, weak_vfs, false);

        // Test initial offset
        assert_eq!(stream.offset(), 0);

        // Write some data
        stream.write(b"Hello").unwrap();
        assert_eq!(stream.offset(), 5);

        // Write more data
        stream.write(b" World").unwrap();
        assert_eq!(stream.offset(), 11);

        // Write zeroes
        stream.write_zeroes(5).unwrap();
        assert_eq!(stream.offset(), 16);

        // Verify content
        {
            let vfs = vfs_arc.lock().unwrap();
            // Seek to beginning before reading
            use std::io::SeekFrom;
            vfs.seek(fd, SeekFrom::Start(0)).unwrap();
            let mut buffer = vec![0u8; 16];
            vfs.read(fd, &mut buffer).unwrap();
            assert_eq!(&buffer[..11], b"Hello World");
            assert_eq!(&buffer[11..], &[0u8; 5]);
        }
    }

    #[test]
    fn test_append_mode() {
        let vfs_arc = setup_test_vfs();
        let weak_vfs = Arc::downgrade(&vfs_arc);

        // Create a file with initial content
        let fd = {
            let vfs = vfs_arc.lock().unwrap();
            let fd = vfs.open(Path::new("/test/append.txt"), true).unwrap();
            vfs.write(fd, b"Initial").unwrap();
            fd
        };

        // Create append stream
        let mut stream = VfsOutputStream::new(fd, 0, weak_vfs, true);

        // Write should append regardless of initial offset
        stream.write(b" Content").unwrap();

        // Verify content
        {
            let vfs = vfs_arc.lock().unwrap();
            use std::io::SeekFrom;
            vfs.seek(fd, SeekFrom::Start(0)).unwrap(); // Seek to beginning
            let mut buffer = vec![0u8; 100];
            let bytes_read = vfs.read(fd, &mut buffer).unwrap();
            buffer.truncate(bytes_read);
            assert_eq!(buffer, b"Initial Content");
        }
    }

    #[test]
    fn test_eof_detection() {
        let vfs_arc = setup_test_vfs();
        let weak_vfs = Arc::downgrade(&vfs_arc);

        // Create a small file
        let fd = {
            let vfs = vfs_arc.lock().unwrap();
            let fd = vfs.open(Path::new("/test/small.txt"), true).unwrap();
            vfs.write(fd, b"Short").unwrap();
            use std::io::SeekFrom;
            vfs.seek(fd, SeekFrom::Start(0)).unwrap(); // Reset to beginning
            fd
        };

        // Create input stream
        let mut stream = VfsInputStream::new(fd, 0, weak_vfs);

        // Initially not at EOF
        assert!(!stream.is_eof().unwrap());

        // Read all content
        stream.read(5).unwrap();

        // Now at EOF
        assert!(stream.is_eof().unwrap());

        // Reading at EOF returns empty
        let data = stream.read(10).unwrap();
        assert!(data.is_empty());
    }

    #[test]
    fn test_stream_error_on_closed_fd() {
        let vfs_arc = setup_test_vfs();
        let weak_vfs = Arc::downgrade(&vfs_arc);

        // Create and close a file
        let fd = {
            let vfs = vfs_arc.lock().unwrap();
            let fd = vfs.open(Path::new("/test/closed.txt"), true).unwrap();
            vfs.close(fd).unwrap();
            fd
        };

        // Try to use closed fd
        let mut stream = VfsInputStream::new(fd, 0, weak_vfs);
        let result = stream.read(10);

        // Should get Closed error
        assert!(matches!(result, Err(StreamError::Closed)));
    }

    #[test]
    fn test_weak_reference_invalidation() {
        let vfs_arc = setup_test_vfs();
        let weak_vfs = Arc::downgrade(&vfs_arc);

        let fd = {
            let vfs = vfs_arc.lock().unwrap();
            vfs.open(Path::new("/test/temp.txt"), true).unwrap()
        };

        let mut stream = VfsInputStream::new(fd, 0, weak_vfs);

        // Drop the strong reference
        drop(vfs_arc);

        // Stream operations should fail with Closed
        let result = stream.read(10);
        assert!(matches!(result, Err(StreamError::Closed)));
    }
}
