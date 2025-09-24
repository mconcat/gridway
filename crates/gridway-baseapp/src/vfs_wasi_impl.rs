//! VFS WASI Implementation
//!
//! This module implements WASI filesystem interfaces directly, routing all filesystem
//! operations to our Merkle-backed VFS. The key insight is to NOT use wasmtime-wasi's
//! filesystem implementation at all, but provide our own implementation of the WASI
//! filesystem interfaces.

use crate::vfs::{Capability, VfsError, VirtualFilesystem};
use crate::vfs_streams::{VfsInputStream, VfsOutputStream};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
#[derive(Clone)]
pub(crate) enum DescriptorKind {
    /// Directory descriptor
    Dir {
        mount_id: usize,
        path: PathBuf, // Track the actual directory path
    },
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

impl DirectoryStream {
    /// Create a new directory stream for the given path
    fn new(path: PathBuf, mount_id: usize) -> Self {
        Self {
            path,
            mount_id,
            entries: Vec::new(),
            position: 0,
        }
    }

    /// Add an entry to the stream
    fn add_entry(&mut self, name: String, is_directory: bool) {
        let entry = fs_types::DirectoryEntry {
            type_: if is_directory {
                fs_types::DescriptorType::Directory
            } else {
                fs_types::DescriptorType::RegularFile
            },
            name,
        };
        self.entries.push(entry);
    }
}

// FileHandle type is imported from vfs_streams_simple module
use crate::vfs_streams_simple::FileHandle;

/// Our VFS-backed filesystem implementation
///
/// ID Space Management:
/// - Descriptor IDs: Start at 10, incremented for each file/directory descriptor
/// - Stream IDs: Start at 1000000, incremented for each directory stream
/// - Input stream IDs: Start at 2000000, incremented for each input stream
/// - Output stream IDs: Start at 3000000, incremented for each output stream
/// These ID spaces are intentionally disjoint to avoid collisions between
/// different resource types. Stream IDs use high starting values to ensure
/// they never overlap with descriptor IDs even with heavy usage.
pub struct VfsFilesystem {
    /// Resource table for managing WASI resources
    table: ResourceTable,
    /// The underlying VFS
    vfs: Arc<Mutex<VirtualFilesystem>>,
    /// Configured mounts
    mounts: Vec<Mount>,
    /// Descriptor mapping (ID space: 10+)
    pub descriptors: HashMap<u32, DescriptorKind>,
    /// Next descriptor ID
    next_descriptor: u32,
    /// Directory streams mapping (ID space: 1000000+)
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
            next_stream_id: 1_000_000, // High starting value to avoid ID collisions
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
            VfsError::DirectoryNotEmpty(_) => fs_types::ErrorCode::NotEmpty,
        }
    }

    /// Check if a path represents a directory by looking for entries with that prefix
    fn is_directory(&self, namespace: &str, path: &[u8]) -> bool {
        let Ok(vfs) = self.vfs.lock() else {
            // Lock poisoned, treat as not a directory
            return false;
        };
        if let Some(store) = vfs.get_store(namespace) {
            let Ok(store) = store.lock() else {
                // Lock poisoned, treat as not a directory
                return false;
            };
            // A path is a directory if there are keys that start with "path/"
            let mut dir_prefix = path.to_vec();
            if !dir_prefix.is_empty() && dir_prefix[dir_prefix.len() - 1] != b'/' {
                dir_prefix.push(b'/');
            }
            let mut iter = store.prefix_iterator(&dir_prefix);
            iter.next().is_some()
        } else {
            false
        }
    }

    /// Check if file handle has read permission
    fn has_read_permission(&self, _file_handle: &FileHandle) -> bool {
        // For now, allow read access if file was not opened write-only
        // In a full implementation, this would check actual capabilities
        true
    }
}

impl preopens::Host for VfsFilesystem {
    fn get_directories(&mut self) -> HostResult<Vec<(Resource<fs_types::Descriptor>, String)>> {
        let mut preopens: Vec<(Resource<fs_types::Descriptor>, String)> = Vec::new();

        // Create a descriptor for each mount point
        for (mount_id, mount) in self.mounts.iter().enumerate() {
            let descriptor_id = self.next_descriptor;
            self.next_descriptor += 1;

            self.descriptors.insert(
                descriptor_id,
                DescriptorKind::Dir {
                    mount_id,
                    path: PathBuf::from(&mount.guest_prefix),
                },
            );

            // Hand back a handle with our descriptor id as the representation
            preopens.push((Resource::new_own(descriptor_id), mount.guest_prefix.clone()));
        }

        Ok(preopens)
    }
}

