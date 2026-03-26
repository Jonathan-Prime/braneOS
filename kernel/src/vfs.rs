// ============================================================
// Brane OS Kernel — Virtual Filesystem (VFS)
// ============================================================
//
// Provides a unified interface for filesystem operations.
// All filesystem implementations (RamFS, FAT32, etc.) register
// through this layer via mount points.
//
// Spec reference: ARCHITECTURE.md §5.3 (planned)
// ============================================================

use spin::Mutex;

/// Maximum simultaneous mount points.
const MAX_MOUNTS: usize = 8;

/// Maximum path length.
pub const MAX_PATH: usize = 256;

/// Maximum filename length.
pub const MAX_NAME: usize = 64;

/// Maximum directory entries returned.
pub const MAX_DIR_ENTRIES: usize = 64;

/// Maximum open file descriptors per process.
pub const MAX_FDS: usize = 16;

/// Maximum simultaneously open files (system-wide).
#[allow(dead_code)]
const MAX_OPEN_FILES: usize = 64;

// -----------------------------------------------------------------------
// VFS Types
// -----------------------------------------------------------------------

/// Type of a VFS node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    File,
    Directory,
    Device,
}

/// Metadata about a filesystem node.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub name: [u8; MAX_NAME],
    pub name_len: usize,
    pub node_type: NodeType,
    pub size: usize,
    pub inode: u64,
}

impl NodeInfo {
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("<invalid>")
    }
}

/// A directory entry (for listing).
#[derive(Debug, Clone, Copy)]
pub struct DirEntry {
    pub name: [u8; MAX_NAME],
    pub name_len: usize,
    pub node_type: NodeType,
}

impl DirEntry {
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("<invalid>")
    }
}

/// Errors from VFS operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    NotFound,
    AlreadyExists,
    NotADirectory,
    NotAFile,
    NoSpace,
    InvalidPath,
    MountFull,
    NotMounted,
    FdTableFull,
    BadFd,
    IoError,
}

// -----------------------------------------------------------------------
// Filesystem Trait
// -----------------------------------------------------------------------

/// Trait that all filesystem implementations must satisfy.
pub trait FileSystem: Send {
    /// Get info about a node at `path` (relative to mount root).
    fn stat(&self, path: &str) -> Result<NodeInfo, VfsError>;

    /// Read up to `buf.len()` bytes from the file at `path`, starting at `offset`.
    /// Returns the number of bytes actually read.
    fn read(&self, path: &str, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError>;

    /// Write `data` to the file at `path` starting at `offset`.
    /// Returns the number of bytes written.
    fn write(&mut self, path: &str, offset: usize, data: &[u8]) -> Result<usize, VfsError>;

    /// Create a file or directory at `path`.
    fn create(&mut self, path: &str, node_type: NodeType) -> Result<(), VfsError>;

    /// List entries in a directory at `path`.
    fn readdir(&self, path: &str, entries: &mut [DirEntry]) -> Result<usize, VfsError>;

    /// Remove a node at `path`.
    fn remove(&mut self, path: &str) -> Result<(), VfsError>;

    /// Filesystem name (e.g., "ramfs").
    fn fs_name(&self) -> &str;
}

// -----------------------------------------------------------------------
// Mount Table
// -----------------------------------------------------------------------

struct MountPoint {
    path: [u8; MAX_PATH],
    path_len: usize,
    // We store a raw pointer because we need dynamic dispatch with mutability
    // in a no_std, no-alloc (at mount time) context. The pointer is always valid
    // for the lifetime of the kernel.
    fs: *mut dyn FileSystem,
}

impl MountPoint {
    fn path_str(&self) -> &str {
        core::str::from_utf8(&self.path[..self.path_len]).unwrap_or("")
    }
}

// Safety: MountPoint.fs points to a static-lifetime filesystem behind a Mutex.
unsafe impl Send for MountPoint {}

// -----------------------------------------------------------------------
// Global VFS Manager
// -----------------------------------------------------------------------

pub struct VfsManager {
    mounts: [Option<MountPoint>; MAX_MOUNTS],
    mount_count: usize,
}

impl VfsManager {
    const fn new() -> Self {
        const NONE: Option<MountPoint> = None;
        Self {
            mounts: [NONE; MAX_MOUNTS],
            mount_count: 0,
        }
    }

