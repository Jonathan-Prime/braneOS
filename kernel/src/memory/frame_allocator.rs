#![allow(dead_code)]
// ============================================================
// Brane OS Kernel — Frame Allocator (Bitmap-based)
// ============================================================
//
// Manages physical memory frames (4 KiB pages) using a bitmap.
// Each bit represents one physical frame:
//   0 = free, 1 = allocated.
//
// At boot, the allocator is initialized with the memory map
// provided by the bootloader, marking usable frames as free
// and everything else as reserved.
//
// This is a Phase 2 component.
// Spec reference: ARCHITECTURE.md §5.2.1
// ============================================================

use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

/// Size of a physical page frame (4 KiB).
pub const FRAME_SIZE: usize = 4096;

/// Maximum physical memory supported (1 GiB for now).
/// This gives us 262,144 frames = 32 KiB bitmap.
const MAX_MEMORY: usize = 1024 * 1024 * 1024; // 1 GiB
const MAX_FRAMES: usize = MAX_MEMORY / FRAME_SIZE;
const BITMAP_SIZE: usize = MAX_FRAMES / 8;

static mut ALLOCATOR_BITMAP: [u8; BITMAP_SIZE] = [0xFF; BITMAP_SIZE];

/// Bitmap-based physical frame allocator.
///
/// Tracks which 4 KiB physical frames are free or allocated.
pub struct BitmapFrameAllocator {
    total_frames: usize,
    free_frames: usize,
}

impl Default for BitmapFrameAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl BitmapFrameAllocator {
    /// Create a new allocator with all frames marked as used.
    ///
    /// Call `mark_region_free` to mark usable memory regions.
    pub fn new() -> Self {
        Self {
            total_frames: MAX_FRAMES,
            free_frames: 0,
        }
    }

    /// Mark a range of physical frames as free (usable).
    ///
    /// `start` and `end` are physical addresses.
    /// Both are aligned down/up to frame boundaries.
    pub fn mark_region_free(&mut self, start: u64, end: u64) {
        let start_frame = (start as usize) / FRAME_SIZE;
        let end_frame = (end as usize) / FRAME_SIZE;

        for frame in start_frame..end_frame {
            if frame < MAX_FRAMES && self.is_used(frame) {
                self.set_free(frame);
                self.free_frames += 1;
            }
        }
    }

    /// Mark a range of physical frames as used (reserved).
    pub fn mark_region_used(&mut self, start: u64, end: u64) {
        let start_frame = (start as usize) / FRAME_SIZE;
        let end_frame = (end as usize).div_ceil(FRAME_SIZE);

        for frame in start_frame..end_frame {
            if frame < MAX_FRAMES && !self.is_used(frame) {
                self.set_used(frame);
                self.free_frames -= 1;
            }
        }
    }

    /// Allocate a single physical frame.
    ///
    /// Returns the physical address of the frame, or `None` if OOM.
    pub fn allocate(&mut self) -> Option<u64> {
        for byte_idx in 0..BITMAP_SIZE {
            if unsafe { ALLOCATOR_BITMAP[byte_idx] } != 0xFF {
                // This byte has at least one free bit
                for bit in 0..8 {
                    let frame = byte_idx * 8 + bit;
                    if frame < MAX_FRAMES && !self.is_used(frame) {
                        self.set_used(frame);
                        self.free_frames -= 1;
                        return Some((frame * FRAME_SIZE) as u64);
                    }
                }
            }
        }
        None // Out of memory
    }

    /// Deallocate a physical frame by its address.
    pub fn deallocate(&mut self, addr: u64) {
        let frame = (addr as usize) / FRAME_SIZE;
        if frame < MAX_FRAMES && self.is_used(frame) {
            self.set_free(frame);
            self.free_frames += 1;
        }
    }

    /// Number of free frames available.
    pub fn free_count(&self) -> usize {
        self.free_frames
    }

    /// Total frames tracked by the allocator.
    pub fn total_count(&self) -> usize {
        self.total_frames
    }

    // --- Private helpers ---

    fn is_used(&self, frame: usize) -> bool {
        let byte = frame / 8;
        let bit = frame % 8;
        unsafe { ALLOCATOR_BITMAP[byte] & (1 << bit) != 0 }
    }

    fn set_used(&mut self, frame: usize) {
        let byte = frame / 8;
        let bit = frame % 8;
        unsafe { ALLOCATOR_BITMAP[byte] |= 1 << bit };
    }

    fn set_free(&mut self, frame: usize) {
        let byte = frame / 8;
        let bit = frame % 8;
        unsafe { ALLOCATOR_BITMAP[byte] &= !(1 << bit) };
    }
}

// -----------------------------------------------------------------------
// Implement x86_64's FrameAllocator trait
// -----------------------------------------------------------------------

unsafe impl FrameAllocator<Size4KiB> for BitmapFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        self.allocate()
            .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}