impl fs_types::Host for VfsFilesystem {
    fn filesystem_error_code(
        &mut self,
        _err: Resource<fs_types::Error>,
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
        let fd = descriptor.rep();
        match self.descriptors.get(&fd) {
            Some(DescriptorKind::File { handle }) => {
                if !handle.writable {
                    return Err(fs_types::ErrorCode::Access.into());
                }

                // Use the VFS set_size method
                let vfs = handle.vfs.lock().unwrap();
                vfs.set_size(handle.vfs_fd as u32, size)
                    .map_err(|e| wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e)))?;

                Ok(())
            }
            Some(DescriptorKind::Dir { .. }) => Err(fs_types::ErrorCode::IsDirectory.into()),
            None => Err(fs_types::ErrorCode::BadDescriptor.into()),
        }
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

        // Get the mount ID and directory path from the descriptor
        let (mount_id, dir_path) = match self.descriptors.get(&fd) {
            Some(DescriptorKind::Dir { mount_id, path }) => (*mount_id, path.clone()),
            Some(DescriptorKind::File { .. }) => {
                return Err(fs_types::ErrorCode::NotDirectory.into())
            }
            None => return Err(fs_types::ErrorCode::BadDescriptor.into()),
        };

        // Get the mount and namespace
        let mount = &self.mounts[mount_id];
        let namespace = &mount.vfs_namespace;

        // Create a new directory stream
        let mut stream = DirectoryStream::new(dir_path.clone(), mount_id);

        // Get entries from VFS
        let vfs = self
            .vfs
            .lock()
            .map_err(|_| wasmtime_wasi::TrappableError::from(fs_types::ErrorCode::Io))?;

        // Build proper prefix for listing based on the directory path
        let prefix = if dir_path == Path::new(&mount.guest_prefix) {
            // Listing root of mount
            Vec::new()
        } else {
            // Listing a subdirectory - extract relative path
            let relative = dir_path.strip_prefix(&mount.guest_prefix).map_err(|_| {
                wasmtime_wasi::TrappableError::from(fs_types::ErrorCode::NotDirectory)
            })?;
            relative.to_string_lossy().into_owned().into_bytes()
        };

        // Collect all entries and organize them
        // Performance Note: This currently loads all entries upfront. For very large
        // directories, consider implementing lazy loading or pagination in future iterations.
        let mut seen_dirs = std::collections::HashSet::new();

        if let Some(store) = vfs.get_store(namespace) {
            let store = store
                .lock()
                .map_err(|_| wasmtime_wasi::TrappableError::from(fs_types::ErrorCode::Io))?;
            let iter = store.prefix_iterator(&prefix);

            for (key, _) in iter {
                if let Ok(key_str) = String::from_utf8(key.clone()) {
                    // Skip the prefix itself
                    let relative_key = if prefix.is_empty() {
                        key_str.clone()
                    } else {
                        let prefix_str = String::from_utf8_lossy(&prefix);
                        if key_str.starts_with(&*prefix_str) {
                            key_str[prefix_str.len()..]
                                .trim_start_matches('/')
                                .to_string()
                        } else {
                            continue;
                        }
                    };

                    // Check if this is a directory (has more path components)
                    if let Some(slash_pos) = relative_key.find('/') {
                        // This is a directory - add only the directory name
                        let dir_name = &relative_key[..slash_pos];
                        if seen_dirs.insert(dir_name.to_string()) {
                            stream.add_entry(dir_name.to_string(), true);
                        }
                    } else if !relative_key.is_empty() {
                        // This is a file in the current directory
                        stream.add_entry(relative_key, false);
                    }
                }
            }
        }

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
        descriptor: Resource<fs_types::Descriptor>,
        path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        let dir_fd = descriptor.rep();

        // Validate directory descriptor
        let mount_id = match self.descriptors.get(&dir_fd) {
            Some(DescriptorKind::Dir { mount_id, .. }) => *mount_id,
            Some(DescriptorKind::File { .. }) => {
                return Err(fs_types::ErrorCode::NotDirectory.into())
            }
            None => return Err(fs_types::ErrorCode::BadDescriptor.into()),
        };

        let mount = &self.mounts[mount_id];

        // Check write permission on mount
        if !mount.caps.write {
            return Err(fs_types::ErrorCode::Access.into());
        }

        // Build VFS path: /{namespace}/{path}/
        let vfs_path = if path.is_empty() {
            return Err(fs_types::ErrorCode::BadDescriptor.into());
        } else {
            PathBuf::from(format!("/{}/{}/", mount.vfs_namespace, path))
        };

        // Add write capability for this path
        {
            let vfs = self.vfs.lock().unwrap();
            let _ = vfs.add_capability(Capability::Write(vfs_path.clone()));
        }

        // Create a directory marker in the VFS
        // This helps distinguish between non-existent paths and empty directories
        let vfs = self.vfs.lock().unwrap();

        // Use the VFS create_directory method to create the marker
        vfs.create_directory(&vfs_path)
            .map_err(|e| wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e)))?;

        Ok(())
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
                size: 0, // Directories always have size 0
                data_access_timestamp: None,
                data_modification_timestamp: None,
                status_change_timestamp: None,
            }),
            Some(DescriptorKind::File { handle }) => {
                // Use the VFS stat_by_fd method to get accurate size
                let vfs = handle.vfs.lock().unwrap();
                let file_info = vfs
                    .stat_by_fd(handle.vfs_fd as u32)
                    .map_err(|e| wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e)))?;

                Ok(fs_types::DescriptorStat {
                    type_: fs_types::DescriptorType::RegularFile,
                    link_count: 1,
                    size: file_info.size, // Use actual size from file descriptor
                    data_access_timestamp: None,
                    data_modification_timestamp: None,
                    status_change_timestamp: None,
                })
            }
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
        _path_flags: fs_types::PathFlags,
        path: String,
        open_flags: fs_types::OpenFlags,
        flags: fs_types::DescriptorFlags,
    ) -> Result<Resource<fs_types::Descriptor>, wasmtime_wasi::TrappableError<fs_types::ErrorCode>>
    {
        let dir_fd = descriptor.rep();
        // Validate directory descriptor
        let (mount_id, parent_path) = match self.descriptors.get(&dir_fd) {
            Some(DescriptorKind::Dir { mount_id, path }) => (*mount_id, path.clone()),
            Some(DescriptorKind::File { .. }) => {
                return Err(fs_types::ErrorCode::NotDirectory.into())
            }
            None => return Err(fs_types::ErrorCode::BadDescriptor.into()),
        };

        let mount = &self.mounts[mount_id];

        // Step 1: Validate flag combinations
        // EXCLUSIVE requires CREATE
        if open_flags.contains(fs_types::OpenFlags::EXCLUSIVE)
            && !open_flags.contains(fs_types::OpenFlags::CREATE)
        {
            return Err(fs_types::ErrorCode::Invalid.into());
        }

        // Determine if we need write capability
        let needs_write = flags.contains(fs_types::DescriptorFlags::WRITE)
            || flags.contains(fs_types::DescriptorFlags::MUTATE_DIRECTORY);

        let needs_write_for_create = open_flags.contains(fs_types::OpenFlags::CREATE);
        let needs_write_for_truncate = open_flags.contains(fs_types::OpenFlags::TRUNCATE);

        // Any write operation requires write capability
        let requires_write_cap = needs_write || needs_write_for_create || needs_write_for_truncate;

        // Step 2: Check mount capabilities
        if requires_write_cap && !mount.caps.write {
            return Err(fs_types::ErrorCode::Access.into());
        }

        if flags.contains(fs_types::DescriptorFlags::READ) && !mount.caps.read {
            return Err(fs_types::ErrorCode::Access.into());
        }

        // Build the full guest path by joining parent path with the relative path
        let full_guest_path = if path.is_empty() {
            parent_path.clone()
        } else {
            parent_path.join(&path)
        };

        // Build VFS path: /{namespace}/{relative_path}
        // Extract relative path from guest path
        let relative_path = full_guest_path
            .strip_prefix(&mount.guest_prefix)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let vfs_path = if relative_path.is_empty() {
            PathBuf::from(format!("/{}/", mount.vfs_namespace))
        } else {
            PathBuf::from(format!("/{}/{}", mount.vfs_namespace, relative_path))
        };

        // Check if this path is a directory
        let path_bytes = relative_path.as_bytes();
        let is_dir = if path.is_empty() && parent_path == PathBuf::from(&mount.guest_prefix) {
            true // Mount root is always a directory
        } else if open_flags.contains(fs_types::OpenFlags::DIRECTORY) {
            // Explicitly opening as directory - verify it actually is one
            let is_directory = self.is_directory(&mount.vfs_namespace, path_bytes);
            if !is_directory && !open_flags.contains(fs_types::OpenFlags::CREATE) {
                // Trying to open non-directory as directory
                return Err(fs_types::ErrorCode::NotDirectory.into());
            }
            true
        } else {
            // Check if path has directory entries
            self.is_directory(&mount.vfs_namespace, path_bytes)
        };

        // Register descriptor based on type
        let new_fd = self.next_descriptor;
        self.next_descriptor += 1;

        if is_dir {
            // Opening a directory - directories don't support CREATE/TRUNCATE
            if open_flags.contains(fs_types::OpenFlags::CREATE) && !path.is_empty() {
                // Can't create a file when DIRECTORY flag is set
                if open_flags.contains(fs_types::OpenFlags::DIRECTORY) {
                    return Err(fs_types::ErrorCode::Invalid.into());
                }
            }

            self.descriptors.insert(
                new_fd,
                DescriptorKind::Dir {
                    mount_id,              // Use the same mount as parent
                    path: full_guest_path, // Store the full guest path
                },
            );
        } else {
            // Opening a file

            // Step 3: Map capabilities to VFS operations
            {
                let vfs = self
                    .vfs
                    .lock()
                    .map_err(|_| wasmtime_wasi::TrappableError::from(fs_types::ErrorCode::Io))?;

                // Always need read capability for open
                let _ = vfs.add_capability(Capability::Read(vfs_path.clone()));

                if requires_write_cap {
                    let _ = vfs.add_capability(Capability::Write(vfs_path.clone()));
                }
            }

            // Check file existence for EXCLUSIVE flag
            let file_exists = {
                let vfs = self
                    .vfs
                    .lock()
                    .map_err(|_| wasmtime_wasi::TrappableError::from(fs_types::ErrorCode::Io))?;
                vfs.exists(&vfs_path)
            };

            // Handle EXCLUSIVE flag - fail if file exists when creating exclusively
            if open_flags.contains(fs_types::OpenFlags::EXCLUSIVE) && file_exists {
                return Err(fs_types::ErrorCode::Exist.into());
            }

            // Open or create in VFS
            let vfs_fd = {
                let vfs = self
                    .vfs
                    .lock()
                    .map_err(|_| wasmtime_wasi::TrappableError::from(fs_types::ErrorCode::Io))?;

                // Handle CREATE flag
                let fd = if open_flags.contains(fs_types::OpenFlags::CREATE) && !file_exists {
                    vfs.create(&vfs_path).map_err(|e| {
                        wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e))
                    })?
                } else if !file_exists {
                    // File doesn't exist and CREATE not specified
                    return Err(fs_types::ErrorCode::NoEntry.into());
                } else {
                    // File exists, open it
                    vfs.open(&vfs_path, requires_write_cap).map_err(|e| {
                        wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e))
                    })?
                };

                // Step 4: Handle TRUNCATE flag
                if open_flags.contains(fs_types::OpenFlags::TRUNCATE) {
                    // Truncate means resize to 0
                    vfs.set_size(fd, 0).map_err(|e| {
                        wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e))
                    })?;
                }

                fd
            } as u64;

            // Step 5: Create file handle with mapped descriptor flags
            self.descriptors.insert(
                new_fd,
                DescriptorKind::File {
                    handle: FileHandle {
                        vfs_fd,
                        position: Arc::new(AtomicU64::new(0)),
                        vfs: self.vfs.clone(),
                        writable: needs_write, // Map WRITE flag to writable
                    },
                },
            );
        }

        Ok(Resource::new_own(new_fd))
    }

    fn drop(&mut self, descriptor: Resource<fs_types::Descriptor>) -> HostResult<()> {
        let fd = descriptor.rep();
        // If this was a file descriptor, close underlying VFS fd
        if let Some(kind) = self.descriptors.remove(&fd) {
            if let DescriptorKind::File { handle } = kind {
                // Best effort close - if lock is poisoned, we can't close
                if let Ok(vfs) = self.vfs.lock() {
                    let _ = vfs.close(handle.vfs_fd as u32);
                }
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
        descriptor: Resource<fs_types::Descriptor>,
        path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        let dir_fd = descriptor.rep();

        // Validate directory descriptor
        let mount_id = match self.descriptors.get(&dir_fd) {
            Some(DescriptorKind::Dir { mount_id, .. }) => *mount_id,
            Some(DescriptorKind::File { .. }) => {
                return Err(fs_types::ErrorCode::NotDirectory.into())
            }
            None => return Err(fs_types::ErrorCode::BadDescriptor.into()),
        };

        let mount = &self.mounts[mount_id];

        // Check write permission on mount
        if !mount.caps.write {
            return Err(fs_types::ErrorCode::Access.into());
        }

        // Build VFS path for directory: /{namespace}/{path}/
        let vfs_path = if path.is_empty() {
            return Err(fs_types::ErrorCode::BadDescriptor.into());
        } else {
            PathBuf::from(format!("/{}/{}/", mount.vfs_namespace, path))
        };

        // Add write capability for this path
        {
            let vfs = self.vfs.lock().unwrap();
            let _ = vfs.add_capability(Capability::Write(vfs_path.clone()));
            let _ = vfs.add_capability(Capability::Read(vfs_path.clone()));
        }

        // Check if directory is empty by looking for any files with this prefix
        let vfs = self.vfs.lock().unwrap();
        let (namespace, _) = vfs
            .parse_path(&vfs_path)
            .map_err(|e| wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e)))?;

        // Check if there are any entries with this directory path as prefix
        // We need to check for any keys that start with "directory_name/"
        let dir_prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        };

        if vfs
            .has_prefix(&namespace, dir_prefix.as_bytes())
            .map_err(|e| wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e)))?
        {
            return Err(fs_types::ErrorCode::NotEmpty.into());
        }

        // Directory is empty, remove it
        // Note: In a KV store, directories are implicit. We remove the directory marker
        // if it exists. If the directory doesn't exist at all (no marker and no files),
        // we follow POSIX semantics and return ENOENT.

        // Try to remove the directory through VFS
        vfs.remove_directory(&vfs_path)
            .map_err(|e| wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e)))?;

        Ok(())
    }

    async fn rename_at(
        &mut self,
        descriptor: Resource<fs_types::Descriptor>,
        old_path: String,
        new_descriptor: Resource<fs_types::Descriptor>,
        new_path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        let old_dir_fd = descriptor.rep();
        let new_dir_fd = new_descriptor.rep();

        // Validate old directory descriptor
        let old_mount_id = match self.descriptors.get(&old_dir_fd) {
            Some(DescriptorKind::Dir { mount_id, .. }) => *mount_id,
            Some(DescriptorKind::File { .. }) => {
                return Err(fs_types::ErrorCode::NotDirectory.into())
            }
            None => return Err(fs_types::ErrorCode::BadDescriptor.into()),
        };

        // Validate new directory descriptor
        let new_mount_id = match self.descriptors.get(&new_dir_fd) {
            Some(DescriptorKind::Dir { mount_id, .. }) => *mount_id,
            Some(DescriptorKind::File { .. }) => {
                return Err(fs_types::ErrorCode::NotDirectory.into())
            }
            None => return Err(fs_types::ErrorCode::BadDescriptor.into()),
        };

        let old_mount = &self.mounts[old_mount_id];
        let new_mount = &self.mounts[new_mount_id];

        // Check permissions
        if !old_mount.caps.write || !new_mount.caps.write {
            return Err(fs_types::ErrorCode::Access.into());
        }

        // Build VFS paths
        let old_vfs_path = PathBuf::from(format!("/{}/{}", old_mount.vfs_namespace, old_path));
        let new_vfs_path = PathBuf::from(format!("/{}/{}", new_mount.vfs_namespace, new_path));

        // Add capabilities
        {
            let vfs = self.vfs.lock().unwrap();
            let _ = vfs.add_capability(Capability::Read(old_vfs_path.clone()));
            let _ = vfs.add_capability(Capability::Write(old_vfs_path.clone()));
            let _ = vfs.add_capability(Capability::Write(new_vfs_path.clone()));
        }

        // Perform the rename operation using VFS's rename method
        let vfs = self.vfs.lock().unwrap();
        vfs.rename(&old_vfs_path, &new_vfs_path)
            .map_err(|e| wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e)))?;

        Ok(())
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
        descriptor: Resource<fs_types::Descriptor>,
        path: String,
    ) -> Result<(), wasmtime_wasi::TrappableError<fs_types::ErrorCode>> {
        let dir_fd = descriptor.rep();

        // Validate directory descriptor
        let mount_id = match self.descriptors.get(&dir_fd) {
            Some(DescriptorKind::Dir { mount_id, .. }) => *mount_id,
            Some(DescriptorKind::File { .. }) => {
                return Err(fs_types::ErrorCode::NotDirectory.into())
            }
            None => return Err(fs_types::ErrorCode::BadDescriptor.into()),
        };

        let mount = &self.mounts[mount_id];

        // Check write permission on mount
        if !mount.caps.write {
            return Err(fs_types::ErrorCode::Access.into());
        }

        // Build VFS path: /{namespace}/{path}
        let vfs_path = if path.is_empty() {
            return Err(fs_types::ErrorCode::BadDescriptor.into());
        } else {
            PathBuf::from(format!("/{}/{}", mount.vfs_namespace, path))
        };

        // Check if it's a directory (ends with /) - we can't unlink directories
        if path.ends_with('/') {
            return Err(fs_types::ErrorCode::IsDirectory.into());
        }

        // Add write capability for this path
        {
            let vfs = self.vfs.lock().unwrap();
            let _ = vfs.add_capability(Capability::Write(vfs_path.clone()));
        }

        // Delete the file from VFS
        let vfs = self.vfs.lock().unwrap();
        vfs.unlink(&vfs_path)
            .map_err(|e| wasmtime_wasi::TrappableError::from(Self::convert_vfs_error(e)))?;

        Ok(())
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
        use crate::vfs_streams_simple::VfsInputStream;

        let fd = descriptor.rep();
        match self.descriptors.get(&fd).cloned() {
            Some(DescriptorKind::File { handle }) => {
                // Create a new file handle with the specified offset
                let stream_handle = handle.clone();
                stream_handle.position.store(offset, Ordering::SeqCst);

                // Create the input stream and box it
                let stream = VfsInputStream::new(stream_handle);
                let boxed_stream: Box<dyn wasmtime_wasi::p2::InputStream> = Box::new(stream);

                // Push to resource table and return
                let resource = self
                    .table
                    .push(boxed_stream)
                    .map_err(|_| wasmtime_wasi::TrappableError::from(fs_types::ErrorCode::Io))?;
                Ok(resource)
            }
            Some(DescriptorKind::Dir { .. }) => Err(fs_types::ErrorCode::IsDirectory.into()),
            None => Err(fs_types::ErrorCode::BadDescriptor.into()),
        }
    }

    fn write_via_stream(
        &mut self,
        descriptor: Resource<fs_types::Descriptor>,
        offset: fs_types::Filesize,
    ) -> Result<
        Resource<io_streams::OutputStream>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        use crate::vfs_streams_simple::VfsOutputStream;

        let fd = descriptor.rep();
        match self.descriptors.get(&fd).cloned() {
            Some(DescriptorKind::File { handle }) => {
                if !handle.writable {
                    return Err(fs_types::ErrorCode::Access.into());
                }

                // Create a new file handle with the specified offset
                let stream_handle = handle.clone();
                stream_handle.position.store(offset, Ordering::SeqCst);

                // Create the output stream and box it
                let stream = VfsOutputStream::new(stream_handle);
                let boxed_stream: Box<dyn wasmtime_wasi::p2::OutputStream> = Box::new(stream);

                // Push to resource table and return
                let resource = self
                    .table
                    .push(boxed_stream)
                    .map_err(|_| wasmtime_wasi::TrappableError::from(fs_types::ErrorCode::Io))?;
                Ok(resource)
            }
            Some(DescriptorKind::Dir { .. }) => Err(fs_types::ErrorCode::IsDirectory.into()),
            None => Err(fs_types::ErrorCode::BadDescriptor.into()),
        }
    }

    fn append_via_stream(
        &mut self,
        descriptor: Resource<fs_types::Descriptor>,
    ) -> Result<
        Resource<io_streams::OutputStream>,
        wasmtime_wasi::TrappableError<fs_types::ErrorCode>,
    > {
        use crate::vfs_streams_simple::VfsOutputStream;

        let fd = descriptor.rep();
        match self.descriptors.get(&fd).cloned() {
            Some(DescriptorKind::File { handle }) => {
                if !handle.writable {
                    return Err(fs_types::ErrorCode::Access.into());
                }

                // For append, get the current file size to set position at end
                let file_size = {
                    let vfs = handle.vfs.lock().map_err(|_| {
                        wasmtime_wasi::TrappableError::from(fs_types::ErrorCode::Io)
                    })?;
                    vfs.stat_by_fd(handle.vfs_fd as u32)
                        .map(|info| info.size)
                        .unwrap_or(0)
                };

                // Create a new file handle positioned at end of file
                let stream_handle = handle.clone();
                stream_handle.position.store(file_size, Ordering::SeqCst);

                // Create the output stream and box it
                let stream = VfsOutputStream::new(stream_handle);
                let boxed_stream: Box<dyn wasmtime_wasi::p2::OutputStream> = Box::new(stream);

                // Push to resource table and return
                let resource = self
                    .table
                    .push(boxed_stream)
                    .map_err(|_| wasmtime_wasi::TrappableError::from(fs_types::ErrorCode::Io))?;
                Ok(resource)
            }
            Some(DescriptorKind::Dir { .. }) => Err(fs_types::ErrorCode::IsDirectory.into()),
            None => Err(fs_types::ErrorCode::BadDescriptor.into()),
        }
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
        let dir_stream = self.directory_streams.get_mut(&stream_id).ok_or_else(|| {
            wasmtime_wasi::TrappableError::from(fs_types::ErrorCode::BadDescriptor)
        })?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use gridway_store::{KVStore, MemStore};
    use wasmtime_wasi::p2::bindings::filesystem::{preopens, types as fs_types};

    #[tokio::test]
    async fn test_open_at_capability_enforcement() {
        // Test that CREATE without write capability fails
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let store = Arc::new(Mutex::new(MemStore::new()));

        vfs.lock()
            .unwrap()
            .mount_store("config".to_string(), store.clone())
            .unwrap();

        let mut fs = VfsFilesystem::new(vfs);

        // Get config mount descriptor (read-only)
        let preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        let config_descriptor = preopens
            .iter()
            .find(|(_, path)| path == "/config")
            .map(|(desc, _)| Resource::new_borrow(desc.rep()))
            .unwrap();

        // Try to create a file without write capability
        let result = <VfsFilesystem as fs_types::HostDescriptor>::open_at(
            &mut fs,
            config_descriptor,
            fs_types::PathFlags::empty(),
            "test.txt".to_string(),
            fs_types::OpenFlags::CREATE,
            fs_types::DescriptorFlags::WRITE,
        )
        .await;

        // Should fail with Access error
        assert!(result.is_err());
        match result.unwrap_err().downcast() {
            Ok(err) => assert_eq!(err, fs_types::ErrorCode::Access),
            Err(_) => panic!("Expected Access error"),
        }
    }

    #[tokio::test]
    async fn test_open_at_truncate_flag() {
        // Test that TRUNCATE flag correctly resizes file to 0
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let store = Arc::new(Mutex::new(MemStore::new()));

        // Pre-populate a file with content
        {
            let mut store = store.lock().unwrap();
            store
                .set(b"existing.txt", b"This is existing content")
                .unwrap();
        }

        vfs.lock()
            .unwrap()
            .mount_store("state".to_string(), store.clone())
            .unwrap();

        let mut fs = VfsFilesystem::new(vfs);

        // Get root descriptor
        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        let (root_descriptor, _) = preopens.remove(0);

        // Open file with TRUNCATE flag
        let fd = <VfsFilesystem as fs_types::HostDescriptor>::open_at(
            &mut fs,
            root_descriptor,
            fs_types::PathFlags::empty(),
            "existing.txt".to_string(),
            fs_types::OpenFlags::TRUNCATE,
            fs_types::DescriptorFlags::WRITE | fs_types::DescriptorFlags::READ,
        )
        .await
        .unwrap();

        // Verify file was truncated by checking through the store
        let fd_id = fd.rep();
        if let Some(DescriptorKind::File { handle: _ }) = fs.descriptors.get(&fd_id) {
            // Check the actual content in the store to verify truncation
            let store = store.lock().unwrap();
            let content = store.get(b"existing.txt").unwrap().unwrap_or_default();
            // The file should still exist but be empty after truncation
            assert_eq!(content.len(), 0, "File should be truncated to 0 bytes");
        } else {
            panic!("Expected file descriptor");
        }
    }

    #[tokio::test]
    async fn test_open_at_exclusive_flag() {
        // Test that EXCLUSIVE flag prevents overwriting existing files
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let store = Arc::new(Mutex::new(MemStore::new()));

        // Pre-populate a file
        {
            let mut store = store.lock().unwrap();
            store.set(b"existing.txt", b"content").unwrap();
        }

        vfs.lock()
            .unwrap()
            .mount_store("state".to_string(), store.clone())
            .unwrap();

        let mut fs = VfsFilesystem::new(vfs);

        // Get root descriptor
        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        let (root_descriptor, _) = preopens.remove(0);

        // Try to create file with EXCLUSIVE flag when it already exists
        let result = <VfsFilesystem as fs_types::HostDescriptor>::open_at(
            &mut fs,
            root_descriptor,
            fs_types::PathFlags::empty(),
            "existing.txt".to_string(),
            fs_types::OpenFlags::CREATE | fs_types::OpenFlags::EXCLUSIVE,
            fs_types::DescriptorFlags::WRITE,
        )
        .await;

        // Should fail with Exist error
        assert!(result.is_err());
        match result.unwrap_err().downcast() {
            Ok(err) => assert_eq!(err, fs_types::ErrorCode::Exist),
            Err(_) => panic!("Expected Exist error"),
        }
    }

    #[tokio::test]
    async fn test_open_at_invalid_flag_combinations() {
        // Test that EXCLUSIVE without CREATE fails
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let store = Arc::new(Mutex::new(MemStore::new()));

        vfs.lock()
            .unwrap()
            .mount_store("state".to_string(), store.clone())
            .unwrap();

        let mut fs = VfsFilesystem::new(vfs);

        // Get root descriptor
        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        let (root_descriptor, _) = preopens.remove(0);

        // Try EXCLUSIVE without CREATE
        let result = <VfsFilesystem as fs_types::HostDescriptor>::open_at(
            &mut fs,
            root_descriptor,
            fs_types::PathFlags::empty(),
            "test.txt".to_string(),
            fs_types::OpenFlags::EXCLUSIVE, // Missing CREATE
            fs_types::DescriptorFlags::WRITE,
        )
        .await;

        // Should fail with Invalid error
        assert!(result.is_err());
        match result.unwrap_err().downcast() {
            Ok(err) => assert_eq!(err, fs_types::ErrorCode::Invalid),
            Err(_) => panic!("Expected Invalid error"),
        }
    }

    #[tokio::test]
    async fn test_open_at_descriptor_flag_mapping() {
        // Test that descriptor flags are correctly mapped to file handle properties
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let store = Arc::new(Mutex::new(MemStore::new()));

        vfs.lock()
            .unwrap()
            .mount_store("state".to_string(), store.clone())
            .unwrap();

        let mut fs = VfsFilesystem::new(vfs);

        // Get root descriptor
        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        let (root_descriptor, _) = preopens.remove(0);

        // Test 1: Open with WRITE flag
        let write_fd = <VfsFilesystem as fs_types::HostDescriptor>::open_at(
            &mut fs,
            root_descriptor,
            fs_types::PathFlags::empty(),
            "write_test.txt".to_string(),
            fs_types::OpenFlags::CREATE,
            fs_types::DescriptorFlags::WRITE,
        )
        .await
        .unwrap();

        // Verify writable flag is set
        let write_fd_id = write_fd.rep();
        if let Some(DescriptorKind::File { handle }) = fs.descriptors.get(&write_fd_id) {
            assert!(handle.writable, "File should be writable with WRITE flag");
        }

        // Test 2: Open with READ only flag
        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        let (root_descriptor, _) = preopens.remove(0);

        // Create file first
        {
            let mut store = store.lock().unwrap();
            store.set(b"read_test.txt", b"content").unwrap();
        }

        let read_fd = <VfsFilesystem as fs_types::HostDescriptor>::open_at(
            &mut fs,
            root_descriptor,
            fs_types::PathFlags::empty(),
            "read_test.txt".to_string(),
            fs_types::OpenFlags::empty(),
            fs_types::DescriptorFlags::READ,
        )
        .await
        .unwrap();

        // Verify writable flag is not set
        let read_fd_id = read_fd.rep();
        if let Some(DescriptorKind::File { handle }) = fs.descriptors.get(&read_fd_id) {
            assert!(
                !handle.writable,
                "File should not be writable with only READ flag"
            );
        }
    }

    #[tokio::test]
    async fn test_open_at_nonexistent_file_without_create() {
        // Test that opening non-existent file without CREATE fails
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let store = Arc::new(Mutex::new(MemStore::new()));

        vfs.lock()
            .unwrap()
            .mount_store("state".to_string(), store.clone())
            .unwrap();

        let mut fs = VfsFilesystem::new(vfs);

        // Get root descriptor
        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        let (root_descriptor, _) = preopens.remove(0);

        // Try to open non-existent file without CREATE
        let result = <VfsFilesystem as fs_types::HostDescriptor>::open_at(
            &mut fs,
            root_descriptor,
            fs_types::PathFlags::empty(),
            "nonexistent.txt".to_string(),
            fs_types::OpenFlags::empty(), // No CREATE flag
            fs_types::DescriptorFlags::READ,
        )
        .await;

        // Should fail with NoEntry error
        assert!(result.is_err());
        match result.unwrap_err().downcast() {
            Ok(err) => assert_eq!(err, fs_types::ErrorCode::NoEntry),
            Err(_) => panic!("Expected NoEntry error"),
        }
    }

    #[tokio::test]
    async fn test_open_at_directory_as_file() {
        // Test that opening a directory without DIRECTORY flag works correctly
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let store = Arc::new(Mutex::new(MemStore::new()));

        // Create directory structure
        {
            let mut store = store.lock().unwrap();
            store.set(b"mydir/file.txt", b"content").unwrap();
        }

        vfs.lock()
            .unwrap()
            .mount_store("state".to_string(), store.clone())
            .unwrap();

        let mut fs = VfsFilesystem::new(vfs);

        // Get root descriptor
        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        let (root_descriptor, _) = preopens.remove(0);

        // Open directory as directory (should succeed)
        let dir_fd = <VfsFilesystem as fs_types::HostDescriptor>::open_at(
            &mut fs,
            root_descriptor,
            fs_types::PathFlags::empty(),
            "mydir".to_string(),
            fs_types::OpenFlags::DIRECTORY,
            fs_types::DescriptorFlags::READ,
        )
        .await
        .unwrap();

        // Verify it's a directory descriptor
        let dir_fd_id = dir_fd.rep();
        assert!(matches!(
            fs.descriptors.get(&dir_fd_id),
            Some(DescriptorKind::Dir { .. })
        ));
    }

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
        vfs.lock()
            .unwrap()
            .mount_store("state".to_string(), store.clone())
            .unwrap();

        // Create VFS filesystem
        let mut fs = VfsFilesystem::new(vfs);

        // Get the preopened directory descriptor for root using the trait
        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        assert!(!preopens.is_empty());

        let (root_descriptor, _path) = preopens.remove(0);

        // Read the directory using the trait
        let stream =
            <VfsFilesystem as fs_types::HostDescriptor>::read_directory(&mut fs, root_descriptor)
                .await
                .unwrap();

        // Read entries from the stream using the trait
        let mut entries = Vec::new();
        let stream_id = stream.rep(); // Get the stream ID for repeated use
        loop {
            // Use the stream ID to create a new resource handle for each call
            let stream_ref = Resource::new_borrow(stream_id);
            match <VfsFilesystem as fs_types::HostDirectoryEntryStream>::read_directory_entry(
                &mut fs, stream_ref,
            )
            .await
            .unwrap()
            {
                Some(entry) => entries.push(entry.name),
                None => break,
            }
        }

        // Verify we got the expected entries
        assert!(entries.contains(&"file1.txt".to_string()));
        assert!(entries.contains(&"file2.txt".to_string()));
        // The directory listing should show "subdir" as a directory, not "subdir/file3.txt"
        assert!(entries.contains(&"subdir".to_string()));

        // Clean up using the trait
        <VfsFilesystem as fs_types::HostDirectoryEntryStream>::drop(&mut fs, stream).unwrap();
    }

    #[tokio::test]
    async fn test_nested_directory_iteration() {
        // Create a VFS with nested directory structure
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let store = Arc::new(Mutex::new(MemStore::new()));

        // Add nested directory structure
        {
            let mut store = store.lock().unwrap();
            // Root files
            store.set(b"README.md", b"root readme").unwrap();
            store.set(b"config.toml", b"config").unwrap();

            // src directory
            store.set(b"src/main.rs", b"main code").unwrap();
            store.set(b"src/lib.rs", b"lib code").unwrap();
            store.set(b"src/utils.rs", b"utils").unwrap();

            // src/modules subdirectory
            store.set(b"src/modules/auth.rs", b"auth module").unwrap();
            store.set(b"src/modules/db.rs", b"db module").unwrap();

            // tests directory
            store.set(b"tests/unit.rs", b"unit tests").unwrap();
            store
                .set(b"tests/integration.rs", b"integration tests")
                .unwrap();
        }

        // Mount the store
        vfs.lock()
            .unwrap()
            .mount_store("state".to_string(), store.clone())
            .unwrap();

        // Create VFS filesystem
        let mut fs = VfsFilesystem::new(vfs);

        // Test 1: List root directory
        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        let (root_descriptor, _) = preopens.remove(0);

        let stream =
            <VfsFilesystem as fs_types::HostDescriptor>::read_directory(&mut fs, root_descriptor)
                .await
                .unwrap();

        let mut root_entries = Vec::new();
        let stream_id = stream.rep();
        loop {
            let stream_ref = Resource::new_borrow(stream_id);
            match <VfsFilesystem as fs_types::HostDirectoryEntryStream>::read_directory_entry(
                &mut fs, stream_ref,
            )
            .await
            .unwrap()
            {
                Some(entry) => root_entries.push((entry.name, entry.type_)),
                None => break,
            }
        }

        // Verify root contains files and directories
        assert!(root_entries.iter().any(|(name, _)| name == "README.md"));
        assert!(root_entries.iter().any(|(name, _)| name == "config.toml"));
        assert!(root_entries
            .iter()
            .any(|(name, typ)| name == "src" && *typ == fs_types::DescriptorType::Directory));
        assert!(root_entries
            .iter()
            .any(|(name, typ)| name == "tests" && *typ == fs_types::DescriptorType::Directory));

        <VfsFilesystem as fs_types::HostDirectoryEntryStream>::drop(&mut fs, stream).unwrap();

        // Test 2: Open and list the src directory
        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        let (root_descriptor, _) = preopens.remove(0);

        let src_descriptor = <VfsFilesystem as fs_types::HostDescriptor>::open_at(
            &mut fs,
            root_descriptor,
            fs_types::PathFlags::empty(),
            "src".to_string(),
            fs_types::OpenFlags::DIRECTORY,
            fs_types::DescriptorFlags::READ,
        )
        .await
        .unwrap();

        let src_stream =
            <VfsFilesystem as fs_types::HostDescriptor>::read_directory(&mut fs, src_descriptor)
                .await
                .unwrap();

        let mut src_entries = Vec::new();
        let src_stream_id = src_stream.rep();
        loop {
            let stream_ref = Resource::new_borrow(src_stream_id);
            match <VfsFilesystem as fs_types::HostDirectoryEntryStream>::read_directory_entry(
                &mut fs, stream_ref,
            )
            .await
            .unwrap()
            {
                Some(entry) => src_entries.push((entry.name, entry.type_)),
                None => break,
            }
        }

        // Verify src directory contents
        assert!(src_entries.iter().any(|(name, _)| name == "main.rs"));
        assert!(src_entries.iter().any(|(name, _)| name == "lib.rs"));
        assert!(src_entries.iter().any(|(name, _)| name == "utils.rs"));
        assert!(src_entries
            .iter()
            .any(|(name, typ)| name == "modules" && *typ == fs_types::DescriptorType::Directory));

        <VfsFilesystem as fs_types::HostDirectoryEntryStream>::drop(&mut fs, src_stream).unwrap();
    }

    #[tokio::test]
    async fn test_stat_returns_accurate_size() {
        // Create a VFS and filesystem
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let store = Arc::new(Mutex::new(MemStore::new()));

        // Mount the store
        vfs.lock()
            .unwrap()
            .mount_store("state".to_string(), store.clone())
            .unwrap();

        // Create VFS filesystem
        let mut fs = VfsFilesystem::new(vfs);

        // Get root directory descriptor
        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        let (root_descriptor, _) = preopens.remove(0);

        // Create a file with specific content
        let test_content = b"This is test content for stat verification";
        let root_id = root_descriptor.rep();
        let file_descriptor = <VfsFilesystem as fs_types::HostDescriptor>::open_at(
            &mut fs,
            Resource::new_borrow(root_id),
            fs_types::PathFlags::empty(),
            "test_file.txt".to_string(),
            fs_types::OpenFlags::CREATE,
            fs_types::DescriptorFlags::WRITE | fs_types::DescriptorFlags::READ,
        )
        .await
        .unwrap();

        // Write content to the file through VFS
        let file_id = file_descriptor.rep();
        if let Some(DescriptorKind::File { handle }) = fs.descriptors.get(&file_id) {
            let vfs = handle.vfs.lock().unwrap();
            vfs.write(handle.vfs_fd as u32, test_content).unwrap();
        }

        // Get stat and verify size
        let stat = <VfsFilesystem as fs_types::HostDescriptor>::stat(
            &mut fs,
            Resource::new_borrow(file_id),
        )
        .await
        .unwrap();

        assert_eq!(stat.type_, fs_types::DescriptorType::RegularFile);
        assert_eq!(stat.size, test_content.len() as u64);

        // Test directory stat returns size 0
        let dir_stat = <VfsFilesystem as fs_types::HostDescriptor>::stat(
            &mut fs,
            Resource::new_borrow(root_id),
        )
        .await
        .unwrap();

        assert_eq!(dir_stat.type_, fs_types::DescriptorType::Directory);
        assert_eq!(dir_stat.size, 0);

        // Test stat after resizing file
        <VfsFilesystem as fs_types::HostDescriptor>::set_size(
            &mut fs,
            Resource::new_borrow(file_id),
            100,
        )
        .await
        .unwrap();

        let resized_stat = <VfsFilesystem as fs_types::HostDescriptor>::stat(
            &mut fs,
            Resource::new_borrow(file_id),
        )
        .await
        .unwrap();

        assert_eq!(resized_stat.size, 100);
    }

    #[tokio::test]
    async fn test_large_directory_performance() {
        // Create a VFS with many entries to validate performance
        let vfs = Arc::new(Mutex::new(VirtualFilesystem::new()));
        let store = Arc::new(Mutex::new(MemStore::new()));

        // Add many entries to test performance
        {
            let mut store = store.lock().unwrap();
            // Add 100 files to test performance with larger directories
            for i in 0..100 {
                let key = format!("file_{i:03}.txt");
                let value = format!("content_{i}");
                store.set(key.as_bytes(), value.as_bytes()).unwrap();
            }

            // Add some nested directories with files
            for i in 0..10 {
                let dir_files = vec![
                    format!("dir_{:02}/file_a.txt", i),
                    format!("dir_{:02}/file_b.txt", i),
                    format!("dir_{:02}/subdir/file_c.txt", i),
                ];
                for file in dir_files {
                    store.set(file.as_bytes(), b"content").unwrap();
                }
            }
        }

        // Mount the store
        vfs.lock()
            .unwrap()
            .mount_store("state".to_string(), store.clone())
            .unwrap();

        // Create VFS filesystem
        let mut fs = VfsFilesystem::new(vfs);

        // List root directory and measure
        let start = std::time::Instant::now();

        let mut preopens = <VfsFilesystem as preopens::Host>::get_directories(&mut fs).unwrap();
        let (root_descriptor, _) = preopens.remove(0);

        let stream =
            <VfsFilesystem as fs_types::HostDescriptor>::read_directory(&mut fs, root_descriptor)
                .await
                .unwrap();

        let mut entries = Vec::new();
        let stream_id = stream.rep();
        loop {
            let stream_ref = Resource::new_borrow(stream_id);
            match <VfsFilesystem as fs_types::HostDirectoryEntryStream>::read_directory_entry(
                &mut fs, stream_ref,
            )
            .await
            .unwrap()
            {
                Some(entry) => entries.push(entry.name),
                None => break,
            }
        }

        let elapsed = start.elapsed();

        // Verify we got all entries
        assert_eq!(entries.len(), 110); // 100 files + 10 directories

        // Verify correct ordering (entries should be consistent)
        let file_entries: Vec<_> = entries.iter().filter(|e| e.starts_with("file_")).collect();
        assert_eq!(file_entries.len(), 100);

        let dir_entries: Vec<_> = entries.iter().filter(|e| e.starts_with("dir_")).collect();
        assert_eq!(dir_entries.len(), 10);

        // Performance check - should complete reasonably quickly
        // This is a soft check, mainly to catch severe performance regressions
        assert!(
            elapsed.as_millis() < 100,
            "Directory listing took {elapsed:?} which seems slow"
        );

        <VfsFilesystem as fs_types::HostDirectoryEntryStream>::drop(&mut fs, stream).unwrap();
    }
}
