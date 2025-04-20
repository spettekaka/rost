#![no_std]
#![feature(sync_unsafe_cell)]
#![feature(allocator_api)]

pub mod alloc;
pub mod arch;
pub mod clint;
pub mod interrupt;
pub mod klog;
pub mod mem;
pub mod page;
pub mod plic;
pub mod rand;
pub mod slab;
pub mod symbols;
pub mod sync;
pub mod trap;
pub mod uart;

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
