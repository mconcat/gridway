//! Simplified VFS Stream Implementations for WASI I/O
//!
//! This provides a basic implementation of streams backed by VFS file descriptors.
//! Since VFS operations are synchronous and operate on in-memory buffers,
//! streams are always ready for I/O operations (P0 requirement).

use crate::vfs::VirtualFilesystem;
use bytes::Bytes;
use std::io::SeekFrom;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use wasmtime_wasi::p2::StreamError;
use wasmtime_wasi_io::poll::Pollable;
use wasmtime_wasi_io::streams::{
    InputStream as WasiInputStream, OutputStream as WasiOutputStream, StreamResult,
};

/// File handle containing VFS file descriptor and position
/// Made public for use in vfs_wasi_impl
pub struct FileHandle {
    /// VFS file descriptor
    pub vfs_fd: u64,
    /// Current position in file (thread-safe)
    pub position: Arc<AtomicU64>,
    /// Reference to VFS
    pub vfs: Arc<Mutex<VirtualFilesystem>>,
    /// Whether file is open for writing
    pub writable: bool,
}

impl Clone for FileHandle {
    fn clone(&self) -> Self {
        Self {
            vfs_fd: self.vfs_fd,
            position: Arc::clone(&self.position),
            vfs: Arc::clone(&self.vfs),
            writable: self.writable,
        }
    }
}

/// Input stream backed by a VFS file descriptor
/// For P0, this operates synchronously on in-memory VFS data
pub struct VfsInputStream {
    /// The underlying file handle
    handle: FileHandle,
}

#[async_trait::async_trait]
impl WasiInputStream for VfsInputStream {
    fn read(&mut self, size: usize) -> StreamResult<Bytes> {
        match self.read_impl(size) {
            Ok(data) => Ok(Bytes::from(data)),
            Err(e) => Err(e),
        }
    }

    fn skip(&mut self, nelem: usize) -> StreamResult<usize> {
        self.skip_impl(nelem as u64).map(|n| n as usize)
    }
}

#[async_trait::async_trait]
impl Pollable for VfsInputStream {
    async fn ready(&mut self) {
        // Always ready for P0
    }
}

impl VfsInputStream {
    /// Create a new input stream from a file handle
    pub fn new(handle: FileHandle) -> Self {
        Self { handle }
    }

    /// Read data from the stream (implementation)
    pub fn read_impl(&mut self, len: usize) -> Result<Vec<u8>, StreamError> {
        let vfs = self.handle.vfs.lock().map_err(|_| StreamError::Closed)?;

        // Get current position atomically
        let current_pos = self.handle.position.load(Ordering::SeqCst);

        // Seek to current position
        if vfs
            .seek(self.handle.vfs_fd as u32, SeekFrom::Start(current_pos))
            .is_err()
        {
            return Err(StreamError::Closed);
        }

        // Read from current position
        let mut buffer = vec![0u8; len];
        match vfs.read(self.handle.vfs_fd as u32, &mut buffer) {
            Ok(bytes_read) => {
                // Update position atomically
                self.handle
                    .position
                    .fetch_add(bytes_read as u64, Ordering::SeqCst);
                // Truncate buffer to actual bytes read
                buffer.truncate(bytes_read);
                Ok(buffer)
            }
            Err(_) => Err(StreamError::Closed),
        }
    }

    /// Skip bytes in the stream (implementation)
    pub fn skip_impl(&mut self, len: u64) -> Result<u64, StreamError> {
        // Get file size to ensure we don't skip beyond EOF
        let vfs = self.handle.vfs.lock().map_err(|_| StreamError::Closed)?;

        // Get current position atomically
        let current_pos = self.handle.position.load(Ordering::SeqCst);

        // Get file size by seeking to end and back
        let original_pos = vfs
            .seek(self.handle.vfs_fd as u32, SeekFrom::Current(0))
            .map_err(|_| StreamError::Closed)?;
        let file_size = vfs
            .seek(self.handle.vfs_fd as u32, SeekFrom::End(0))
            .map_err(|_| StreamError::Closed)?;
        // Seek back to original position
        vfs.seek(self.handle.vfs_fd as u32, SeekFrom::Start(original_pos))
            .map_err(|_| StreamError::Closed)?;

        // Calculate actual skip amount (don't go beyond EOF)
        let remaining = file_size.saturating_sub(current_pos);
        let actual_skip = len.min(remaining);

        // Update position atomically
        self.handle
            .position
            .fetch_add(actual_skip, Ordering::SeqCst);
        Ok(actual_skip)
    }

