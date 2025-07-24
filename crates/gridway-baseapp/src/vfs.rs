//! Virtual Filesystem (VFS) for WASI State Access
//!
//! This module provides a WASI-compatible virtual filesystem interface that maps
//! blockchain state stores to filesystem operations. WASM modules can access
//! blockchain state through standard file operations like read, write, seek, etc.
//!
//! The VFS provides path-based isolation where different modules can access
//! different namespaces: `/state/auth/`, `/state/bank/`, etc.

use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use gridway_store::{KVStore, StoreError};
use thiserror::Error;
use tracing::{debug, error, info};

/// VFS error types
#[derive(Error, Debug)]
pub enum VfsError {
    /// Path not found
    #[error("path not found:: {0}")]
    PathNotFound(String),

    /// Access denied due to capabilities
    #[error("access denied:: {0}")]
    AccessDenied(String),

    /// Invalid path format
    #[error("invalid path:: {0}")]
    InvalidPath(String),

    /// File descriptor not found
    #[error("file descriptor not found:: {0}")]
    FdNotFound(u32),

    /// Store operation failed
    #[error("store operation failed:: {0}")]
    StoreError(#[from] StoreError),

    /// IO operation failed
    #[error("IO operation failed:: {0}")]
    IoError(String),

    /// Serialization error
    #[error("serialization error:: {0}")]
    SerializationError(String),

    /// Invalid operation for file type
    #[error("invalid operation:: {0}")]
    InvalidOperation(String),

    /// File already exists
    #[error("file already exists:: {0}")]
    FileExists(String),

    /// Directory not empty
    #[error("directory not empty:: {0}")]
    DirectoryNotEmpty(String),
}

pub type Result<T> = std::result::Result<T, VfsError>;

/// File types in the VFS
#[derive(Debug, Clone, PartialEq)]
pub enum FileType {
    /// Regular file (key-value entry)
    File,
    /// Directory (namespace)
    Directory,
    /// Mounted interface
    Mount,
}

/// File metadata
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// File type (file or directory)
    pub file_type: FileType,
    /// File size in bytes
    pub size: u64,
    /// Last modification time
    pub modified: SystemTime,
    /// File path
    pub path: PathBuf,
}

/// File descriptor representing an open file in the VFS
#[derive(Debug)]
pub struct FileDescriptor {
    /// Unique file descriptor ID
    pub fd: u32,
    /// Virtual path in the filesystem
    pub path: PathBuf,
    /// Current seek position
    pub position: u64,
    /// File content (for read/write operations)
    pub content: Vec<u8>,
    /// Whether the file is open for writing
    pub writable: bool,
    /// Store namespace (e.g., "auth", "bank")
    pub namespace: String,
    /// Store key within the namespace
    pub key: Vec<u8>,
}

impl FileDescriptor {
    /// Create a new file descriptor
    pub fn new(fd: u32, path: PathBuf, namespace: String, key: Vec<u8>, writable: bool) -> Self {
        Self {
            fd,
            path,
            position: 0,
            content: Vec::new(),
            writable,
            namespace,
            key,
        }
    }

    /// Check if at end of file
    pub fn is_eof(&self) -> bool {
        self.position >= self.content.len() as u64
    }
}

/// Capability types for VFS access control
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Read access to a path
    Read(PathBuf),
    /// Write access to a path
    Write(PathBuf),
    /// Execute access to a path
    Execute(PathBuf),
}

/// Represents a mounted interface in the VFS
#[derive(Clone)]
pub enum Mount {
    /// A direct key-value store mount
    Store(Arc<Mutex<dyn KVStore>>),
    /// An interface that can be interacted with
    Interface(Arc<Mutex<dyn VfsInterface>>),
}

pub trait VfsInterface: Send + Sync {
    fn read(&self, path: &Path, buffer: &mut [u8]) -> Result<usize>;
    fn write(&self, path: &Path, data: &[u8]) -> Result<usize>;
}

