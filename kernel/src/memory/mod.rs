// ============================================================
// Brane OS Kernel — Memory Manager
// ============================================================
//
// Provides the memory subsystem:
//   - Frame allocator (physical memory)
//   - Heap allocator (kernel heap via linked_list_allocator)
//
// Spec reference: ARCHITECTURE.md §5.2.1 (Memory Manager)
// ============================================================

pub mod frame_allocator;
pub mod heap;
pub mod paging;
