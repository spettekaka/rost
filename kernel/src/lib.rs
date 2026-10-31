#![no_std]
#![feature(sync_unsafe_cell)]

pub mod alloc;
pub mod clint;
pub mod interrupt;
pub mod klog;
pub mod mem;
pub mod page;
pub mod plic;
pub mod random;
pub mod symbols;
pub mod trap;
pub mod uart;

use rost_core::arch;

/// Panic handler
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("hart {} aborting: ", arch::riscv::thread_pointer());
    if let Some(p) = info.location() {
        println!("line {}, file {}: {}", p.line(), p.file(), info.message());
    } else {
        println!("no information available.");
    }

    loop {
        riscv::asm::wfi();
    }
}
