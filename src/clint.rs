use crate::arch;
use core::ptr;

use riscv::register::{mtvec::Mtvec, *};

use log::info;

pub const CLINT_BASE: usize = 0x2_000_000;
pub const CLINT_SIZE: usize = 0x10_000;
pub const CLINT_MTIME_OFFSET: usize = 0xBFF8;
pub const CLINT_MTIMECMP_OFFSET: usize = 0x4_000;

const TIMER_INTERVAL: u64 = 1_000_000;

const MAX_HARTS: usize = 8;

static mut TIMER_SCRATCH: [[u64; 5]; MAX_HARTS] = [[0u64; 5]; MAX_HARTS];

unsafe fn read_mtime() -> u64 {
    unsafe { ptr::read_volatile((CLINT_BASE + CLINT_MTIME_OFFSET) as *const u64) }
}

unsafe fn read_mtimecmp(hart: usize) -> u64 {
    unsafe { ptr::read_volatile((CLINT_BASE + 8 * hart + CLINT_MTIMECMP_OFFSET) as *const u64) }
}

unsafe fn write_mtimecmp(hart: usize, val: u64) {
    let addr = (CLINT_BASE + 8 * hart + CLINT_MTIMECMP_OFFSET) as *mut u64;
    unsafe { ptr::write_volatile(addr, val) };
}

unsafe fn increment_mtimecmp(hart: usize, interval: u64) {
    unsafe {
        let current = read_mtime();
        write_mtimecmp(hart, current + interval)
    };
}

fn mtiecmp_hart() -> usize {
    let hart = arch::riscv::thread_pointer();
    CLINT_BASE + 8 * hart + CLINT_MTIMECMP_OFFSET
}

pub fn timer_init() {
    info!("Enabling timer interrupts");
    // Enable machine mode timer interrupts
    unsafe {
        unsafe extern "C" {
            fn timervec();
        }

        let hart = arch::riscv::thread_pointer();
        increment_mtimecmp(hart, TIMER_INTERVAL);

        TIMER_SCRATCH[hart][3] = mtiecmp_hart() as u64;
        TIMER_SCRATCH[hart][4] = TIMER_INTERVAL;

        mscratch::write(TIMER_SCRATCH[hart].as_ptr() as usize);

        let mut mtvec_reg = Mtvec::from_bits(0);
        mtvec_reg.set_address(timervec as usize);
        mtvec_reg.set_trap_mode(stvec::TrapMode::Direct);

        mtvec::write(mtvec_reg);

        mstatus::set_mie();
        mie::set_mtimer();
    }
}

pub fn debug() {
    let hart = arch::riscv::thread_pointer();
    unsafe {
        info!(
            "{} {} 0x{:X} {}",
            read_mtime(),
            read_mtimecmp(hart),
            TIMER_SCRATCH[hart][3],
            TIMER_SCRATCH[hart][4]
        );
    }
}
