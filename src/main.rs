#![no_std]
#![no_main]

use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, Ordering};

use rost::arch;
use rost::clint;
use rost::klog;
use rost::mem;
use rost::plic;
use rost::trap;
use rost::uart;

use log::{info, LevelFilter};

use riscv::register::*;
use riscv_rt::entry;

/// Blocks harts > 0 from booting, gets set to true when hart 0
/// is initated and interrupts are turned on
static BOOT: AtomicBool = AtomicBool::new(false);

extern "C" {
    fn goto_supervised();
}

global_asm!(
    r#"
.global goto_supervised
.align 4
goto_supervised:
    csrw satp, zero # Disable paging
    csrw pmpcfg0, 0xf
    # Delegate interrupts to supervisor mode
    li t0, 0xffff
    csrw medeleg, t0
    li t0, 0xffff
    csrw mideleg, t0
    # Save hart id
    csrr a1, mhartid
    mv tp, a1
    # Switch to supervisor mode
    mret
"#
);

/// Initiates the kernel
///
/// Go to supervised mode when initialization is done
#[entry]
unsafe fn kinit() -> ! {
    if mhartid::read() == 0 {
        klog::init(LevelFilter::Trace).expect("Failed to setup logger");
        uart::Uart::new(uart::UART_BASE_ADDR).init();

        info!("Booting Rost ...");
        info!("Current hart: {}", mhartid::read());

        mem::init();
        plic::init();
        mem::enable_mmu();
        trap::hartinit();
        plic::hartinit();
        clint::timer_init();
    }

    info!("Jumping to supervisor mode");

    mstatus::set_mpp(mstatus::MPP::Supervisor);
    mepc::write(kmain as usize);

    goto_supervised();

    loop {
        rost::arch::riscv::wait();
    }
}

/// Kernel main
/// Never returns.
#[no_mangle]
unsafe fn kmain() -> ! {
    info!("Initiating hart:{}", arch::riscv::thread_pointer());
    if arch::riscv::thread_pointer() == 0 {
        // Release the other HARTs
        // BOOT.store(true, Ordering::Relaxed);
    } else {
        while !BOOT.load(Ordering::Relaxed) {
            rost::arch::riscv::wait();
        }
        hartinit();
    }

    trap::enable_interrupts();

    info!("hart #{} ready", arch::riscv::thread_pointer());

    loop {
        rost::arch::riscv::wait();
    }
}

/// Initiate a hart
///
/// Called for each hart that is starting up.
/// Calls kmain
fn hartinit() {
    info!("Booting hart {}", arch::riscv::thread_pointer());
    mem::enable_mmu();
    plic::hartinit();
    unsafe {
        trap::hartinit();
    }
}
