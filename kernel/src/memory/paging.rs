#![allow(dead_code)]
// ============================================================
// Brane OS Kernel — Page Table Manager
// ============================================================
//
// Manages virtual-to-physical address mappings using x86_64
// 4-level page tables.
//
// Provides:
//   - Active page table access (via physical memory offset)
//   - Map / unmap operations
//   - Boot-info aware initialization
//
// Spec reference: ARCHITECTURE.md §5.2.3
// ============================================================

use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

// -----------------------------------------------------------------------
// Initialization
// -----------------------------------------------------------------------

/// Initialize an `OffsetPageTable` from the currently active level-4 table.
///
/// # Safety
/// - `physical_memory_offset` must be correct (all physical memory is
///   mapped at this virtual offset by the bootloader).
/// - Must only be called once to avoid aliasing `&mut` references.
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = active_level_4_table(physical_memory_offset);
    unsafe { OffsetPageTable::new(level_4_table, physical_memory_offset) }
}

/// Get a mutable reference to the active level-4 page table.
unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    let (level_4_frame, _) = Cr3::read();
    let phys = level_4_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();
    unsafe { &mut *page_table_ptr }
}

// -----------------------------------------------------------------------
// Mapping helpers
// -----------------------------------------------------------------------

/// Map a virtual page to a physical frame.
///
/// Uses the provided frame allocator for any intermediate page table frames.
pub fn map_page(
    mapper: &mut OffsetPageTable,
    page: Page<Size4KiB>,
    frame: PhysFrame<Size4KiB>,
    flags: PageTableFlags,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), &'static str> {
    unsafe {
        mapper
            .map_to(page, frame, flags, frame_allocator)
            .map_err(|_| "map_to failed")?
            .flush();
    }
    Ok(())
}

/// Map a range of virtual pages to physical frames (identity or offset).
pub fn map_range(
    mapper: &mut OffsetPageTable,
    start_page: Page<Size4KiB>,
    start_frame: PhysFrame<Size4KiB>,
    count: u64,
    flags: PageTableFlags,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), &'static str> {
    for i in 0..count {
        let page = Page::containing_address(start_page.start_address() + i * 4096);
        let frame = PhysFrame::containing_address(start_frame.start_address() + i * 4096);
        map_page(mapper, page, frame, flags, frame_allocator)?;
    }
    Ok(())
}

/// Unmap a virtual page and return the physical frame it was mapped to.
pub fn unmap_page(
    mapper: &mut OffsetPageTable,
    page: Page<Size4KiB>,
) -> Result<PhysFrame<Size4KiB>, &'static str> {
    let (frame, flush) = mapper.unmap(page).map_err(|_| "unmap failed")?;
    flush.flush();
    Ok(frame)
}

// -----------------------------------------------------------------------
// Info / debugging
// -----------------------------------------------------------------------

/// Translate a virtual address to a physical address.
pub fn translate_addr(mapper: &OffsetPageTable, addr: VirtAddr) -> Option<PhysAddr> {
    use x86_64::structures::paging::Translate;
    mapper.translate_addr(addr)
}