// Type aliases to simplify complex types
type StoreMap = HashMap<String, Arc<Mutex<dyn KVStore>>>;
type MountMap = HashMap<PathBuf, Mount>;
type FileDescriptorMap = HashMap<u32, FileDescriptor>;

/// Virtual Filesystem for WASI State Access
///
/// The VFS maps blockchain state stores to a filesystem-like interface where:
/// - `/{module}/{key}` maps to a key-value pair in the module's store
/// - Directories represent namespaces/prefixes
/// - Special paths can be mounted to provide access to other interfaces (e.g., IBC)
pub struct VirtualFilesystem {
    /// Mapping from namespace to store
    stores: Arc<Mutex<StoreMap>>,
    /// Mounted interfaces
    mounts: Arc<Mutex<MountMap>>,
    /// Open file descriptors
    file_descriptors: Arc<Mutex<FileDescriptorMap>>,
    /// Next available file descriptor ID
    next_fd: Arc<Mutex<u32>>,
    /// Capability-based access control
    capabilities: Arc<Mutex<Vec<Capability>>>,
}

impl VirtualFilesystem {
    /// Create a new virtual filesystem
    pub fn new() -> Self {
        Self {
            stores: Arc::new(Mutex::new(HashMap::new())),
            mounts: Arc::new(Mutex::new(HashMap::new())),
            file_descriptors: Arc::new(Mutex::new(HashMap::new())),
            next_fd: Arc::new(Mutex::new(3)), // Start after stdin, stdout, stderr
            capabilities: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Mount a store at a specific namespace
    pub fn mount_store(&self, namespace: String, store: Arc<Mutex<dyn KVStore>>) -> Result<()> {
        debug!("Mounting store for namespace:: {}", namespace);

        let mut stores = self
            .stores
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;
        stores.insert(namespace.clone(), store);

        info!("Successfully mounted store for namespace:: {}", namespace);
        Ok(())
    }

    /// Mount an interface at a specific path
    pub fn mount(&self, path: PathBuf, mount: Mount) -> Result<()> {
        debug!("Mounting interface at path:: {}", path.display());

        let mut mounts = self
            .mounts
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;
        mounts.insert(path.clone(), mount);

        info!(
            "Successfully mounted interface at path:: {}",
            path.display()
        );
        Ok(())
    }

    /// Add a capability for access control
    pub fn add_capability(&self, capability: Capability) -> Result<()> {
        debug!("Adding capability:: {:?}", capability);

        let mut capabilities = self
            .capabilities
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;
        capabilities.push(capability);

        Ok(())
    }

    /// Check if access is allowed for a given operation
    fn check_access(&self, path: &Path, operation: &str) -> Result<()> {
        let capabilities = self
            .capabilities
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;

        let required_cap = match operation {
            "read" => Capability::Read(path.to_path_buf()),
            "write" => Capability::Write(path.to_path_buf()),
            _ => {
                return Err(VfsError::InvalidOperation(format!(
                    "Unknown operation:: {operation}"
                )))
            }
        };

        if capabilities.contains(&required_cap) {
            Ok(())
        } else {
            Err(VfsError::AccessDenied(format!(
                "Missing capability for {} on {}",
                operation,
                path.display()
            )))
        }
    }

    /// Parse a virtual path into namespace and key components
    fn parse_path(&self, path: &Path) -> Result<(String, Vec<u8>)> {
        let mounts = self
            .mounts
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;

        if mounts.contains_key(path) {
            return Ok(("".to_string(), path.to_str().unwrap().as_bytes().to_vec()));
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| VfsError::InvalidPath("Path contains invalid UTF-8".to_string()))?;

        // Expected format: /{namespace}/{key...}
        let parts: Vec<&str> = path_str.trim_start_matches('/').split('/').collect();

        if parts.is_empty() || (parts.len() == 1 && parts[0].is_empty()) {
            return Err(VfsError::InvalidPath("Path cannot be empty".to_string()));
        }

        let namespace = parts[0].to_string();

        if parts.len() == 1 {
            // Directory path: /auth
            Ok((namespace, Vec::new()))
        } else {
            // File path: /auth/accounts/addr123
            let key_parts = &parts[1..];
            let key = key_parts.join("/").into_bytes();
            Ok((namespace, key))
        }
    }

    /// Get the next available file descriptor ID
    fn next_fd_id(&self) -> Result<u32> {
        let mut next_fd = self
            .next_fd
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;
        let fd = *next_fd;
        *next_fd += 1;
        Ok(fd)
    }

    /// Open a file for reading or writing
    pub fn open(&self, path: &Path, writable: bool) -> Result<u32> {
        debug!(
            "Opening file:: {} (writable:: {})",
            path.display(),
            writable
        );

        // Check access permissions
        if writable {
            self.check_access(path, "write")?;
        } else {
            self.check_access(path, "read")?;
        }

        // Check mounts first
        {
            let mounts = self
                .mounts
                .lock()
                .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;

            if let Some(_mount) = mounts.get(path) {
                let fd = self.next_fd_id()?;
                let file_desc =
                    FileDescriptor::new(fd, path.to_path_buf(), "".to_string(), vec![], writable);
                let mut fds = self
                    .file_descriptors
                    .lock()
                    .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;
                fds.insert(fd, file_desc);
                return Ok(fd);
            }
        } // mounts lock is dropped here

        let (namespace, key) = self.parse_path(path)?;

        // Get the store for this namespace
        let stores = self
            .stores
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;
        let store = stores
            .get(&namespace)
            .ok_or_else(|| VfsError::PathNotFound(format!("Namespace not found:: {namespace}")))?
            .clone();
        drop(stores);

        // Read current content if file exists
        let content = {
            let store = store
                .lock()
                .map_err(|e| VfsError::IoError(format!("Store lock poisoned:: {e}")))?;
            store.get(&key)?.unwrap_or_default()
        };

        // Create file descriptor
        let fd = self.next_fd_id()?;
        let mut file_desc = FileDescriptor::new(fd, path.to_path_buf(), namespace, key, writable);
        file_desc.content = content;

        let mut fds = self
            .file_descriptors
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;
        fds.insert(fd, file_desc);

        info!(
            "Successfully opened file:: {} with fd:: {}",
            path.display(),
            fd
        );
        Ok(fd)
    }

    /// Read data from a file descriptor
    pub fn read(&self, fd: u32, buffer: &mut [u8]) -> Result<usize> {
        debug!(
            "Reading from fd:: {} into buffer of size:: {}",
            fd,
            buffer.len()
        );

        let mut fds = self
            .file_descriptors
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;

        let file_desc = fds.get_mut(&fd).ok_or(VfsError::FdNotFound(fd))?;

        let mounts = self
            .mounts
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;

        if let Some(mount) = mounts.get(&file_desc.path) {
            return match mount {
                Mount::Interface(interface) => {
                    let interface = interface.lock().unwrap();
                    interface.read(&file_desc.path, buffer)
                }
                _ => Err(VfsError::InvalidOperation(
                    "Read not supported for this mount type".to_string(),
                )),
            };
        }

        if file_desc.key.is_empty() {
            // Reading from directory - return directory listing
            return self.read_directory(file_desc, buffer);
        }

        let start = file_desc.position as usize;
        let end = std::cmp::min(start + buffer.len(), file_desc.content.len());

        if start >= file_desc.content.len() {
            return Ok(0); // EOF
        }

        let bytes_read = end - start;
        buffer[..bytes_read].copy_from_slice(&file_desc.content[start..end]);
        file_desc.position += bytes_read as u64;

        debug!("Read {} bytes from fd:: {}", bytes_read, fd);
        Ok(bytes_read)
    }

    /// Read directory listing
    fn read_directory(&self, file_desc: &mut FileDescriptor, buffer: &mut [u8]) -> Result<usize> {
        // For directory reading, we need to list all keys with the namespace prefix
        let stores = self
            .stores
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;

        let store = stores
            .get(&file_desc.namespace)
            .ok_or_else(|| {
                VfsError::PathNotFound(format!("Namespace not found:: {}", file_desc.namespace))
            })?
            .clone();
        drop(stores);

        // Get all keys in this namespace
        let entries = {
            let store = store
                .lock()
                .map_err(|e| VfsError::IoError(format!("Store lock poisoned:: {e}")))?;

            let mut entries = Vec::new();
            let iter = store.prefix_iterator(&[]);

            for (key, _) in iter {
                if let Ok(key_str) = String::from_utf8(key) {
                    entries.push(key_str);
                }
            }
            entries
        };

        // Format entries as directory listing (simple newline-separated format)
        let listing = entries.join("\n");
        let listing_bytes = listing.as_bytes();

        let start = file_desc.position as usize;
        let end = std::cmp::min(start + buffer.len(), listing_bytes.len());

        if start >= listing_bytes.len() {
            return Ok(0); // EOF
        }

        let bytes_read = end - start;
        buffer[..bytes_read].copy_from_slice(&listing_bytes[start..end]);
        file_desc.position += bytes_read as u64;

        Ok(bytes_read)
    }

    /// Write data to a file descriptor
    pub fn write(&self, fd: u32, data: &[u8]) -> Result<usize> {
        debug!("Writing {} bytes to fd:: {}", data.len(), fd);

        let mut fds = self
            .file_descriptors
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;

        let file_desc = fds.get_mut(&fd).ok_or(VfsError::FdNotFound(fd))?;

        if !file_desc.writable {
            return Err(VfsError::AccessDenied(
                "File not open for writing".to_string(),
            ));
        }

        let mounts = self
            .mounts
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;

        if let Some(mount) = mounts.get(&file_desc.path) {
            return match mount {
                Mount::Interface(interface) => {
                    let interface = interface.lock().unwrap();
                    interface.write(&file_desc.path, data)
                }
                _ => Err(VfsError::InvalidOperation(
                    "Write not supported for this mount type".to_string(),
                )),
            };
        }

        if file_desc.key.is_empty() {
            return Err(VfsError::InvalidOperation(
                "Cannot write to directory".to_string(),
            ));
        }

        // Extend content if necessary
        let end_pos = file_desc.position as usize + data.len();
        if end_pos > file_desc.content.len() {
            file_desc.content.resize(end_pos, 0);
        }

        // Write data at current position
        let start = file_desc.position as usize;
        file_desc.content[start..start + data.len()].copy_from_slice(data);
        file_desc.position += data.len() as u64;

        debug!("Wrote {} bytes to fd:: {}", data.len(), fd);
        Ok(data.len())
    }

    /// Seek to a position in a file
    pub fn seek(&self, fd: u32, pos: SeekFrom) -> Result<u64> {
        debug!("Seeking in fd:: {} to position:: {:?}", fd, pos);

        let mut fds = self
            .file_descriptors
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;

        let file_desc = fds.get_mut(&fd).ok_or(VfsError::FdNotFound(fd))?;

        let new_pos = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::End(offset) => {
                let file_len = file_desc.content.len() as i64;
                (file_len + offset).max(0) as u64
            }
            SeekFrom::Current(offset) => {
                let current = file_desc.position as i64;
                (current + offset).max(0) as u64
            }
        };

        file_desc.position = new_pos;
        debug!("Seeked to position:: {} in fd:: {}", new_pos, fd);
        Ok(new_pos)
    }

    /// Get file information/metadata
    pub fn stat(&self, path: &Path) -> Result<FileInfo> {
        debug!("Getting stat for path:: {}", path.display());

        // Check read access
        self.check_access(path, "read")?;

        // Check mounts first, but drop lock before parse_path
        {
            let mounts = self
                .mounts
                .lock()
                .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;

            if mounts.contains_key(path) {
                return Ok(FileInfo {
                    file_type: FileType::Mount,
                    size: 0,
                    modified: SystemTime::now(),
                    path: path.to_path_buf(),
                });
            }
        } // mounts lock is dropped here

        let (namespace, key) = self.parse_path(path)?;

        let stores = self
            .stores
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;
        let store = stores
            .get(&namespace)
            .ok_or_else(|| VfsError::PathNotFound(format!("Namespace not found:: {namespace}")))?
            .clone();
        drop(stores);

        if key.is_empty() {
            // Directory stat
            Ok(FileInfo {
                file_type: FileType::Directory,
                size: 0,
                modified: SystemTime::now(),
                path: path.to_path_buf(),
            })
        } else {
            // File stat
            let store = store
                .lock()
                .map_err(|e| VfsError::IoError(format!("Store lock poisoned:: {e}")))?;

            if let Some(value) = store.get(&key)? {
                Ok(FileInfo {
                    file_type: FileType::File,
                    size: value.len() as u64,
                    modified: SystemTime::now(), // TODO: actual modification time
                    path: path.to_path_buf(),
                })
            } else {
                Err(VfsError::PathNotFound(path.to_string_lossy().to_string()))
            }
        }
    }

    /// Close a file descriptor and flush changes to store
    pub fn close(&self, fd: u32) -> Result<()> {
        debug!("Closing fd:: {}", fd);

        let mut fds = self
            .file_descriptors
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;

        let file_desc = fds.remove(&fd).ok_or(VfsError::FdNotFound(fd))?;

        // If file was writable and has content, write back to store
        if file_desc.writable && !file_desc.key.is_empty() {
            let stores = self
                .stores
                .lock()
                .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;
            let store = stores
                .get(&file_desc.namespace)
                .ok_or_else(|| {
                    VfsError::PathNotFound(format!("Namespace not found:: {}", file_desc.namespace))
                })?
                .clone();
            drop(stores);

            let mut store = store
                .lock()
                .map_err(|e| VfsError::IoError(format!("Store lock poisoned:: {e}")))?;
            store.set(&file_desc.key, &file_desc.content)?;
        }

        info!("Successfully closed fd:: {}", fd);
        Ok(())
    }

    /// Create a new file
    pub fn create(&self, path: &Path) -> Result<u32> {
        debug!("Creating file:: {}", path.display());

        // Check create access
        self.check_access(path, "write")?;

        let (namespace, key) = self.parse_path(path)?;

        if key.is_empty() {
            return Err(VfsError::InvalidOperation(
                "Cannot create directory as file".to_string(),
            ));
        }

        // Check if file already exists
        let stores = self
            .stores
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;
        let store = stores
            .get(&namespace)
            .ok_or_else(|| VfsError::PathNotFound(format!("Namespace not found:: {namespace}")))?
            .clone();
        drop(stores);

        {
            let store = store
                .lock()
                .map_err(|e| VfsError::IoError(format!("Store lock poisoned:: {e}")))?;
            if store.has(&key)? {
                return Err(VfsError::FileExists(path.to_string_lossy().to_string()));
            }
        }

        // Open file for writing (creates empty file)
        self.open(path, true)
    }

    /// Delete a file
    pub fn unlink(&self, path: &Path) -> Result<()> {
        debug!("Deleting file:: {}", path.display());

        // Check delete access
        self.check_access(path, "write")?;

        let (namespace, key) = self.parse_path(path)?;

        if key.is_empty() {
            return Err(VfsError::InvalidOperation(
                "Cannot delete directory".to_string(),
            ));
        }

        let stores = self
            .stores
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;
        let store = stores
            .get(&namespace)
            .ok_or_else(|| VfsError::PathNotFound(format!("Namespace not found:: {namespace}")))?
            .clone();
        drop(stores);

        let mut store = store
            .lock()
            .map_err(|e| VfsError::IoError(format!("Store lock poisoned:: {e}")))?;
        store.delete(&key)?;

        info!("Successfully deleted file:: {}", path.display());
        Ok(())
    }

    /// List all open file descriptors (for debugging)
    pub fn list_open_fds(&self) -> Result<Vec<u32>> {
        let fds = self
            .file_descriptors
            .lock()
            .map_err(|e| VfsError::IoError(format!("Lock poisoned:: {e}")))?;
        Ok(fds.keys().cloned().collect())
    }
}

impl Default for VirtualFilesystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridway_store::MemStore;
    use std::path::PathBuf;

