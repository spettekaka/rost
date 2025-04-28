// Platform Level Interrupt Controller (PLIC)
// IRQS in qemu:
// VIRTIO: 1..8
// UART0: 10
// PCIE: 32..35
//

use core::ptr::addr_of_mut;

use crate::arch::riscv::thread_pointer;

use log::info;

// PLIC mmio registers
pub const PLIC_BASE: usize = 0x0c00_0000;
const _PLIC_PRIORITY: usize = PLIC_BASE + 0x0;
const PLIC_PENDING: usize = PLIC_BASE + 0x1000;
const _PLIC_MENABLE_BASE: usize = PLIC_BASE + 0x2000;
const PLIC_SENABLE_BASE: usize = PLIC_BASE + 0x2080;
const _PLIC_MPRIORITY_BASE: usize = PLIC_BASE + 0x200000;
const PLIC_SPRIORITY_BASE: usize = PLIC_BASE + 0x201000;
const _PLIC_MCLAIM_BASE: usize = PLIC_BASE + 0x200004;
const PLIC_SCLAIM_BASE: usize = PLIC_BASE + 0x201004;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Threshold {
    All,
    None,
    Level(Priority),
}

/// Interrupt priority.
///
/// One equals the highest priority
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Priority {
    Zero = 0,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
}

impl From<Threshold> for Priority {
    fn from(threshold: Threshold) -> Self {
        match threshold {
            Threshold::All => Self::Zero,
            Threshold::None => Self::Seven,
            Threshold::Level(priority) => priority,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InterruptId {
    Unknown = 0,
    Uart0 = 10,
}

impl From<u32> for InterruptId {
    fn from(irq: u32) -> Self {
        match irq {
            10 => Self::Uart0,
            _ => Self::Unknown,
        }
    }
}

pub struct Plic;

impl Plic {
    /// Create a new Plic instance
    pub const fn new() -> Self {
        Self {}
    }

    /// Initialize the PLIC.
    /// Enables an interrupt id.
    pub unsafe fn init(&mut self, id: InterruptId) {
        let enabled = PLIC_BASE as *mut u32;
        unsafe { enabled.add(id as usize).write_volatile(1) };
    }

    /// Enable an interrupt id.
    pub fn enable(&mut self, id: InterruptId) {
        let enable = Self::senable(thread_pointer()) as *mut u32;
        // The plic enable register contains a bitmap over enabled interrupts.
        let id = 1 << id as u32;
        unsafe {
            enable.write_volatile(enable.read_volatile() | id);
        }
    }

    /// Disable an interrupt id.
    pub fn disable(&mut self, id: InterruptId) {
        let disable = Self::senable(thread_pointer()) as *mut u32;
        // The plic enable register contains a bitmap over enabled interrupts.
        let id = !(1 << id as u32);
        unsafe {
            disable.write_volatile(disable.read_volatile() & id);
        }
    }

    /// Set priority for an interrupt
    ///
    /// Priority must be in range [0..7]
    pub fn set_priority(&mut self, id: InterruptId, priority: Priority) {
        let priority = priority as u32;
        let reg = Plic::spriority(thread_pointer()) as *mut u32;
        unsafe {
            // Interrupt id offset is: PLIC_PRIORITY + 4 * id
            // Reg is u32, no neeed to multiply by 4
            reg.add(id as usize).write_volatile(priority);
        }
    }

    /// Set global interrupt threshold.
    ///
    /// Interrupt with priority equal to or below the threshlod will be masked.
    /// 7 will mask all interrupts and 0 will allow all interrupts.
    ///
    /// Threshold must be in [0..7]
    pub fn set_threshold(&mut self, threshold: Threshold) {
        let threshold = Priority::from(threshold) as u32;
        let reg = Plic::spriority(thread_pointer()) as *mut u32;
        unsafe {
            reg.write_volatile(threshold);
        }
    }

    /// Check if a given interrupt is pending
    pub fn is_pending(&mut self, id: InterruptId) -> bool {
        let pending = PLIC_PENDING as *const u32;
        let id = 1 << id as u32;
        unsafe { pending.read_volatile() & id != 0 }
    }

    /// Complete an interrupt by id
    ///
    /// The id must be from `next`
    pub fn complete(&mut self, id: u32) {
        let reg = Self::sclaim(thread_pointer()) as *mut u32;
        unsafe {
            reg.write_volatile(id);
        }
    }

    /// Get the next available interrupt
    ///
    /// The PLIC will sort by priority and return the ID of the pending interrupt
    pub fn next(&mut self) -> Option<InterruptId> {
        let reg = Self::sclaim(thread_pointer()) as *const u32;

        let id = unsafe { reg.read_volatile() };

        match id {
            0 => None,
            id => Some(InterruptId::from(id)),
        }
    }

    const fn _menable(hart: usize) -> usize {
        _PLIC_MENABLE_BASE + hart * 0x100
    }

    const fn senable(hart: usize) -> usize {
        PLIC_SENABLE_BASE + hart * 0x100
    }

    const fn _mpriority(hart: usize) -> usize {
        _PLIC_MPRIORITY_BASE + hart * 0x2000
    }

    const fn spriority(hart: usize) -> usize {
        PLIC_SPRIORITY_BASE + hart * 0x2000
    }

    const fn _mclaim(hart: usize) -> usize {
        _PLIC_MCLAIM_BASE + hart * 0x2000
    }

    const fn sclaim(hart: usize) -> usize {
        PLIC_SCLAIM_BASE + hart * 0x2000
    }
}

static mut _PLIC: Plic = Plic::new();

pub fn plic() -> &'static mut Plic {
    unsafe { addr_of_mut!(_PLIC).as_mut().unwrap() }
}

/// Initiate PLIC
/// Should only be called once
pub unsafe fn init() {
    info!("Initating PLIC");
    let plic = plic();
    unsafe { plic.init(InterruptId::Uart0) };
}

/// Initiate the plic for the current HART
pub fn hartinit() {
    let plic = plic();
    plic.enable(InterruptId::Uart0);
    plic.set_threshold(Threshold::All);
    plic.set_priority(InterruptId::Uart0, Priority::One);
}
