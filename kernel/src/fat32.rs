#![allow(dead_code)]
// ============================================================
// Brane OS Kernel — FAT32 Stub
// ============================================================
//
// A minimalist, read-only FAT32 parser for Phase 10 boot.
// It detects and parses the boot sector and basic directory
// structures. Currently acts as a stub for the VFS.
//
// Spec reference: ROADMAP.md Fase 10 (Estabilización)
// ============================================================

use crate::vfs::{DirEntry, FileSystem, NodeInfo, NodeType, VfsError};

/// Standard size of a disk sector.
pub const SECTOR_SIZE: usize = 512;

/// A parsed Master Boot Record (MBR) partition entry.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct PartitionEntry {
    pub status: u8,
    pub start_chs: [u8; 3],
    pub partition_type: u8,
    pub end_chs: [u8; 3],
    pub start_lba: u32,
    pub sector_count: u32,
}

impl PartitionEntry {
    /// Parse a partition entry from a 16-byte slice of the MBR.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 16 || data[4] == 0x00 {
            return None; // empty or invalid length
        }
        Some(Self {
            status: data[0],
            start_chs: [data[1], data[2], data[3]],
            partition_type: data[4],
            end_chs: [data[5], data[6], data[7]],
            start_lba: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            sector_count: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        })
    }
}

/// A parsed FAT32 Boot Sector (Volume ID).
#[derive(Debug, Clone)]
pub struct Fat32BootSector {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub fat_count: u8,
    pub root_dir_entries: u16,
    pub total_sectors_16: u16,
    pub media_descriptor: u8,
    pub sectors_per_fat_16: u16,
    pub sectors_per_track: u16,
    pub heads: u16,
    pub hidden_sectors: u32,
    pub total_sectors_32: u32,

    // FAT32 Extended Boot Record
    pub sectors_per_fat_32: u32,
    pub ext_flags: u16,
    pub fs_version: u16,
    pub root_cluster: u32,
    pub fs_info_sector: u16,
    pub backup_boot_sector: u16,
    pub drive_number: u8,
    pub boot_signature: u8,
    pub volume_id: u32,
    pub volume_label: [u8; 11],
    pub fs_type_label: [u8; 8],
}

impl Fat32BootSector {
    /// Parse a FAT32 boot sector from a 512-byte sector slice.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 512 {
            return None;
        }

        // Check boot signature at end of sector (0x55 0xAA)
        if data[510] != 0x55 || data[511] != 0xAA {
            return None;
        }

        let mut vol_label = [0u8; 11];
        vol_label.copy_from_slice(&data[71..82]);

        let mut fs_type = [0u8; 8];
        fs_type.copy_from_slice(&data[82..90]);

        Some(Self {
            bytes_per_sector: u16::from_le_bytes([data[11], data[12]]),
            sectors_per_cluster: data[13],
            reserved_sectors: u16::from_le_bytes([data[14], data[15]]),
            fat_count: data[16],
            root_dir_entries: u16::from_le_bytes([data[17], data[18]]),
            total_sectors_16: u16::from_le_bytes([data[19], data[20]]),
            media_descriptor: data[21],
            sectors_per_fat_16: u16::from_le_bytes([data[22], data[23]]),
            sectors_per_track: u16::from_le_bytes([data[24], data[25]]),
            heads: u16::from_le_bytes([data[26], data[27]]),
            hidden_sectors: u32::from_le_bytes([data[28], data[29], data[30], data[31]]),
            total_sectors_32: u32::from_le_bytes([data[32], data[33], data[34], data[35]]),

            // FAT32 Extended block
            sectors_per_fat_32: u32::from_le_bytes([data[36], data[37], data[38], data[39]]),
            ext_flags: u16::from_le_bytes([data[40], data[41]]),
            fs_version: u16::from_le_bytes([data[42], data[43]]),
            root_cluster: u32::from_le_bytes([data[44], data[45], data[46], data[47]]),
            fs_info_sector: u16::from_le_bytes([data[48], data[49]]),
            backup_boot_sector: u16::from_le_bytes([data[50], data[51]]),
            drive_number: data[64],
            boot_signature: data[66],
            volume_id: u32::from_le_bytes([data[67], data[68], data[69], data[70]]),
            volume_label: vol_label,
            fs_type_label: fs_type,
        })
    }
}

// -----------------------------------------------------------------------
// VFS FileSystem trait implementation (Stub)
// -----------------------------------------------------------------------

/// A minimalist stub implementation of a FAT32 filesystem for the VFS.
pub struct Fat32Fs {
    boot_sector: Fat32BootSector,
    partition_lba: u32,
    ready: bool,
}

impl Fat32Fs {
    /// Initialize a new FAT32 stub from a known boot sector.
    pub fn new(boot_sector: Fat32BootSector, partition_lba: u32) -> Self {
        Self {
            boot_sector,
            partition_lba,
            ready: true,
        }
    }

    /// Read volume label as string.
    pub fn volume_label(&self) -> &str {
        core::str::from_utf8(&self.boot_sector.volume_label)
            .unwrap_or("NO NAME")
            .trim()
    }
}

impl FileSystem for Fat32Fs {
    fn stat(&self, _path: &str) -> Result<NodeInfo, VfsError> {
        if !self.ready {
            return Err(VfsError::IoError);
        }
        // STUB: return mocked root dir
        Err(VfsError::NotFound)
    }

    fn read(&self, _path: &str, _offset: usize, _buf: &mut [u8]) -> Result<usize, VfsError> {
        Err(VfsError::IoError) // Read not implemented yet
    }

    fn write(&mut self, _path: &str, _offset: usize, _data: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::IoError) // Read-only FS
    }

    fn create(&mut self, _path: &str, _node_type: NodeType) -> Result<(), VfsError> {
        Err(VfsError::IoError) // Read-only FS
    }

    fn readdir(&self, path: &str, _entries: &mut [DirEntry]) -> Result<usize, VfsError> {
        if path != "/" {
            return Err(VfsError::NotFound);
        }
        // STUB: Empty directory
        Ok(0)
    }

    fn remove(&mut self, _path: &str) -> Result<(), VfsError> {
        Err(VfsError::IoError) // Read-only FS
    }

    fn fs_name(&self) -> &str {
        "fat32"
    }
}
