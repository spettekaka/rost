use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering, fence};

use crate::virtio::common::{LegacyVirtqueue, QUEUE_SIZE};
use crate::virtio::virtio_mmio::PAGE_SIZE;
use crate::virtio::{DescFlags, DeviceStatus, VirtIODevice};

// TODO: Use page allocator to get memory, will require changes to legacy virtqueue
pub static mut RNG_QUEUE: LegacyVirtqueue = unsafe { core::mem::zeroed() };

// Atomic flag so main kernel code knows when the interrupt has finished filling the buffer
pub static ENTROPY_READY: AtomicBool = AtomicBool::new(false);

pub struct VirtIORng {
    dev: VirtIODevice,
}

impl VirtIORng {
    pub fn init(dev: VirtIODevice) -> Result<Self, &'static str> {
        let mut mmio = dev.registers();

        // Reset
        mmio.reset();

        // Ack and init driver
        mmio.set_status(DeviceStatus::ACK);
        mmio.add_status(DeviceStatus::DRIVER);

        // Setup queues
        mmio.set_guest_page_size(PAGE_SIZE);
        let max = mmio.max_queue_size(0);
        let queue_size = (QUEUE_SIZE as u32).min(max);

        let ring_paddr = core::ptr::addr_of_mut!(RNG_QUEUE) as usize;
        mmio.setup_queue(0, queue_size, ring_paddr as usize);

        // Finalize
        mmio.add_status(DeviceStatus::DRIVER_OK);

        Ok(Self { dev })
    }

    /// Only descriptor '0' is used
    pub fn request_entropy(&mut self, buf: &mut [u8]) {
        let mut mmio = self.dev.registers();

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

            // Memory fence so the device sees descriptor updates before idx increments
            fence(Ordering::SeqCst);

            // Increment Available index
            write_volatile(&raw mut RNG_QUEUE.avail.index, index.wrapping_add(1));

            fence(Ordering::SeqCst);

            //  Notify Queue 0
            mmio.notify_queue(0);
        }
    }
}
