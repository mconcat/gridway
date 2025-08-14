//! Simplified VFS Stream Implementations for WASI I/O
//!
//! This provides a basic implementation of streams that returns unsupported
//! for now, allowing the rest of the system to compile and run.

use crate::vfs::VirtualFilesystem;
use std::sync::{Arc, Mutex};

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

/// Pollable implementation that is always ready
/// This is used for VFS streams since VFS operations are synchronous
pub struct AlwaysReadyPollable;

// For now, we'll provide a minimal implementation that allows compilation
// Full stream support will be added in a future iteration