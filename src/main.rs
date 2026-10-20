#![no_std]
#![no_main]

extern crate alloc;

use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

use rost::arch;
use rost::clint;
use rost::klog;
use rost::mem;
use rost::plic;
use rost::random::{self, get_random_bytes};
use rost::symbols::{HEAP_SIZE, HEAP_START};
use rost::trap;
use rost::uart;
use rost::virtio;
use rost::virtio::rng;

use log::{LevelFilter, info};

use riscv::register::*;
use riscv_rt::entry;

/// Blocks harts > 0 from booting, gets set to true when hart 0
/// is initated and interrupts are turned on
static BOOT: AtomicBool = AtomicBool::new(false);

/// Initiates the kernel
///
/// Go to supervised mode when initialization is done
#[entry]
unsafe fn kinit() -> ! {
    unsafe {
        if mhartid::read() == 0 {
            // Perform init that should only execute once
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

        info!("Preparing for supervisor mode");

        // Set next mode to supervisor
        mstatus::set_mpp(mstatus::MPP::Supervisor);
        mepc::write(kmain as *const () as usize);

        // Disable paging during transition
        let value = satp::Satp::from_bits(satp::Mode::Bare.into());
        satp::write(value);

        // Handle interupts in supervisor mode
        let value = medeleg::Medeleg::from_bits(medeleg::Medeleg::BITMASK);
        medeleg::write(value);

        let value = mideleg::Mideleg::from_bits(mideleg::Mideleg::BITMASK);
        mideleg::write(value);

        // Provide access to rdcycles i s-mode
        arch::riscv::expose_cycles();

        // Give access to physical memory
        pmpaddr0::write(usize::MAX);
        pmpcfg0::set_pmp(0, Range::NAPOT, Permission::RWX, false);

        info!("Jumping to supervisor mode");
        // switch to supervisor mode and jump to kmain().
        asm!("mret");
    }

    loop {
        rost::arch::riscv::wait();
    }
}

/// Kernel main
///
/// Initiates each hart and the allocator.
///
/// Never returns.
#[unsafe(no_mangle)]
unsafe fn kmain() -> ! {
    info!("Initiating hart: {}", arch::riscv::thread_pointer());
    if arch::riscv::thread_pointer() == 0 {
        // Setup allocator
        unsafe {
            rost::alloc::LockedHeap::init(HEAP_START(), HEAP_SIZE());
            trap::enable_interrupts();
        };

        // Probe virtio
        virtio::init();
        // Init devices
        let (_, mmio) = virtio::take_transport(virtio::VirtIODeviceType::EntropySource)
            .expect("Failed to get rng_device");
        let mut rng_device = rng::VirtIORng::new(mmio);
        rng_device.init();

        // If randomness fails to init, panic
        random::init();

        // Release the other HARTs
        BOOT.store(true, Ordering::Relaxed);
    } else {
        while !BOOT.load(Ordering::Acquire) {
            rost::arch::riscv::wait();
        }
        hartinit();
    }

    info!("Hart #{} ready", arch::riscv::thread_pointer());

    let mut entropy = [0u8; 32];

    get_random_bytes(&mut entropy);

    info!("{:?}", entropy);

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
        trap::enable_interrupts();
    }
}
