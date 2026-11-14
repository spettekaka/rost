use crate::consts::PAGE_SIZE;

use core::arch::asm;
use core::time::Duration;

#[repr(usize)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SatpMode {
    Bare = 0,
    Sv39 = 8,
    Sv48 = 9,
    Sv57 = 10,
}

/// Build satp value from mode, asid and page table base addr
pub fn build_satp(mode: SatpMode, asid: usize, addr: usize) -> usize {
    assert!(addr.is_multiple_of(PAGE_SIZE));
    (mode as usize) << 60 | (asid & 0xffff) << 44 | ((addr >> 12) & 0xfff_ffff_ffff)
}

pub fn wait() {
    riscv::asm::wfi();
}

pub fn uptime() -> Duration {
    let time = time();
    Duration::from_nanos(time as u64 * 100)
}

pub fn time() -> usize {
    riscv::register::time::read()
}

pub fn time_u64() -> u64 {
    riscv::register::time::read64()
}

/// Sets stimecmp to the value passed
///
/// # Safety
///
/// Assumes riscv64
pub fn set_time(timer_val: u64) {
    unsafe {
        // Direct write to the stimecmp CSR (CSR address 0x14D)
        asm!("csrw stimecmp, {val}", val = in(reg) timer_val);
    }
}

/// Check if interrupt is enabled
pub fn intr_get() -> bool {
    riscv::register::sstatus::read().sie()
}

pub fn thread_pointer() -> usize {
    let mut hart_id: usize;
    unsafe { asm!("mv {}, tp", out(reg) hart_id) }
    hart_id
}

/// # Safety
///
/// The assembly is only valid for riscv
pub unsafe fn clear_sie_ssoft() {
    const SSIP: usize = 1 << 1;
    unsafe {
        asm!("csrc sip, {}", in(reg) SSIP);
    }
}

pub fn expose_cycles() {
    unsafe {
        riscv::register::mcounteren::set_cy();
    }
}

///  Disable M-mode timer interrupt so M-mode never interferes with S-mode timer management
pub fn disable_mtime_interrupts() {
    unsafe {
        asm!("csrc mie, {}", in(reg) (1 << 7));
    }
}

pub fn expose_time() {
    const MENVCFG_STCE: usize = 1 << 63; // RV64: Bit 63 enables STCE
    unsafe {
        riscv::register::mcounteren::set_tm();
        asm!("csrs menvcfg, {}", in(reg) MENVCFG_STCE);
    }
}

pub fn cycles() -> u64 {
    let cycles: u64;
    unsafe {
        asm!("rdcycle {}", out(reg) cycles);
    }
    cycles
}
