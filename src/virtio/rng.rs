use crate::virtio::virtio_mmio::PAGE_SIZE;
use crate::virtio::{DeviceStatus, VirtIODevice};

#[repr(C, align(4096))]
struct AlignedRingMemory([u8; 8192]); // 2 contiguous 4KB pages

// TODO: Use page allocator to get memory
static mut RNG_QUEUE_MEM: AlignedRingMemory = AlignedRingMemory([0; 8192]);

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

        // Negotiate features
        mmio.add_status(DeviceStatus::FEATURES_OK);

        mmio.set_guest_page_size(PAGE_SIZE);

        // Setup queues
        let max = mmio.max_queue_size(0);
        let queue_size = 16.min(max);

        let ring_paddr = core::ptr::addr_of_mut!(RNG_QUEUE_MEM) as usize;
        mmio.setup_queue(0, queue_size, ring_paddr as usize);

        // Finalize
        mmio.add_status(DeviceStatus::DRIVER_OK);
        Ok(Self { dev })
    }
}
