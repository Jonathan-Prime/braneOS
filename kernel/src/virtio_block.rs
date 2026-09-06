//! Synchronous legacy virtio-blk transport.
//!
//! The driver uses one polling virtqueue and one 512-byte bounce buffer. Queue
//! state is protected by a spinlock, so the device-independent block layer can
//! safely call it from different CPUs while only one request is in flight.

#![allow(dead_code)]

use core::sync::atomic::{fence, Ordering};

use spin::{Mutex, Once};
use x86_64::instructions::port::Port;

use crate::block::{BlockDevice, BlockError};
use crate::dma::{DmaError, DmaRegion};
use crate::memory::frame_allocator::{BitmapFrameAllocator, FRAME_SIZE};
use crate::pci::{self, PciDevice};
use crate::virtio::{legacy_io_base, status, virtio_reg, VirtioInitError};

const SECTOR_SIZE: usize = 512;
const QUEUE_INDEX: u16 = 0;
const VIRTQ_ALIGNMENT: usize = FRAME_SIZE;
const MAX_POLL_SPINS: usize = 10_000_000;

const VIRTIO_BLK_F_RO: u32 = 1 << 5;
const VIRTIO_BLK_F_FLUSH: u32 = 1 << 9;
const SUPPORTED_FEATURES: u32 = VIRTIO_BLK_F_RO | VIRTIO_BLK_F_FLUSH;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_T_FLUSH: u32 = 4;

const VIRTIO_BLK_S_OK: u8 = 0;
const VIRTIO_BLK_S_UNSUPP: u8 = 2;

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;
const VIRTQ_AVAIL_F_NO_INTERRUPT: u16 = 1;

const REQUEST_HEADER_OFFSET: usize = 0;
const REQUEST_DATA_OFFSET: usize = 16;
const REQUEST_STATUS_OFFSET: usize = REQUEST_DATA_OFFSET + SECTOR_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtioBlockInitError {
    Transport(VirtioInitError),
    Dma(DmaError),
    IoPortRange,
    QueueUnavailable,
    InvalidQueueSize,
    QueueAddressOutOfRange,
    FeaturesRejected,
    InvalidCapacity,
    AlreadyInitialized,
}

impl From<VirtioInitError> for VirtioBlockInitError {
    fn from(error: VirtioInitError) -> Self {
        Self::Transport(error)
    }
}

