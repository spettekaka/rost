pub const QUEUE_SIZE: usize = 8;

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct VirtqDesc {
    /// Address, guest physical
    pub addr: u64,
    /// Length
    pub len: u32,
    /// Flags
    pub flags: u16,
    /// Next descriptor
    pub next: u16,
}

#[repr(C, align(2))]
#[derive(Debug, Clone, Copy)]
pub struct VirtqAvail {
    pub flags: u16,
    pub index: u16,
    pub ring: [u16; QUEUE_SIZE],
    pub used: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtqUsedElement {
    /// Index to start of used descriptor chain
    pub index: u32,
    /// Length of the descriptor chain whitch was written to
    pub len: u32,
}

#[repr(C, align(4096))]
pub struct VirtqUsed {
    pub flags: u16,
    pub index: u16,
    pub ring: [VirtqUsedElement; QUEUE_SIZE],
    pub avail_event: u16,
}

#[repr(C, align(4096))]
/// Legacy Virt Queue
///
/// Must be aligned to 4096. Part of the two page allocated memory
pub struct LegacyVirtqueue {
    pub desc: [VirtqDesc; QUEUE_SIZE], // Must be padded to 4096
    pub avail: VirtqAvail,             // Must be aligned to 256
    pub used: VirtqUsed,               // Must be padded to 4096
}
