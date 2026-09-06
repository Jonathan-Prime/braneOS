//! Physically contiguous DMA memory backed by the boot frame allocator.
//!
//! The bootloader maps physical RAM at a fixed virtual offset. DMA regions
//! retain both addresses so drivers can program hardware with a physical base
//! while accessing the same bytes through the direct map.

#![allow(dead_code)]

use crate::memory::frame_allocator::{BitmapFrameAllocator, FRAME_SIZE};

/// Legacy 32-bit PCI devices cannot address memory at or above 4 GiB.
pub const LEGACY_DMA_LIMIT: u64 = 0x1_0000_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaError {
    InvalidLayout,
    OutOfMemory,
    AddressOverflow,
}

/// A page-aligned, physically contiguous DMA allocation.
///
/// Regions are reserved for the kernel lifetime. Returning frames to the boot
/// allocator will be added together with driver hot-unplug support.
#[derive(Debug, Clone, Copy)]
pub struct DmaRegion {
    physical_start: u64,
    virtual_start: usize,
    len: usize,
}

impl DmaRegion {
    /// Reserve and clear enough complete frames for `bytes`.
    pub fn allocate(
        frame_allocator: &mut BitmapFrameAllocator,
        physical_memory_offset: u64,
        bytes: usize,
        alignment: usize,
    ) -> Result<Self, DmaError> {
        if bytes == 0
            || alignment < FRAME_SIZE
            || !alignment.is_power_of_two()
            || !alignment.is_multiple_of(FRAME_SIZE)
        {
            return Err(DmaError::InvalidLayout);
        }

        let pages = bytes.div_ceil(FRAME_SIZE);
        let alignment_frames = alignment / FRAME_SIZE;
        let physical_start = frame_allocator
            .allocate_contiguous_below(pages, LEGACY_DMA_LIMIT, alignment_frames)
            .ok_or(DmaError::OutOfMemory)?;
        let len = pages
            .checked_mul(FRAME_SIZE)
            .ok_or(DmaError::AddressOverflow)?;
        let virtual_address = physical_memory_offset
            .checked_add(physical_start)
            .ok_or(DmaError::AddressOverflow)?;
        virtual_address
            .checked_add(len as u64)
            .ok_or(DmaError::AddressOverflow)?;
        let virtual_start =
            usize::try_from(virtual_address).map_err(|_| DmaError::AddressOverflow)?;

        // The physical-memory direct map is writable and covers every usable
        // frame handed out by the boot allocator.
        unsafe {
            core::ptr::write_bytes(virtual_start as *mut u8, 0, len);
        }

        Ok(Self {
            physical_start,
            virtual_start,
            len,
        })
    }

    pub const fn physical_start(self) -> u64 {
        self.physical_start
    }

    pub const fn virtual_start(self) -> usize {
        self.virtual_start
    }

    pub const fn len(self) -> usize {
        self.len
    }

    pub const fn is_empty(self) -> bool {
        self.len == 0
    }

    pub fn physical_at(self, offset: usize) -> Option<u64> {
        if offset >= self.len {
            return None;
        }
        self.physical_start.checked_add(offset as u64)
    }

    pub fn pointer_at<T>(self, offset: usize) -> Option<*mut T> {
        let end = offset.checked_add(core::mem::size_of::<T>())?;
        if end > self.len {
            return None;
        }
        self.virtual_start
            .checked_add(offset)
            .map(|address| address as *mut T)
    }
}
