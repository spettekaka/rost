extern crate alloc;
use alloc::vec::Vec;
use log::info;

use core::ptr::addr_of;
use core::ptr::read_volatile;

use crate::mmio;

// pub mod queue;

pub const VIRTIO_MMIO_BASE: usize = 0x1000_1000;
pub const VIRTIO_MMIO_STRIDE: usize = 0x1000;
pub const VIRTIO_SLOTS: usize = 32;

const VIRTIO_MAGIC: u32 = 0x74726976; // "virt"

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VirtIODeviceType {
    NetworkCard = 1,
    BlockDevice = 2,
    Console = 3,
    EntropySource = 4,
    MemoryBallooning = 5,
    IOMemory = 6,
    RPMSG = 7,
    SCSIHost = 8,
    NinePTransport = 9,
    MACWlan = 10,
    Unknown(u32),
}

impl From<u32> for VirtIODeviceType {
    fn from(dev_type: u32) -> Self {
        match dev_type {
            1 => Self::NetworkCard,
            2 => Self::BlockDevice,
            3 => Self::Console,
            4 => Self::EntropySource,
            5 => Self::MemoryBallooning,
            6 => Self::IOMemory,
            7 => Self::RPMSG,
            8 => Self::SCSIHost,
            9 => Self::NinePTransport,
            10 => Self::MACWlan,
            other => Self::Unknown(other),
        }
    }
}

pub enum IORegisterOffset {
    DeviceFeatures = 0x02,
    GuestFeatures = 0x04,
    QueueAddress = 0x08,
    QueueSize = 0x0C,
    QueueNotify = 0x10,
    DeviceStatus = 0x12,
    ISRStatus = 0x13,
}

pub enum StatusFlags {
    Clear = 0x0,
    Ack = 0x1,
    Loaded = 0x2,
    Ready = 0x4,
    FeatureOK = 0x8,
    Error = 0x64,
    Failed = 0x128,
}

pub mod device {
    pub mod network {
        pub enum RegisterOffset {
            MACAddr1 = 0x14,
            MACAddr2 = 0x15,
            MACAddr3 = 0x16,
            MACAddr4 = 0x17,
            MACAddr5 = 0x18,
            MACAddr6 = 0x19,
            Status = 0x1a1,
        }
    }

    pub mod block {
        pub enum RegisterOffset {
            TotalSectorCount = 0x14,
            MaxSegmentSize = 0x1a,
            MaxSegmentCount = 0x20,
            CylinderCount = 0x24,
            HeadCount = 0x26,
            SectorCount = 0x27,
            BlockLength = 0x28,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VirtIODevice {
    slot: usize,
    addr: usize,
    device_type: VirtIODeviceType,
    vendor_id: u32,
}

impl VirtIODevice {
    /// Slot at which the device was found
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// The virtio device type
    pub fn device_type(&self) -> VirtIODeviceType {
        self.device_type
    }

    /// The vendor ID
    pub fn vendor_id(&self) -> u32 {
        self.vendor_id
    }

    pub fn registers(&self) -> *mut VirtioMMIOBlock {
        self.addr as *mut VirtioMMIOBlock
    }
}

#[repr(C)]
pub struct VirtioMMIOBlock {
    pub magic_value: u32,         // 0x000: "virt" (0x74726976)
    pub version: u32,             // 0x004: 0x2 for Modern VirtIO
    pub device_id: u32,           // 0x008:
    pub vendor_id: u32,           // 0x00C: 0x554d4551 ("QEMU")
    pub device_features: u32,     // 0x010
    pub device_features_sel: u32, // 0x014
    _pad1: [u32; 2],
    pub driver_features: u32,     // 0x020
    pub driver_features_sel: u32, // 0x024
    _pad2: [u32; 2],
    pub queue_sel: u32,     // 0x030: Select queue index
    pub queue_num_max: u32, // 0x034: Max size supported by device
    pub queue_num: u32,     // 0x038: Selected queue size
    _pad3: [u32; 2],
    pub queue_ready: u32, // 0x044: Queue ready flag (1 = active)
    _pad4: [u32; 6],
    pub queue_notify: u32, // 0x050: Write queue index here to kick device
    _pad5: [u32; 7],
    pub interrupt_status: u32, // 0x060
    pub interrupt_ack: u32,    // 0x064
    _pad6: [u32; 2],
    pub status: u32, // 0x070: Device status bitfield
    _pad7: [u32; 3],
    pub queue_desc_low: u32, // 0x080: Low 32-bits physical addr of Desc Table
    pub queue_desc_high: u32, // 0x084: High 32-bits
    _pad8: [u32; 2],
    pub queue_driver_low: u32, // 0x090: Low 32-bits physical addr of Avail Ring
    pub queue_driver_high: u32, // 0x094: High 32-bits
    _pad9: [u32; 2],
    pub queue_device_low: u32, // 0x0A0: Low 32-bits physical addr of Used Ring
    pub queue_device_high: u32, // 0x0A4: High 32-bits
}

impl VirtioMMIOBlock {
    pub fn meta(&self) -> Option<(u32, u32, u32, u32)> {
        unsafe {
            match mmio::safe_mmio_read_u32(addr_of!(self.magic_value)) {
                Some(magic) if magic == VIRTIO_MAGIC => {
                    let version = read_volatile(&self.version);
                    let device_id = read_volatile(&self.device_id);
                    let vendor_id = read_volatile(&self.vendor_id);
                    Some((magic, version, device_id, vendor_id))
                }
                _ => None,
            }
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

pub struct VirtIOBus {
    devices: Vec<VirtIODevice>,
}

impl VirtIOBus {
    pub unsafe fn probe() -> Self {
        let mut devices = Vec::new();
        unsafe {
            for i in 0..VIRTIO_SLOTS {
                let base_addr = VIRTIO_MMIO_BASE + (i * VIRTIO_MMIO_STRIDE);
                let mmio = base_addr as *mut VirtioMMIOBlock;

                if let Some((_magic, _version, device_id, vendor_id)) = (*mmio).meta() {
                    if (*mmio).is_valid() {
                        devices.push(VirtIODevice {
                            slot: i,
                            addr: base_addr,
                            device_type: VirtIODeviceType::from(device_id),
                            vendor_id,
                        });
                    }
                }
            }
        }
        Self { devices }
    }

    pub fn iter(&self) -> core::slice::Iter<'_, VirtIODevice> {
        self.devices.iter()
    }

    pub fn filter_by_type(
        &self,
        device_type: VirtIODeviceType,
    ) -> impl Iterator<Item = &VirtIODevice> {
        self.devices
            .iter()
            .filter(move |dev| dev.device_type() == device_type)
    }

    pub fn dump(&self) {
        info!("----- VirtIO devices -----");
        for device in self.iter() {
            info!("\t Slot {:X}: {:?}", device.slot(), device.device_type());
        }
    }
}

impl IntoIterator for VirtIOBus {
    type Item = VirtIODevice;
    type IntoIter = alloc::vec::IntoIter<VirtIODevice>;

    fn into_iter(self) -> Self::IntoIter {
        self.devices.into_iter()
    }
}