    fn setup_test_vfs() -> VirtualFilesystem {
        let vfs = VirtualFilesystem::new();

        // Mount test stores
        let auth_store = Arc::new(Mutex::new(MemStore::new()));
        let bank_store = Arc::new(Mutex::new(MemStore::new()));

        vfs.mount_store("auth".to_string(), auth_store.clone())
            .unwrap();
        vfs.mount_store("bank".to_string(), bank_store.clone())
            .unwrap();

        // Add capabilities
        vfs.add_capability(Capability::Read(PathBuf::from("/auth")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/auth")))
            .unwrap();
        vfs.add_capability(Capability::Read(PathBuf::from("/bank")))
            .unwrap();
        vfs.add_capability(Capability::Write(PathBuf::from("/bank")))
            .unwrap();

        vfs
    }

    #[test]
    fn test_vfs_creation() {
        let vfs = VirtualFilesystem::new();
        let fds = vfs.list_open_fds().unwrap();
        assert!(fds.is_empty());
    }

    #[test]
    fn test_path_parsing() {
        let vfs = VirtualFilesystem::new();

        // Test valid paths
        let (ns, key) = vfs.parse_path(&PathBuf::from("/auth/")).unwrap();
        assert_eq!(ns, "auth");
        assert!(key.is_empty());

        let (ns, key) = vfs
            .parse_path(&PathBuf::from("/bank/accounts/addr123"))
            .unwrap();
        assert_eq!(ns, "bank");
        assert_eq!(key, b"accounts/addr123");

        // Test invalid paths
        assert!(vfs.parse_path(&PathBuf::from("/")).is_err());
        assert!(vfs.parse_path(&PathBuf::from("")).is_err());
    }

    #[test]
    fn test_file_operations() {
        let vfs = setup_test_vfs();
        let path = PathBuf::from("/auth/accounts/test_account");

        // Grant access to the specific path
        vfs.add_capability(Capability::Write(path.clone())).unwrap();
        vfs.add_capability(Capability::Read(path.clone())).unwrap();

        // Create and write to file
        let fd = vfs.create(&path).unwrap();

        let data = b"test account data";
        let written = vfs.write(fd, data).unwrap();
        assert_eq!(written, data.len());

        // Close file to flush to store
        vfs.close(fd).unwrap();

        // Open and read file
        let fd = vfs.open(&path, false).unwrap();
        let mut buffer = vec![0u8; 20];
        let read = vfs.read(fd, &mut buffer).unwrap();
        assert_eq!(read, data.len());
        assert_eq!(&buffer[..read], data);

        vfs.close(fd).unwrap();
    }

    #[test]
    fn test_directory_listing() {
        let vfs = setup_test_vfs();

        // Create some files
        let files = [
            "/auth/accounts/addr1",
            "/auth/accounts/addr2",
            "/auth/validators/val1",
        ];

        for file_path in &files {
            let path = PathBuf::from(file_path);
            vfs.add_capability(Capability::Write(path.clone())).unwrap();
            let fd = vfs.create(&path).unwrap();
            vfs.write(fd, b"test data").unwrap();
            vfs.close(fd).unwrap();
        }

        // List directory
        let dir_path = PathBuf::from("/auth/");
        vfs.add_capability(Capability::Read(dir_path.clone()))
            .unwrap();
        let fd = vfs.open(&dir_path, false).unwrap();
        let mut buffer = vec![0u8; 1024];
        let read = vfs.read(fd, &mut buffer).unwrap();

        let listing = String::from_utf8_lossy(&buffer[..read]);
        assert!(listing.contains("accounts/addr1"));
        assert!(listing.contains("accounts/addr2"));
        assert!(listing.contains("validators/val1"));

        vfs.close(fd).unwrap();
    }

    #[test]
    fn test_seek_operations() {
        let vfs = setup_test_vfs();

        let path = PathBuf::from("/bank/balances/test");
        vfs.add_capability(Capability::Write(path.clone())).unwrap();
        vfs.add_capability(Capability::Read(path.clone())).unwrap();
        let fd = vfs.create(&path).unwrap();

        // Write test data
        let data = b"0123456789";
        vfs.write(fd, data).unwrap();

        // Seek to beginning
        let pos = vfs.seek(fd, SeekFrom::Start(0)).unwrap();
        assert_eq!(pos, 0);

        // Seek to middle
        let pos = vfs.seek(fd, SeekFrom::Start(5)).unwrap();
        assert_eq!(pos, 5);

        // Read from middle
        let mut buffer = vec![0u8; 3];
        let read = vfs.read(fd, &mut buffer).unwrap();
        assert_eq!(read, 3);
        assert_eq!(&buffer, b"567");

        vfs.close(fd).unwrap();
    }

    #[test]
    fn test_file_stat() {
        let vfs = setup_test_vfs();

        let path = PathBuf::from("/auth/test_file");
        vfs.add_capability(Capability::Write(path.clone())).unwrap();
        vfs.add_capability(Capability::Read(path.clone())).unwrap();
        let fd = vfs.create(&path).unwrap();
        vfs.write(fd, b"test content").unwrap();
        vfs.close(fd).unwrap();

        let file_info = vfs.stat(&path).unwrap();
        assert_eq!(file_info.file_type, FileType::File);
        assert_eq!(file_info.size, 12); // "test content".len()

        // Test directory stat
        let dir_path = PathBuf::from("/auth/");
        vfs.add_capability(Capability::Read(dir_path.clone()))
            .unwrap();
        let dir_info = vfs.stat(&dir_path).unwrap();
        assert_eq!(dir_info.file_type, FileType::Directory);
    }

    #[test]
    fn test_mount() {
        let vfs = setup_test_vfs();

        struct MockInterface;
        impl VfsInterface for MockInterface {
            fn read(&self, _path: &Path, buffer: &mut [u8]) -> Result<usize> {
                let data = b"hello from mock";
                buffer[..data.len()].copy_from_slice(data);
                Ok(data.len())
            }

            fn write(&self, _path: &Path, data: &[u8]) -> Result<usize> {
                assert_eq!(data, b"hello to mock");
                Ok(data.len())
            }
        }

        let mount_path = PathBuf::from("/ibc/connections/conn-1");
        let interface = Arc::new(Mutex::new(MockInterface));
        vfs.mount(mount_path.clone(), Mount::Interface(interface))
            .unwrap();

        vfs.add_capability(Capability::Read(mount_path.clone()))
            .unwrap();
        vfs.add_capability(Capability::Write(mount_path.clone()))
            .unwrap();

        let fd = vfs.open(&mount_path, true).unwrap();

        let mut buffer = [0u8; 20];
        let bytes_read = vfs.read(fd, &mut buffer).unwrap();
        assert_eq!(bytes_read, 15);
        assert_eq!(&buffer[..bytes_read], b"hello from mock");

        let bytes_written = vfs.write(fd, b"hello to mock").unwrap();
        assert_eq!(bytes_written, 13);

        vfs.close(fd).unwrap();
    }
}
