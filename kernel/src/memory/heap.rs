#![allow(dead_code)]
// ============================================================
// Brane OS Kernel — Heap Allocator
// ============================================================
//
// Provides a global kernel heap using `linked_list_allocator`.
// The heap is mapped into the kernel's virtual address space
// and initialized during early boot.
//
// Heap region: HEAP_START..HEAP_START+HEAP_SIZE
// Default: 1 MiB heap at a fixed virtual address.
//
// Spec reference: ARCHITECTURE.md §5.2.1
// ============================================================

use linked_list_allocator::LockedHeap;
use x86_64::structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB};
use x86_64::VirtAddr;

/// Start address of the kernel heap (virtual).
pub const HEAP_START: u64 = 0x_4444_4444_0000;

/// Size of the kernel heap (1 MiB).
pub const HEAP_SIZE: u64 = 1024 * 1024;

/// Global allocator used by Rust's `alloc` crate.
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Initialize the kernel heap.
///
/// Maps `HEAP_SIZE / 4096` physical frames to the heap region
/// and initializes the linked-list allocator.
///
/// # Safety
/// Must be called only once, after page tables and the frame
/// allocator are initialized.
pub fn init(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), &'static str> {
    let heap_start = VirtAddr::new(HEAP_START);
    let heap_end = heap_start + HEAP_SIZE;
    let heap_start_page = Page::containing_address(heap_start);
    let heap_end_page = Page::containing_address(heap_end - 1u64);

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    // Map each page in the heap region to a physical frame
    let mut page = heap_start_page;
    while page <= heap_end_page {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or("heap init: frame allocation failed")?;

        unsafe {
            mapper
                .map_to(page, frame, flags, frame_allocator)
                .map_err(|_| "heap init: page mapping failed")?
                .flush();
        }

        page = Page::containing_address(page.start_address() + 4096u64);
    }

    // Initialize the allocator with the heap region
    unsafe {
        ALLOCATOR
            .lock()
            .init(heap_start.as_mut_ptr(), HEAP_SIZE as usize);
    }

    Ok(())
}
