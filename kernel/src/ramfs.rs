// ============================================================
// Brane OS Kernel — RamFS (In-Memory Filesystem)
// ============================================================
//
// A simple in-memory filesystem for early boot and testing.
// Data is stored in fixed-size inode slots with inline content.
//
// Spec reference: ARCHITECTURE.md §5.3 (planned)
// ============================================================

use crate::vfs::{DirEntry, FileSystem, NodeInfo, NodeType, VfsError, MAX_NAME};
use spin::Mutex;

/// Maximum inodes (files + directories).
const MAX_INODES: usize = 256;

/// Maximum inline file content.
const MAX_FILE_SIZE: usize = 4096;

/// Maximum children per directory.
const MAX_CHILDREN: usize = 32;

// -----------------------------------------------------------------------
// Initramfs embedded logic
// -----------------------------------------------------------------------

const MOTD_CONTENT: &[u8] = b"Welcome to Brane OS v1.0-alpha\n";
const INIT_SCRIPT_CONTENT: &[u8] = b"#!/bin/brsh\necho 'Starting Brane OS Services...'\nbrane status\nlsmod\n";

// -----------------------------------------------------------------------
// Inode
// -----------------------------------------------------------------------

struct Inode {
    used: bool,
    node_type: NodeType,
    name: [u8; MAX_NAME],
    name_len: usize,
    parent: u16, // inode index of parent dir (0 = root)
    // File data
    data: [u8; MAX_FILE_SIZE],
    data_len: usize,
    // Directory children (inode indices)
    children: [u16; MAX_CHILDREN],
    child_count: usize,
}

impl Inode {
    const fn empty() -> Self {
        Self {
            used: false,
            node_type: NodeType::File,
            name: [0; MAX_NAME],
            name_len: 0,
            parent: 0,
            data: [0; MAX_FILE_SIZE],
            data_len: 0,
            children: [0; MAX_CHILDREN],
            child_count: 0,
        }
    }

    fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }

    fn set_name(&mut self, name: &str) {
        let len = name.len().min(MAX_NAME);
        self.name[..len].copy_from_slice(&name.as_bytes()[..len]);
        self.name_len = len;
    }
}

// -----------------------------------------------------------------------
// RamFS
// -----------------------------------------------------------------------

pub struct RamFs {
    inodes: [Inode; MAX_INODES],
    inode_count: usize,
}

impl Default for RamFs {
    fn default() -> Self {
        Self::new()
    }
}

impl RamFs {
    /// Create a new RamFS with a root directory at inode 0.
    pub fn new() -> Self {
        let mut fs = Self {
            inodes: {
                // Can't use [Inode::empty(); MAX_INODES] because Inode is too large.
                // Use unsafe zeroed init (all zeros = empty() equivalent).
                unsafe { core::mem::zeroed() }
            },
            inode_count: 1,
        };
        // Set up root directory (inode 0)
        fs.inodes[0].used = true;
        fs.inodes[0].node_type = NodeType::Directory;
        fs.inodes[0].set_name("/");
        fs.inodes[0].parent = 0;
        fs
    }

    /// Allocate a new inode, returns its index.
    fn alloc_inode(&mut self) -> Result<usize, VfsError> {
        for i in 1..MAX_INODES {
            if !self.inodes[i].used {
                self.inodes[i] = Inode::empty();
                self.inodes[i].used = true;
                self.inode_count += 1;
                return Ok(i);
            }
        }
        Err(VfsError::NoSpace)
    }

    /// Resolve a path to an inode index, starting from root (inode 0).
    fn resolve_path(&self, path: &str) -> Result<usize, VfsError> {
        if path == "/" || path.is_empty() {
            return Ok(0);
        }

        let path = path.trim_start_matches('/');
        let mut current = 0usize; // start at root

        for component in path.split('/') {
            if component.is_empty() {
                continue;
            }

            let dir = &self.inodes[current];
            if dir.node_type != NodeType::Directory {
                return Err(VfsError::NotADirectory);
            }

            let mut found = false;
            for i in 0..dir.child_count {
                let child_idx = dir.children[i] as usize;
                if child_idx < MAX_INODES
                    && self.inodes[child_idx].used
                    && self.inodes[child_idx].name_str() == component
                {
                    current = child_idx;
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(VfsError::NotFound);
            }
        }

        Ok(current)
    }

    /// Split a path into (parent_path, name).
    fn split_path(path: &str) -> (&str, &str) {
        let path = path.trim_end_matches('/');
        if let Some(pos) = path.rfind('/') {
            let parent = &path[..pos];
            let name = &path[pos + 1..];
            if parent.is_empty() {
                ("/", name)
            } else {
                (parent, name)
            }
        } else {
            ("/", path)
        }
    }

    /// Number of used inodes.
    pub fn inode_count(&self) -> usize {
        self.inode_count
    }
}

impl FileSystem for RamFs {
    fn stat(&self, path: &str) -> Result<NodeInfo, VfsError> {
        let idx = self.resolve_path(path)?;
        let inode = &self.inodes[idx];
        let mut info = NodeInfo {
            name: [0; MAX_NAME],
            name_len: inode.name_len,
            node_type: inode.node_type,
            size: inode.data_len,
            inode: idx as u64,
        };
        info.name[..inode.name_len].copy_from_slice(&inode.name[..inode.name_len]);
        Ok(info)
    }