    /// Get a subscribable pollable (always ready for P0)
    pub fn subscribe(&self) -> Box<dyn Pollable> {
        Box::new(AlwaysReadyPollable)
    }
}

/// Output stream backed by a VFS file descriptor
/// For P0, this operates synchronously on in-memory VFS data
pub struct VfsOutputStream {
    /// The underlying file handle
    handle: FileHandle,
}

#[async_trait::async_trait]
impl WasiOutputStream for VfsOutputStream {
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        self.write_impl(&bytes)
    }

    fn flush(&mut self) -> StreamResult<()> {
        self.flush_impl()
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        self.check_write_impl().map(|n| n as usize)
    }
}

#[async_trait::async_trait]
impl Pollable for VfsOutputStream {
    async fn ready(&mut self) {
        // Always ready for P0
    }
}

impl VfsOutputStream {
    /// Create a new output stream from a file handle
    pub fn new(handle: FileHandle) -> Self {
        Self { handle }
    }

    /// Write data to the stream (implementation)
    pub fn write_impl(&mut self, contents: &[u8]) -> Result<(), StreamError> {
        if !self.handle.writable {
            return Err(StreamError::Closed);
        }

        let vfs = self.handle.vfs.lock().map_err(|_| StreamError::Closed)?;

        // Get current position atomically
        let current_pos = self.handle.position.load(Ordering::SeqCst);

        // Seek to current position
        if vfs
            .seek(self.handle.vfs_fd as u32, SeekFrom::Start(current_pos))
            .is_err()
        {
            return Err(StreamError::Closed);
        }

        // Write at current position
        match vfs.write(self.handle.vfs_fd as u32, contents) {
            Ok(bytes_written) => {
                // Update position atomically
                self.handle
                    .position
                    .fetch_add(bytes_written as u64, Ordering::SeqCst);
                Ok(())
            }
            Err(_) => Err(StreamError::Closed),
        }
    }

    /// Flush the stream (no-op for synchronous VFS) (implementation)
    pub fn flush_impl(&mut self) -> Result<(), StreamError> {
        // VFS operations are synchronous, so flush is a no-op
        Ok(())
    }

    /// Check how much can be written (for P0, always return a large value) (implementation)
    pub fn check_write_impl(&self) -> Result<u64, StreamError> {
        if !self.handle.writable {
            return Err(StreamError::Closed);
        }
        // For P0 with in-memory buffers, we can always write a reasonable amount
        Ok(1024 * 1024) // 1MB
    }

    /// Write zeroes to the stream
    pub fn write_zeroes(&mut self, len: u64) -> Result<(), StreamError> {
        if !self.handle.writable {
            return Err(StreamError::Closed);
        }

        // Write zeroes in chunks to avoid large allocations
        const CHUNK_SIZE: usize = 64 * 1024; // 64KB chunks
        let chunk = vec![0u8; CHUNK_SIZE];
        let mut remaining = len as usize;

        while remaining > 0 {
            let write_size = remaining.min(CHUNK_SIZE);
            self.write_impl(&chunk[..write_size])?;
            remaining -= write_size;
        }

        Ok(())
    }

    /// Get a subscribable pollable (always ready for P0)
    pub fn subscribe(&self) -> Box<dyn Pollable> {
        Box::new(AlwaysReadyPollable)
    }
}

/// Pollable implementation that is always ready
/// This is used for VFS streams since VFS operations are synchronous
pub struct AlwaysReadyPollable;

