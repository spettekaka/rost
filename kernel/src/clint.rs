use crate::arch;

use log::info;

pub const CLINT_BASE: usize = 0x2_000_000;
pub const CLINT_SIZE: usize = 0x10_000;

pub fn timer_init() {
    info!("Enabling timer interrupts");
    // Enable machine mode timer interrupts
    // Expose timer registers to S-mode
    arch::riscv::expose_time();

    arch::riscv::disable_mtime_interrupts();
}
