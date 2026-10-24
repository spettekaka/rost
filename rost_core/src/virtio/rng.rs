use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering, fence};

use crate::virtio::common::{LegacyVirtqueue, QUEUE_SIZE};
use crate::virtio::virtio_mmio::{LegacyMmioTransport, PAGE_SIZE};
use crate::virtio::{DescFlags, DeviceStatus};

// TODO: Use page allocator to get memory, will require changes to legacy virtqueue
pub static mut RNG_QUEUE: LegacyVirtqueue = unsafe { core::mem::zeroed() };

// Atomic flag so main kernel code knows when the interrupt has finished filling the buffer
pub static ENTROPY_READY: AtomicBool = AtomicBool::new(false);

pub struct VirtIORng {
    mmio: LegacyMmioTransport,
}

impl VirtIORng {
    pub fn new(mmio: LegacyMmioTransport) -> Self {
        Self { mmio }
    }

    pub fn init(&mut self) {
        // Reset
        self.mmio.reset();

        // Ack and init driver
        self.mmio.set_status(DeviceStatus::ACK);
        self.mmio.add_status(DeviceStatus::DRIVER);

        // Setup queues
        self.mmio.set_guest_page_size(PAGE_SIZE);
        let max = self.mmio.max_queue_size(0);
        let queue_size = (QUEUE_SIZE as u32).min(max);

        let ring_paddr = core::ptr::addr_of_mut!(RNG_QUEUE) as usize;
        self.mmio.setup_queue(0, queue_size, ring_paddr as usize);

        // Finalize
        self.mmio.add_status(DeviceStatus::DRIVER_OK);
    }

    /// Only descriptor '0' is used
    pub fn request_entropy(&mut self, buf: &mut [u8]) {
        ENTROPY_READY.store(false, Ordering::Release);

        unsafe {
            // Ensure interrupts are NOT suppressed on the available ring
            write_volatile(&raw mut RNG_QUEUE.avail.flags, 0);

            // Populate descriptor 0
            write_volatile(&mut RNG_QUEUE.desc[0].addr, buf.as_mut_ptr() as u64);
            write_volatile(&mut RNG_QUEUE.desc[0].len, buf.len() as u32);
            write_volatile(&mut RNG_QUEUE.desc[0].flags, DescFlags::WRITE.into());
            write_volatile(&mut RNG_QUEUE.desc[0].next, 0);

            // Make descriptor available
            let index = read_volatile(&raw const RNG_QUEUE.avail.index);
            let slot = index as usize % QUEUE_SIZE;
            write_volatile(&mut RNG_QUEUE.avail.ring[slot], 0); // Descriptor 0

            // Memory fence so the device sees descriptor updates before index increments
            fence(Ordering::SeqCst);

            // Increment Available index
            write_volatile(&raw mut RNG_QUEUE.avail.index, index.wrapping_add(1));

            fence(Ordering::SeqCst);

            //  Notify Queue 0
            self.mmio.notify_queue(0);
        }
    }
}