#[async_trait::async_trait]
impl Pollable for AlwaysReadyPollable {
    async fn ready(&mut self) {
        // Always ready - VFS operations are synchronous
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::VirtualFilesystem;
    use gridway_store::MemStore;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn create_test_vfs() -> Arc<Mutex<VirtualFilesystem>> {
        let vfs = VirtualFilesystem::new();
        let store = Arc::new(Mutex::new(MemStore::new()));
        vfs.mount_store("test".to_string(), store).unwrap();

        // For tests, we'll grant capabilities to all the specific paths we'll use
        let test_paths = vec![
            "/test/input_test.txt",
            "/test/skip_test.txt",
            "/test/output_test.txt",
            "/test/zeroes_test.txt",
            "/test/check_write_test.txt",
            "/test/position_test.txt",
            "/test/empty.txt",
            "/test/small.txt",
            "/test/large_zeroes.txt",
            "/test/concurrent.txt",
        ];

        for path in test_paths {
            vfs.add_capability(crate::vfs::Capability::Read(PathBuf::from(path)))
                .unwrap();
            vfs.add_capability(crate::vfs::Capability::Write(PathBuf::from(path)))
                .unwrap();
        }

        Arc::new(Mutex::new(vfs))
    }

    #[test]
    fn test_input_stream_read() {
        let vfs = create_test_vfs();

        // Create a file with test content
        let test_content = b"Hello, VFS streams!";
        let path = PathBuf::from("/test/input_test.txt");
        {
            let vfs = vfs.lock().unwrap();
            let fd = vfs.create(&path).unwrap();
            vfs.write(fd, test_content).unwrap();
            vfs.close(fd).unwrap();
        }

        // Open file for reading
        let fd = {
            let vfs = vfs.lock().unwrap();
            vfs.open(&path, false).unwrap()
        };

        // Create input stream
        let handle = FileHandle {
            vfs_fd: fd as u64,
            position: Arc::new(AtomicU64::new(0)),
            vfs: vfs.clone(),
            writable: false,
        };
        let mut stream = VfsInputStream::new(handle);

        // Read from stream
        let data = stream.read_impl(10).unwrap();
        assert_eq!(&data, b"Hello, VFS");

        // Read more data - should continue from position
        let data = stream.read_impl(10).unwrap();
        assert_eq!(&data, b" streams!");

        // Clean up
        let vfs = vfs.lock().unwrap();
        vfs.close(fd).unwrap();
    }

    #[test]
    fn test_input_stream_skip() {
        let vfs = create_test_vfs();

        // Create a file with test content
        let test_content = b"Skip this part! Read this.";
        let path = PathBuf::from("/test/skip_test.txt");
        {
            let vfs = vfs.lock().unwrap();
            let fd = vfs.create(&path).unwrap();
            vfs.write(fd, test_content).unwrap();
            vfs.close(fd).unwrap();
        }

        // Open file for reading
        let fd = {
            let vfs = vfs.lock().unwrap();
            vfs.open(&path, false).unwrap()
        };

        // Create input stream
        let handle = FileHandle {
            vfs_fd: fd as u64,
            position: Arc::new(AtomicU64::new(0)),
            vfs: vfs.clone(),
            writable: false,
        };
        let mut stream = VfsInputStream::new(handle);

        // Skip 15 bytes (skips "Skip this part!")
        let skipped = stream.skip_impl(15).unwrap();
        assert_eq!(skipped, 15);

        // Read after skip - should read " Read this" (10 bytes including leading space)
        let data = stream.read_impl(10).unwrap();
        assert_eq!(&data, b" Read this");

        // Clean up
        let vfs = vfs.lock().unwrap();
        vfs.close(fd).unwrap();
    }

    #[test]
    fn test_output_stream_write() {
        let vfs = create_test_vfs();

        // Create a file for writing
        let path = PathBuf::from("/test/output_test.txt");
        let fd = {
            let vfs = vfs.lock().unwrap();
            vfs.create(&path).unwrap()
        };

        // Create output stream
        let handle = FileHandle {
            vfs_fd: fd as u64,
            position: Arc::new(AtomicU64::new(0)),
            vfs: vfs.clone(),
            writable: true,
        };
        let mut stream = VfsOutputStream::new(handle);

        // Write to stream
        stream.write_impl(b"First write ").unwrap();
        stream.write_impl(b"Second write").unwrap();

        // Verify content
        {
            let vfs = vfs.lock().unwrap();
            vfs.seek(fd, SeekFrom::Start(0)).unwrap();
            let mut buffer = vec![0u8; 100];
            let bytes_read = vfs.read(fd, &mut buffer).unwrap();
            buffer.truncate(bytes_read);
            assert_eq!(&buffer, b"First write Second write");
        }

        // Clean up
        let vfs = vfs.lock().unwrap();
        vfs.close(fd).unwrap();
    }

    #[test]
    fn test_output_stream_write_zeroes() {
        let vfs = create_test_vfs();

        // Create a file for writing
        let path = PathBuf::from("/test/zeroes_test.txt");
        let fd = {
            let vfs = vfs.lock().unwrap();
            vfs.create(&path).unwrap()
        };

        // Create output stream
        let handle = FileHandle {
            vfs_fd: fd as u64,
            position: Arc::new(AtomicU64::new(0)),
            vfs: vfs.clone(),
            writable: true,
        };
        let mut stream = VfsOutputStream::new(handle);

        // Write some data
        stream.write_impl(b"ABC").unwrap();

        // Write zeroes
        stream.write_zeroes(5).unwrap();

        // Write more data
        stream.write_impl(b"XYZ").unwrap();

        // Verify content
        {
            let vfs = vfs.lock().unwrap();
            vfs.seek(fd, SeekFrom::Start(0)).unwrap();
            let mut buffer = vec![0u8; 100];
            let bytes_read = vfs.read(fd, &mut buffer).unwrap();
            buffer.truncate(bytes_read);
            assert_eq!(&buffer, b"ABC\0\0\0\0\0XYZ");
        }

        // Clean up
        let vfs = vfs.lock().unwrap();
        vfs.close(fd).unwrap();
    }

    #[test]
    fn test_output_stream_check_write() {
        let vfs = create_test_vfs();

        // Create a file for writing
        let path = PathBuf::from("/test/check_write_test.txt");
        let fd = {
            let vfs = vfs.lock().unwrap();
            vfs.create(&path).unwrap()
        };

        // Create writable output stream
        let handle = FileHandle {
            vfs_fd: fd as u64,
            position: Arc::new(AtomicU64::new(0)),
            vfs: vfs.clone(),
            writable: true,
        };
        let stream = VfsOutputStream::new(handle);

        // Check write should return positive value for writable stream
        let available = stream.check_write_impl().unwrap();
        assert!(available > 0);

        // Create read-only stream
        let ro_handle = FileHandle {
            vfs_fd: fd as u64,
            position: Arc::new(AtomicU64::new(0)),
            vfs: vfs.clone(),
            writable: false,
        };
        let ro_stream = VfsOutputStream::new(ro_handle);

        // Check write should fail for read-only stream
        assert!(ro_stream.check_write_impl().is_err());

        // Clean up
        let vfs = vfs.lock().unwrap();
        vfs.close(fd).unwrap();
    }

    #[tokio::test]
    async fn test_always_ready_pollable() {
        // The pollable should always be ready immediately
        let mut pollable = AlwaysReadyPollable;

        // This should return immediately without blocking
        pollable.ready().await;

        // Can be called multiple times
        pollable.ready().await;
        pollable.ready().await;
    }

    #[test]
    fn test_stream_position_tracking() {
        let vfs = create_test_vfs();

        // Create a file with test content
        let test_content = b"0123456789ABCDEF";
        let path = PathBuf::from("/test/position_test.txt");
        {
            let vfs = vfs.lock().unwrap();
            let fd = vfs.create(&path).unwrap();
            vfs.write(fd, test_content).unwrap();
            vfs.close(fd).unwrap();
        }

        // Open file for reading
        let fd = {
            let vfs = vfs.lock().unwrap();
            vfs.open(&path, false).unwrap()
        };

        // Create input stream
        let handle = FileHandle {
            vfs_fd: fd as u64,
            position: Arc::new(AtomicU64::new(5)), // Start at position 5
            vfs: vfs.clone(),
            writable: false,
        };
        let mut stream = VfsInputStream::new(handle);

        // Read from position 5
        let data = stream.read_impl(4).unwrap();
        assert_eq!(&data, b"5678");

        // Position should have advanced
        let data = stream.read_impl(4).unwrap();
        assert_eq!(&data, b"9ABC");

        // Clean up
        let vfs = vfs.lock().unwrap();
        vfs.close(fd).unwrap();
    }

    #[test]
    fn test_edge_cases_empty_read() {
        let vfs = create_test_vfs();

        // Create an empty file
        let path = PathBuf::from("/test/empty.txt");
        {
            let vfs = vfs.lock().unwrap();
            let fd = vfs.create(&path).unwrap();
            vfs.close(fd).unwrap();
        }

        // Open file for reading
        let fd = {
            let vfs = vfs.lock().unwrap();
            vfs.open(&path, false).unwrap()
        };

        // Create input stream
        let handle = FileHandle {
            vfs_fd: fd as u64,
            position: Arc::new(AtomicU64::new(0)),
            vfs: vfs.clone(),
            writable: false,
        };
        let mut stream = VfsInputStream::new(handle);

        // Read from empty file should return empty
        let data = stream.read_impl(10).unwrap();
        assert_eq!(data.len(), 0);

        // Clean up
        let vfs = vfs.lock().unwrap();
        vfs.close(fd).unwrap();
    }

    #[test]
    fn test_skip_beyond_eof() {
        let vfs = create_test_vfs();

        // Create a small file
        let test_content = b"12345";
        let path = PathBuf::from("/test/small.txt");
        {
            let vfs = vfs.lock().unwrap();
            let fd = vfs.create(&path).unwrap();
            vfs.write(fd, test_content).unwrap();
            vfs.close(fd).unwrap();
        }

        // Open file for reading
        let fd = {
            let vfs = vfs.lock().unwrap();
            vfs.open(&path, false).unwrap()
        };

        // Create input stream
        let handle = FileHandle {
            vfs_fd: fd as u64,
            position: Arc::new(AtomicU64::new(0)),
            vfs: vfs.clone(),
            writable: false,
        };
        let mut stream = VfsInputStream::new(handle);

        // Skip beyond EOF should be limited
        let skipped = stream.skip_impl(100).unwrap();
        assert_eq!(skipped, 5); // Should only skip to EOF

        // Further skips should return 0
        let skipped = stream.skip_impl(10).unwrap();
        assert_eq!(skipped, 0);

        // Clean up
        let vfs = vfs.lock().unwrap();
        vfs.close(fd).unwrap();
    }

    #[test]
    fn test_large_write_zeroes() {
        let vfs = create_test_vfs();

        // Create a file for writing
        let path = PathBuf::from("/test/large_zeroes.txt");
        let fd = {
            let vfs = vfs.lock().unwrap();
            vfs.create(&path).unwrap()
        };

        // Create output stream
        let handle = FileHandle {
            vfs_fd: fd as u64,
            position: Arc::new(AtomicU64::new(0)),
            vfs: vfs.clone(),
            writable: true,
        };
        let mut stream = VfsOutputStream::new(handle);

        // Write a large number of zeroes (testing chunked implementation)
        let large_size = 256 * 1024; // 256KB
        stream.write_zeroes(large_size).unwrap();

        // Verify size
        {
            let vfs = vfs.lock().unwrap();
            // Get file size by seeking to end
            let file_size = vfs.seek(fd, SeekFrom::End(0)).unwrap();
            assert_eq!(file_size, large_size);

            // Verify content is all zeroes
            vfs.seek(fd, SeekFrom::Start(0)).unwrap();
            let mut buffer = vec![0xff; 1024]; // Initialize with non-zero
            let bytes_read = vfs.read(fd, &mut buffer).unwrap();
            assert_eq!(bytes_read, 1024);
            assert!(buffer.iter().all(|&b| b == 0));
        }

        // Clean up
        let vfs = vfs.lock().unwrap();
        vfs.close(fd).unwrap();
    }

    #[test]
    fn test_concurrent_position_updates() {
        use std::thread;

        let vfs = create_test_vfs();

        // Create a file with test content
        let test_content = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let path = PathBuf::from("/test/concurrent.txt");
        {
            let vfs = vfs.lock().unwrap();
            let fd = vfs.create(&path).unwrap();
            vfs.write(fd, test_content).unwrap();
            vfs.close(fd).unwrap();
        }

        // Open file for reading
        let fd = {
            let vfs = vfs.lock().unwrap();
            vfs.open(&path, false).unwrap()
        };

        // Create shared position
        let position = Arc::new(AtomicU64::new(0));

        // Spawn multiple threads reading from the same handle
        let mut handles = vec![];
        for _ in 0..3 {
            let handle = FileHandle {
                vfs_fd: fd as u64,
                position: Arc::clone(&position),
                vfs: vfs.clone(),
                writable: false,
            };

            let h = thread::spawn(move || {
                let mut stream = VfsInputStream::new(handle);
                let mut all_data = Vec::new();
                for _ in 0..3 {
                    let data = stream.read_impl(2).unwrap_or_default();
                    all_data.extend_from_slice(&data);
                }
                all_data
            });
            handles.push(h);
        }

        // Wait for all threads and collect results
        let results: Vec<Vec<u8>> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // Total bytes read should be consistent
        let total_bytes: usize = results.iter().map(|r| r.len()).sum();
        assert!(total_bytes <= test_content.len());

        // Final position should match total bytes read
        let final_pos = position.load(Ordering::SeqCst);
        assert_eq!(final_pos as usize, total_bytes);

        // Clean up
        let vfs = vfs.lock().unwrap();
        vfs.close(fd).unwrap();
    }
}