    /// Mount a filesystem at the given path.
    ///
    /// # Safety
    /// `fs` must point to a valid `FileSystem` that lives for the kernel's lifetime.
    pub unsafe fn mount(&mut self, path: &str, fs: *mut dyn FileSystem) -> Result<(), VfsError> {
        if self.mount_count >= MAX_MOUNTS {
            return Err(VfsError::MountFull);
        }
        if path.len() > MAX_PATH {
            return Err(VfsError::InvalidPath);
        }

        let mut path_buf = [0u8; MAX_PATH];
        path_buf[..path.len()].copy_from_slice(path.as_bytes());

        // Find an empty slot
        for slot in self.mounts.iter_mut() {
            if slot.is_none() {
                *slot = Some(MountPoint {
                    path: path_buf,
                    path_len: path.len(),
                    fs,
                });
                self.mount_count += 1;
                return Ok(());
            }
        }
        Err(VfsError::MountFull)
    }

    /// Resolve a path to its mount point and the relative path within.
    fn resolve<'a>(&'a self, path: &'a str) -> Option<(&'a MountPoint, &'a str)> {
        let mut best: Option<(&MountPoint, &str)> = None;
        let mut best_len = 0;

        for mp in self.mounts.iter().flatten() {
            let mp_path = mp.path_str();
            if path.starts_with(mp_path) && mp_path.len() >= best_len {
                let relative = &path[mp_path.len()..];
                let relative = if relative.is_empty() { "/" } else { relative };
                best = Some((mp, relative));
                best_len = mp_path.len();
            }
        }
        best
    }

    /// Stat a path.
    pub fn stat(&self, path: &str) -> Result<NodeInfo, VfsError> {
        let (mp, rel) = self.resolve(path).ok_or(VfsError::NotMounted)?;
        let fs = unsafe { &*mp.fs };
        fs.stat(rel)
    }

    /// Read from a file.
    pub fn read(&self, path: &str, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let (mp, rel) = self.resolve(path).ok_or(VfsError::NotMounted)?;
        let fs = unsafe { &*mp.fs };
        fs.read(rel, offset, buf)
    }

    /// Write to a file.
    pub fn write(&self, path: &str, offset: usize, data: &[u8]) -> Result<usize, VfsError> {
        let (mp, rel) = self.resolve(path).ok_or(VfsError::NotMounted)?;
        let fs = unsafe { &mut *mp.fs };
        fs.write(rel, offset, data)
    }

    /// Create a file or directory.
    pub fn create(&self, path: &str, node_type: NodeType) -> Result<(), VfsError> {
        let (mp, rel) = self.resolve(path).ok_or(VfsError::NotMounted)?;
        let fs = unsafe { &mut *mp.fs };
        fs.create(rel, node_type)
    }

    /// List a directory.
    pub fn readdir(&self, path: &str, entries: &mut [DirEntry]) -> Result<usize, VfsError> {
        let (mp, rel) = self.resolve(path).ok_or(VfsError::NotMounted)?;
        let fs = unsafe { &*mp.fs };
        fs.readdir(rel, entries)
    }

    /// Remove a node.
    pub fn remove(&self, path: &str) -> Result<(), VfsError> {
        let (mp, rel) = self.resolve(path).ok_or(VfsError::NotMounted)?;
        let fs = unsafe { &mut *mp.fs };
        fs.remove(rel)
    }

    /// Number of active mounts.
    pub fn mount_count(&self) -> usize {
        self.mount_count
    }
}

/// Global VFS manager.
pub static VFS: Mutex<VfsManager> = Mutex::new(VfsManager::new());
