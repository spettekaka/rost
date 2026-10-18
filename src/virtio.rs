extern crate alloc;
use alloc::vec::Vec;
use log::info;

use crate::{
    sync::spinlock::{Mutex, MutexError},
    virtio::virtio_mmio::LegacyMmioTransport,
};

pub mod common;
pub mod rng;
pub mod virtio_mmio;

pub const VIRTIO_MMIO_BASE: usize = 0x1000_1000;
pub const VIRTIO_MMIO_STRIDE: usize = 0x1000;
pub const VIRTIO_SLOTS: usize = 8; // FIXME: 8 is max when running in QEMU

const VIRTIO_MAGIC: u32 = 0x74726976; // "virt"

pub static VIRTIO_BUS: Mutex<VirtIOBus> = Mutex::new(VirtIOBus::new());

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

#[repr(transparent)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct DeviceStatus(pub u32);

impl DeviceStatus {
    pub const CLEAR: Self = Self(0x0);
    pub const ACK: Self = Self(0x1);
    pub const DRIVER: Self = Self(0x2);
    pub const DRIVER_OK: Self = Self(0x4);
    pub const FEATURES_OK: Self = Self(0x8);
    pub const ERROR: Self = Self(0x64);
    pub const FAILED: Self = Self(0x128);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn set(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub const fn clear(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl core::ops::BitOr for DeviceStatus {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for DeviceStatus {
    fn bitor_assign(&mut self, rhs: Self) {
        self.set(rhs);
    }
}

impl From<DeviceStatus> for u32 {
    fn from(value: DeviceStatus) -> Self {
        value.0
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescFlags(u16);

impl DescFlags {
    pub const READ_ONLY: Self = Self(0);
    pub const NEXT: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const INDIRECT: Self = Self(1 << 2);

    pub const fn empty() -> Self {
        Self(0)
    }
}

impl core::ops::BitOr for DescFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl From<DescFlags> for u16 {
    fn from(value: DescFlags) -> Self {
        value.0
    }
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

    pub fn irq(&self) -> u32 {
        (self.slot + 1) as u32
    }

    /// The virtio device type
    pub fn device_type(&self) -> VirtIODeviceType {
        self.device_type
    }

    /// The vendor ID
    pub fn vendor_id(&self) -> u32 {
        self.vendor_id
    }

    pub fn registers(&self) -> LegacyMmioTransport {
        LegacyMmioTransport::new(self.addr)
    }
}

pub struct VirtIOBus {
    devices: Vec<VirtIODevice>,
}

impl VirtIOBus {
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
        }
    }

    pub unsafe fn probe(&mut self) {
        for i in 0..8 {
            let base_addr = VIRTIO_MMIO_BASE + (i * VIRTIO_MMIO_STRIDE);

            let mmio = LegacyMmioTransport::new(base_addr);

            let (_magic, _version, device_id, vendor_id) = mmio.meta();
            if mmio.is_valid() {
                self.devices.push(VirtIODevice {
                    slot: i,
                    addr: base_addr,
                    device_type: VirtIODeviceType::from(device_id),
                    vendor_id,
                });
            }
        }
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

    pub fn _dump(&self) {
        info!("----- VirtIO devices -----");
        for device in self.iter() {
            info!("\t Slot {:X}: {:?}", device.slot(), device.device_type());
        }
        info!("----- \t -----");
    }

    pub fn dump(bus: &VirtIOBus) {
        bus._dump();
    }

    /// Read-only slice of all discovered devices
    pub fn devices(&self) -> &[VirtIODevice] {
        &self.devices
    }

    /// Claims the `LegacyMmioTransport` and returns `(irq, transport)` for a driver to initialize
    pub fn take_transport(
        &mut self,
        device_type: VirtIODeviceType,
    ) -> Option<(u32, LegacyMmioTransport)> {
        for dev in self.devices.iter() {
            if dev.device_type() == device_type {
                let transport = dev.registers();
                return Some((dev.irq(), transport));
            }
        }
        None
    }
}

pub fn init() -> Result<(), MutexError> {
    let mut bus = VIRTIO_BUS.lock()?;
    unsafe { bus.probe() };
    VirtIOBus::dump(&bus);
    Ok(())
}

pub fn take_transport(id: VirtIODeviceType) -> Option<(u32, LegacyMmioTransport)> {
    VIRTIO_BUS.lock().ok()?.take_transport(id)
}

impl IntoIterator for VirtIOBus {
    type Item = VirtIODevice;
    type IntoIter = alloc::vec::IntoIter<VirtIODevice>;

    fn into_iter(self) -> Self::IntoIter {
        self.devices.into_iter()
    }
}
