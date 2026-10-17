use core::ptr::{read_volatile, write_volatile};

use crate::mmio;
use crate::virtio::{DeviceStatus, VIRTIO_MAGIC, VirtIODeviceType};

pub const PAGE_SIZE: u32 = 4096;

#[repr(usize)]
#[derive(Debug, Clone, Copy)]
pub enum LegacyMmioRegister {
    MagicValue = 0x000, // 0x74726976
    Version = 0x004,    // 0x1 (Legacy)
    DeviceId = 0x008,
    VendorId = 0x00C,
    HostFeatures = 0x010, // Read features
    HostFeaturesSel = 0x014,
    GuestFeatures = 0x020, // Write negotiated features
    GuestFeaturesSel = 0x024,
    GuestPageSize = 0x028, // Write page size (usually 4096)
    QueueSel = 0x030,      // Select queue index
    QueueNumMax = 0x034,   // Max entries supported
    QueueNum = 0x038,      // Set queue size
    QueueAlign = 0x03C,    // Used Ring alignment (4096)
    QueuePfn = 0x040,      // Physical Page Number (base_addr / 4096)
    QueueNotify = 0x050,   // Notify device of work
    InterruptStatus = 0x060,
    InterruptAck = 0x064,
    Status = 0x070,
    ConfigSpace = 0x100, // Device-specific configs start here
}

pub struct LegacyMmioTransport {
    addr: usize,
}

impl LegacyMmioTransport {
    pub fn new(addr: usize) -> Self {
        Self { addr }
    }

    pub fn addr(&self) -> usize {
        self.addr
    }

    fn read_reg(&self, reg: LegacyMmioRegister) -> u32 {
        let addr = self.addr + reg as usize;
        unsafe { read_volatile(addr as *const u32) }
    }

    fn write_reg(&mut self, reg: LegacyMmioRegister, val: u32) {
        let addr = self.addr + reg as usize;
        unsafe { write_volatile(addr as *mut u32, val) };
    }

    pub fn magic_value(&self) -> u32 {
        self.read_reg(LegacyMmioRegister::MagicValue)
    }

    pub fn magic_value_safe(&self) -> Option<u32> {
        let addr = self.addr + LegacyMmioRegister::MagicValue as usize;
        unsafe { mmio::safe_mmio_read_u32(addr as *const u32) }
    }

    pub fn version(&self) -> u32 {
        self.read_reg(LegacyMmioRegister::Version)
    }

    pub fn device_id(&self) -> u32 {
        self.read_reg(LegacyMmioRegister::DeviceId)
    }

    pub fn vendor_id(&self) -> u32 {
        self.read_reg(LegacyMmioRegister::VendorId)
    }

    pub fn status(&self) -> DeviceStatus {
        let status = self.read_reg(LegacyMmioRegister::Status);
        DeviceStatus(status)
    }

    pub fn set_status(&mut self, status: DeviceStatus) {
        self.write_reg(LegacyMmioRegister::Status, status.into());
    }

    pub fn add_status(&mut self, status: DeviceStatus) {
        let current = self.status();
        self.write_reg(LegacyMmioRegister::Status, (status | current).into());
    }

    pub fn reset(&mut self) {
        self.write_reg(LegacyMmioRegister::Status, DeviceStatus::CLEAR.into());
    }

    pub fn select_queue(&mut self, index: u32) {
        self.write_reg(LegacyMmioRegister::QueueSel, index);
    }

    pub fn set_guest_page_size(&mut self, page_size: u32) {
        self.write_reg(LegacyMmioRegister::GuestPageSize, page_size);
    }

    pub fn max_queue_size(&mut self, index: u32) -> u32 {
        self.select_queue(index);
        self.read_reg(LegacyMmioRegister::QueueNumMax)
    }

    pub fn setup_queue(&mut self, index: u32, num: u32, ring_paddr: usize) {
        self.select_queue(index);

        self.write_reg(LegacyMmioRegister::QueueNum, num);
        self.write_reg(LegacyMmioRegister::QueueAlign, PAGE_SIZE);

        let physical_pages = (ring_paddr / PAGE_SIZE as usize) as u32;
        self.write_reg(LegacyMmioRegister::QueuePfn, physical_pages);
    }

    pub fn notify_queue(&mut self, index: u32) {
        self.write_reg(LegacyMmioRegister::QueueNotify, index);
    }

    pub fn meta(&self) -> Option<(u32, u32, u32, u32)> {
        match self.magic_value_safe() {
            Some(magic) if magic == VIRTIO_MAGIC => {
                let version = self.version();
                let device_id = self.read_reg(LegacyMmioRegister::DeviceId);
                let vendor_id = self.read_reg(LegacyMmioRegister::VendorId);
                Some((magic, version, device_id, vendor_id))
            }
            _ => None,
        }
    }

    pub fn is_valid(&self) -> bool {
        if let Some((magic, version, device_id, _)) = self.meta() {
            magic == VIRTIO_MAGIC
                && version == 1
                && VirtIODeviceType::from(device_id) != VirtIODeviceType::Unknown(device_id)
        } else {
            false
        }
    }
}