impl From<DmaError> for VirtioBlockInitError {
    fn from(error: DmaError) -> Self {
        Self::Dma(error)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VirtqDescriptor {
    address: u64,
    length: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct VirtqUsedElement {
    id: u32,
    length: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct VirtioBlockRequestHeader {
    request_type: u32,
    reserved: u32,
    sector: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VirtQueueLayout {
    size: u16,
    available_offset: usize,
    used_offset: usize,
    total_bytes: usize,
}

impl VirtQueueLayout {
    fn new(size: u16) -> Result<Self, VirtioBlockInitError> {
        if size < 3 || !size.is_power_of_two() {
            return Err(VirtioBlockInitError::InvalidQueueSize);
        }
        let entries = size as usize;
        let descriptor_bytes = entries
            .checked_mul(core::mem::size_of::<VirtqDescriptor>())
            .ok_or(VirtioBlockInitError::InvalidQueueSize)?;
        let available_bytes = 4usize
            .checked_add(
                entries
                    .checked_mul(core::mem::size_of::<u16>())
                    .ok_or(VirtioBlockInitError::InvalidQueueSize)?,
            )
            .ok_or(VirtioBlockInitError::InvalidQueueSize)?;
        let used_offset = align_up(
            descriptor_bytes
                .checked_add(available_bytes)
                .ok_or(VirtioBlockInitError::InvalidQueueSize)?,
            VIRTQ_ALIGNMENT,
        )
        .ok_or(VirtioBlockInitError::InvalidQueueSize)?;
        let used_bytes = 4usize
            .checked_add(
                entries
                    .checked_mul(core::mem::size_of::<VirtqUsedElement>())
                    .ok_or(VirtioBlockInitError::InvalidQueueSize)?,
            )
            .ok_or(VirtioBlockInitError::InvalidQueueSize)?;
        let total_bytes = used_offset
            .checked_add(used_bytes)
            .ok_or(VirtioBlockInitError::InvalidQueueSize)?;

        Ok(Self {
            size,
            available_offset: descriptor_bytes,
            used_offset,
            total_bytes,
        })
    }
}

const fn align_up(value: usize, alignment: usize) -> Option<usize> {
    let mask = alignment - 1;
    match value.checked_add(mask) {
        Some(sum) => Some(sum & !mask),
        None => None,
    }
}

struct VirtioBlockState {
    io_base: u16,
    queue: DmaRegion,
    request: DmaRegion,
    layout: VirtQueueLayout,
    available_index: u16,
    last_used_index: u16,
}

impl VirtioBlockState {
    fn descriptor_pointer(&self, index: usize) -> *mut VirtqDescriptor {
        debug_assert!(index < self.layout.size as usize);
        self.queue
            .pointer_at(index * core::mem::size_of::<VirtqDescriptor>())
            .expect("virtqueue descriptor must fit DMA region")
    }

    fn available_index_pointer(&self) -> *mut u16 {
        self.queue
            .pointer_at(self.layout.available_offset + 2)
            .expect("virtqueue available index must fit DMA region")
    }

    fn available_ring_pointer(&self, slot: usize) -> *mut u16 {
        self.queue
            .pointer_at(self.layout.available_offset + 4 + slot * 2)
            .expect("virtqueue available entry must fit DMA region")
    }

    fn used_index_pointer(&self) -> *mut u16 {
        self.queue
            .pointer_at(self.layout.used_offset + 2)
            .expect("virtqueue used index must fit DMA region")
    }

    fn used_element_pointer(&self, slot: usize) -> *mut VirtqUsedElement {
        self.queue
            .pointer_at(
                self.layout.used_offset + 4 + slot * core::mem::size_of::<VirtqUsedElement>(),
            )
            .expect("virtqueue used element must fit DMA region")
    }

    fn submit_read(&mut self, sector: u64, output: &mut [u8]) -> Result<(), BlockError> {
        debug_assert_eq!(output.len(), SECTOR_SIZE);
        unsafe {
            core::ptr::write_bytes(
                self.request.virtual_start() as *mut u8,
                0,
                self.request.len(),
            );
        }
        self.prepare_rw_descriptors(VIRTIO_BLK_T_IN, sector, true)?;
        self.publish_and_wait()?;
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.request
                    .virtual_start()
                    .wrapping_add(REQUEST_DATA_OFFSET) as *const u8,
                output.as_mut_ptr(),
                SECTOR_SIZE,
            );
        }
        Ok(())
    }

    fn submit_write(&mut self, sector: u64, input: &[u8]) -> Result<(), BlockError> {
        debug_assert_eq!(input.len(), SECTOR_SIZE);
        unsafe {
            core::ptr::copy_nonoverlapping(
                input.as_ptr(),
                self.request
                    .virtual_start()
                    .wrapping_add(REQUEST_DATA_OFFSET) as *mut u8,
                SECTOR_SIZE,
            );
        }
        self.prepare_rw_descriptors(VIRTIO_BLK_T_OUT, sector, false)?;
        self.publish_and_wait()
    }

    fn submit_flush(&mut self) -> Result<(), BlockError> {
        self.write_request_header(VIRTIO_BLK_T_FLUSH, 0);
        self.reset_request_status();

        let header = VirtqDescriptor {
            address: self.request_physical(REQUEST_HEADER_OFFSET)?,
            length: core::mem::size_of::<VirtioBlockRequestHeader>() as u32,
            flags: VIRTQ_DESC_F_NEXT,
            next: 2,
        };
        let status = VirtqDescriptor {
            address: self.request_physical(REQUEST_STATUS_OFFSET)?,
            length: 1,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        };
        unsafe {
            core::ptr::write_volatile(self.descriptor_pointer(0), header);
            core::ptr::write_volatile(self.descriptor_pointer(2), status);
        }
        self.publish_and_wait()
    }

    fn prepare_rw_descriptors(
        &mut self,
        request_type: u32,
        sector: u64,
        device_writes_data: bool,
    ) -> Result<(), BlockError> {
        self.write_request_header(request_type, sector);
        self.reset_request_status();
        let descriptors = rw_descriptors(
            self.request_physical(REQUEST_HEADER_OFFSET)?,
            self.request_physical(REQUEST_DATA_OFFSET)?,
            self.request_physical(REQUEST_STATUS_OFFSET)?,
            device_writes_data,
        );
        for (index, descriptor) in descriptors.into_iter().enumerate() {
            unsafe {
                core::ptr::write_volatile(self.descriptor_pointer(index), descriptor);
            }
        }
        Ok(())
    }

    fn write_request_header(&mut self, request_type: u32, sector: u64) {
        let header = VirtioBlockRequestHeader {
            request_type,
            reserved: 0,
            sector,
        };
        let pointer = self
            .request
            .pointer_at::<VirtioBlockRequestHeader>(REQUEST_HEADER_OFFSET)
            .expect("request header must fit DMA region");
        unsafe {
            core::ptr::write_volatile(pointer, header);
        }
    }

    fn reset_request_status(&mut self) {
        let pointer = self
            .request
            .pointer_at::<u8>(REQUEST_STATUS_OFFSET)
            .expect("request status must fit DMA region");
        unsafe {
            core::ptr::write_volatile(pointer, u8::MAX);
        }
    }

    fn request_physical(&self, offset: usize) -> Result<u64, BlockError> {
        self.request.physical_at(offset).ok_or(BlockError::Io)
    }

    fn publish_and_wait(&mut self) -> Result<(), BlockError> {
        let slot = self.available_index as usize & (self.layout.size as usize - 1);
        unsafe {
            core::ptr::write_volatile(self.available_ring_pointer(slot), 0);
        }
        fence(Ordering::Release);
        self.available_index = self.available_index.wrapping_add(1);
        unsafe {
            core::ptr::write_volatile(self.available_index_pointer(), self.available_index);
            Port::<u16>::new(self.io_port(virtio_reg::QUEUE_NOTIFY)?).write(QUEUE_INDEX);
        }

        let expected = self.last_used_index.wrapping_add(1);
        let mut completed = false;
        for _ in 0..MAX_POLL_SPINS {
            let used_index = unsafe { core::ptr::read_volatile(self.used_index_pointer()) };
            if used_index == expected {
                completed = true;
                break;
            }
            core::hint::spin_loop();
        }
        if !completed {
            return Err(BlockError::Busy);
        }

        fence(Ordering::Acquire);
        let used_slot = self.last_used_index as usize & (self.layout.size as usize - 1);
        let used = unsafe { core::ptr::read_volatile(self.used_element_pointer(used_slot)) };
        self.last_used_index = expected;
        if used.id != 0 {
            return Err(BlockError::Io);
        }

        let status_pointer = self
            .request
            .pointer_at::<u8>(REQUEST_STATUS_OFFSET)
            .ok_or(BlockError::Io)?;
        match unsafe { core::ptr::read_volatile(status_pointer) } {
            VIRTIO_BLK_S_OK => Ok(()),
            VIRTIO_BLK_S_UNSUPP => Err(BlockError::Unsupported),
            _ => Err(BlockError::Io),
        }
    }

    fn io_port(&self, offset: u16) -> Result<u16, BlockError> {
        self.io_base.checked_add(offset).ok_or(BlockError::Io)
    }
}

fn rw_descriptors(
    header_address: u64,
    data_address: u64,
    status_address: u64,
    device_writes_data: bool,
) -> [VirtqDescriptor; 3] {
    [
        VirtqDescriptor {
            address: header_address,
            length: core::mem::size_of::<VirtioBlockRequestHeader>() as u32,
            flags: VIRTQ_DESC_F_NEXT,
            next: 1,
        },
        VirtqDescriptor {
            address: data_address,
            length: SECTOR_SIZE as u32,
            flags: VIRTQ_DESC_F_NEXT
                | if device_writes_data {
                    VIRTQ_DESC_F_WRITE
                } else {
                    0
                },
            next: 2,
        },
        VirtqDescriptor {
            address: status_address,
            length: 1,
            flags: VIRTQ_DESC_F_WRITE,
            next: 0,
        },
    ]
}

/// Registered virtio block device backed by a single synchronous queue.
pub struct VirtioBlockDevice {
    state: Mutex<VirtioBlockState>,
    capacity_sectors: u64,
    read_only: bool,
    supports_flush: bool,
}

impl VirtioBlockDevice {
    pub const fn capacity_sectors(&self) -> u64 {
        self.capacity_sectors
    }

    fn validate_transfer(&self, lba: u64, bytes: usize) -> Result<(), BlockError> {
        if bytes == 0 || !bytes.is_multiple_of(SECTOR_SIZE) {
            return Err(BlockError::InvalidBuffer);
        }
        let sectors = (bytes / SECTOR_SIZE) as u64;
        let end = lba.checked_add(sectors).ok_or(BlockError::OutOfRange)?;
        if end > self.capacity_sectors {
            return Err(BlockError::OutOfRange);
        }
        Ok(())
    }
}

impl BlockDevice for VirtioBlockDevice {
    fn name(&self) -> &str {
        "virtio-blk0"
    }

    fn block_size(&self) -> u32 {
        SECTOR_SIZE as u32
    }

    fn block_count(&self) -> u64 {
        self.capacity_sectors
    }

    fn read_only(&self) -> bool {
        self.read_only
    }

    fn read_blocks(&self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        self.validate_transfer(lba, buffer.len())?;
        let mut state = self.state.lock();
        for (index, sector) in buffer.chunks_exact_mut(SECTOR_SIZE).enumerate() {
            let sector_lba = lba
                .checked_add(index as u64)
                .ok_or(BlockError::OutOfRange)?;
            state.submit_read(sector_lba, sector)?;
        }
        Ok(())
    }

    fn write_blocks(&self, lba: u64, data: &[u8]) -> Result<(), BlockError> {
        if self.read_only {
            return Err(BlockError::ReadOnly);
        }
        self.validate_transfer(lba, data.len())?;
        let mut state = self.state.lock();
        for (index, sector) in data.chunks_exact(SECTOR_SIZE).enumerate() {
            let sector_lba = lba
                .checked_add(index as u64)
                .ok_or(BlockError::OutOfRange)?;
            state.submit_write(sector_lba, sector)?;
        }
        Ok(())
    }

    fn flush(&self) -> Result<(), BlockError> {
        if !self.supports_flush {
            return Err(BlockError::Unsupported);
        }
        self.state.lock().submit_flush()
    }
}

static VIRTIO_BLOCK: Once<VirtioBlockDevice> = Once::new();

/// Initialize a transitional virtio-blk PCI device and its legacy queue.
pub fn init(
    pci_device: PciDevice,
    frame_allocator: &mut BitmapFrameAllocator,
    physical_memory_offset: u64,
) -> Result<&'static VirtioBlockDevice, VirtioBlockInitError> {
    if VIRTIO_BLOCK.get().is_some() {
        return Err(VirtioBlockInitError::AlreadyInitialized);
    }

    let io_base = legacy_io_base(pci_device)?;
    let port = |offset: u16| {
        io_base
            .checked_add(offset)
            .ok_or(VirtioBlockInitError::IoPortRange)
    };
    pci::enable_legacy_io_bus_mastering(pci_device);

    unsafe {
        Port::<u8>::new(port(virtio_reg::DEVICE_STATUS)?).write(0);
        Port::<u8>::new(port(virtio_reg::DEVICE_STATUS)?).write(status::ACKNOWLEDGE);
        Port::<u8>::new(port(virtio_reg::DEVICE_STATUS)?)
            .write(status::ACKNOWLEDGE | status::DRIVER);
    }

    let device_features = unsafe { Port::<u32>::new(port(virtio_reg::DEVICE_FEATURES)?).read() };
    let negotiated_features = device_features & SUPPORTED_FEATURES;
    unsafe {
        Port::<u32>::new(port(virtio_reg::GUEST_FEATURES)?).write(negotiated_features);
        Port::<u8>::new(port(virtio_reg::DEVICE_STATUS)?)
            .write(status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK);
    }
    let accepted = unsafe { Port::<u8>::new(port(virtio_reg::DEVICE_STATUS)?).read() };
    if accepted & status::FEATURES_OK == 0 {
        return Err(VirtioBlockInitError::FeaturesRejected);
    }

    unsafe {
        Port::<u16>::new(port(virtio_reg::QUEUE_SELECT)?).write(QUEUE_INDEX);
    }
    let queue_size = unsafe { Port::<u16>::new(port(virtio_reg::QUEUE_SIZE)?).read() };
    if queue_size == 0 {
        return Err(VirtioBlockInitError::QueueUnavailable);
    }
    let layout = VirtQueueLayout::new(queue_size)?;
    let queue = DmaRegion::allocate(
        frame_allocator,
        physical_memory_offset,
        layout.total_bytes,
        VIRTQ_ALIGNMENT,
    )?;
    let request = DmaRegion::allocate(
        frame_allocator,
        physical_memory_offset,
        FRAME_SIZE,
        FRAME_SIZE,
    )?;
    let queue_page = u32::try_from(queue.physical_start() >> 12)
        .map_err(|_| VirtioBlockInitError::QueueAddressOutOfRange)?;

    unsafe {
        let available_flags = queue
            .pointer_at::<u16>(layout.available_offset)
            .ok_or(VirtioBlockInitError::InvalidQueueSize)?;
        core::ptr::write_volatile(available_flags, VIRTQ_AVAIL_F_NO_INTERRUPT);
        Port::<u32>::new(port(virtio_reg::QUEUE_ADDRESS)?).write(queue_page);
    }

    // Legacy virtio-blk device configuration begins at offset 0x14 when
    // MSI-X is not enabled. Capacity is always expressed in 512-byte sectors.
    let capacity_low = unsafe { Port::<u32>::new(port(0x14)?).read() } as u64;
    let capacity_high = unsafe { Port::<u32>::new(port(0x18)?).read() } as u64;
    let capacity_sectors = capacity_low | (capacity_high << 32);
    if capacity_sectors == 0 {
        return Err(VirtioBlockInitError::InvalidCapacity);
    }

    unsafe {
        Port::<u8>::new(port(virtio_reg::DEVICE_STATUS)?)
            .write(status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK | status::DRIVER_OK);
    }

    let device = VirtioBlockDevice {
        state: Mutex::new(VirtioBlockState {
            io_base,
            queue,
            request,
            layout,
            available_index: 0,
            last_used_index: 0,
        }),
        capacity_sectors,
        read_only: negotiated_features & VIRTIO_BLK_F_RO != 0,
        supports_flush: negotiated_features & VIRTIO_BLK_F_FLUSH != 0,
    };
    Ok(VIRTIO_BLOCK.call_once(|| device))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_legacy_queue_on_required_page_boundary() {
        let layout = VirtQueueLayout::new(128).unwrap();
        assert_eq!(layout.available_offset, 2048);
        assert_eq!(layout.used_offset, 4096);
        assert_eq!(layout.total_bytes, 5124);
        assert_eq!(layout.total_bytes.div_ceil(FRAME_SIZE), 2);
        assert_eq!(
            VirtQueueLayout::new(0),
            Err(VirtioBlockInitError::InvalidQueueSize)
        );
        assert_eq!(
            VirtQueueLayout::new(7),
            Err(VirtioBlockInitError::InvalidQueueSize)
        );
    }

    #[test]
    fn builds_read_and_write_descriptor_chains() {
        let read = rw_descriptors(0x1000, 0x2000, 0x2200, true);
        assert_eq!(read[0].flags, VIRTQ_DESC_F_NEXT);
        assert_eq!(read[0].next, 1);
        assert_eq!(read[1].flags, VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE);
        assert_eq!(read[1].next, 2);
        assert_eq!(read[2].flags, VIRTQ_DESC_F_WRITE);

        let write = rw_descriptors(0x1000, 0x2000, 0x2200, false);
        assert_eq!(write[1].flags, VIRTQ_DESC_F_NEXT);
        assert_eq!(write[1].length, SECTOR_SIZE as u32);
    }

    #[test]
    fn negotiates_only_implemented_block_features() {
        let offered = u32::MAX;
        assert_eq!(
            offered & SUPPORTED_FEATURES,
            VIRTIO_BLK_F_RO | VIRTIO_BLK_F_FLUSH
        );
    }
}