    fn read(&self, path: &str, offset: usize, buf: &mut [u8]) -> Result<usize, VfsError> {
        let idx = self.resolve_path(path)?;
        let inode = &self.inodes[idx];
        if inode.node_type != NodeType::File {
            return Err(VfsError::NotAFile);
        }
        if offset >= inode.data_len {
            return Ok(0);
        }
        let available = inode.data_len - offset;
        let to_read = buf.len().min(available);
        buf[..to_read].copy_from_slice(&inode.data[offset..offset + to_read]);
        Ok(to_read)
    }

    fn write(&mut self, path: &str, offset: usize, data: &[u8]) -> Result<usize, VfsError> {
        let idx = self.resolve_path(path)?;
        let inode = &mut self.inodes[idx];
        if inode.node_type != NodeType::File {
            return Err(VfsError::NotAFile);
        }
        let end = offset + data.len();
        if end > MAX_FILE_SIZE {
            return Err(VfsError::NoSpace);
        }
        inode.data[offset..end].copy_from_slice(data);
        if end > inode.data_len {
            inode.data_len = end;
        }
        Ok(data.len())
    }

    fn create(&mut self, path: &str, node_type: NodeType) -> Result<(), VfsError> {
        // Check if already exists
        if self.resolve_path(path).is_ok() {
            return Err(VfsError::AlreadyExists);
        }

        let (parent_path, name) = Self::split_path(path);
        let parent_idx = self.resolve_path(parent_path)?;

        if self.inodes[parent_idx].node_type != NodeType::Directory {
            return Err(VfsError::NotADirectory);
        }
        if self.inodes[parent_idx].child_count >= MAX_CHILDREN {
            return Err(VfsError::NoSpace);
        }

        let new_idx = self.alloc_inode()?;
        self.inodes[new_idx].node_type = node_type;
        self.inodes[new_idx].set_name(name);
        self.inodes[new_idx].parent = parent_idx as u16;

        let cc = self.inodes[parent_idx].child_count;
        self.inodes[parent_idx].children[cc] = new_idx as u16;
        self.inodes[parent_idx].child_count += 1;

        Ok(())
    }

    fn readdir(&self, path: &str, entries: &mut [DirEntry]) -> Result<usize, VfsError> {
        let idx = self.resolve_path(path)?;
        let inode = &self.inodes[idx];
        if inode.node_type != NodeType::Directory {
            return Err(VfsError::NotADirectory);
        }

        let count = inode.child_count.min(entries.len());
        for (i, entry) in entries.iter_mut().enumerate().take(count) {
            let child_idx = inode.children[i] as usize;
            let child = &self.inodes[child_idx];
            *entry = DirEntry {
                name: child.name,
                name_len: child.name_len,
                node_type: child.node_type,
            };
        }
        Ok(count)
    }

    fn remove(&mut self, path: &str) -> Result<(), VfsError> {
        if path == "/" || path.is_empty() {
            return Err(VfsError::InvalidPath); // can't remove root
        }

        let idx = self.resolve_path(path)?;
        let inode = &self.inodes[idx];

        // Don't remove non-empty directories
        if inode.node_type == NodeType::Directory && inode.child_count > 0 {
            return Err(VfsError::IoError);
        }

        let parent_idx = inode.parent as usize;

        // Remove from parent's children list
        let parent = &mut self.inodes[parent_idx];
        if let Some(pos) = parent.children[..parent.child_count]
            .iter()
            .position(|&c| c as usize == idx)
        {
            // Shift remaining children
            for j in pos..parent.child_count - 1 {
                parent.children[j] = parent.children[j + 1];
            }
            parent.child_count -= 1;
        }

        // Free the inode
        self.inodes[idx].used = false;
        self.inode_count -= 1;
        Ok(())
    }

    fn fs_name(&self) -> &str {
        "ramfs"
    }
}

/// Global RamFS instance (mounted at /).
pub static RAMFS: Mutex<RamFs> = Mutex::new(RamFs {
    inodes: {
        // Safety: all-zeroes is a valid representation for our Inode array
        // (used=false for all entries). We initialize root in init().
        unsafe { core::mem::zeroed() }
    },
    inode_count: 0,
});

/// Initialize the global RamFS with root directory and standard dirs.
pub fn init() {
    let mut fs = RAMFS.lock();
    // Set up root (inode 0)
    fs.inodes[0].used = true;
    fs.inodes[0].node_type = NodeType::Directory;
    fs.inodes[0].set_name("/");
    fs.inodes[0].parent = 0;
    fs.inode_count = 1;

    // Create standard directories
    fs.create("/dev", NodeType::Directory).ok();
    fs.create("/proc", NodeType::Directory).ok();
    fs.create("/tmp", NodeType::Directory).ok();
    fs.create("/etc", NodeType::Directory).ok();

    // Populate initramfs files
    fs.create("/etc/motd", NodeType::File).ok();
    let _ = fs.write("/etc/motd", 0, MOTD_CONTENT);

    fs.create("/etc/init.sh", NodeType::File).ok();
    let _ = fs.write("/etc/init.sh", 0, INIT_SCRIPT_CONTENT);
}
