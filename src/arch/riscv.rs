use crate::page::PAGE_SIZE;

use core::arch::asm;
use core::time::Duration;

mod offset {
    pub const BASE: usize = 0x0200_0000;
    pub const MTIME: usize = 0xbff8;
    pub const MTIMECMP: usize = 0x4000;
}

/// Build satp value from mode, asid and page table base addr
pub fn build_satp(mode: usize, asid: usize, addr: usize) -> usize {
    assert!(addr % PAGE_SIZE == 0);
    mode << 60 | (asid & 0xffff) << 44 | (addr >> 12) & 0xff_ffff_ffff
}

pub fn wait() {
    riscv::asm::wfi();
}

pub fn uptime() -> Duration {
    let time = time();
    Duration::from_nanos(time as u64 * 100)
}

pub fn time() -> usize {
    unsafe { return core::ptr::read_volatile((offset::BASE + offset::MTIME) as *const usize) }
}

pub fn set_time(timer_val: usize) {
    let hart_id = thread_pointer();

    unsafe {
        return core::ptr::write_volatile(
            (offset::BASE + offset::MTIMECMP + hart_id) as *mut usize,
            timer_val,
        );
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

pub unsafe fn clear_sie_ssoft() {
    const SSIP: usize = 1 << 1;

    asm!("csrc sip, {}", in(reg) SSIP);
}
