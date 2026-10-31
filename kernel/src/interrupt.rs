use core::ptr::read_volatile;
use core::sync::atomic::Ordering;

use crate::arch::riscv;
use crate::plic::{self, InterruptId};
use crate::uart::uart_interrupt;

use rost_core::virtio::rng::{ENTROPY_READY, RNG_QUEUE};
use rost_core::virtio::virtio_mmio::LegacyMmioTransport;

use log::{debug, error};

/// Handle an interrupt from PLIC
fn plic_interrupt() {
    let plic = plic::plic();
    if let Some(id) = plic.next_interrupt() {
        match id {
            InterruptId::Uart0 => uart_interrupt(),
            InterruptId::Virtio0 => error!("Unhandled Virtio0 interrupts"),
            InterruptId::Virtio1 => error!("Unhandled Virtio1 interrupts"),
            InterruptId::Virtio2 => error!("Unhandled Virtio2 interrupts"),
            InterruptId::Virtio3 => error!("Unhandled Virtio3 interrupts"),
            InterruptId::Virtio4 => error!("Unhandled Virtio4 interrupts"),
            InterruptId::Virtio5 => error!("Unhandled Virtio5 interrupts"),
            InterruptId::Virtio6 => error!("Unhandled Virtio6 interrupts"),
            InterruptId::Virtio7 => {
                // Slot 7: virtio-rng (0x1000_8000)
                let mut transport = LegacyMmioTransport::new(0x1000_8000);

                // Acknowledge on the VirtIO device so it lowers its interrupt line
                let status = transport.interrupt_status();
                transport.ack_interrupt(status);

                // Read `RNG_QUEUE.used.index` — random bytes are now in your buffer!
                unsafe {
                    debug!(
                        "S-mode IRQ 8: RNG complete, used.idx={}",
                        read_volatile(&raw const RNG_QUEUE.used.index)
                    );
                }
                ENTROPY_READY.store(true, Ordering::Release);
            }
            InterruptId::Unknown => error!("Unkown PLIC interrupt ({:?}", id),
        }
        plic.complete(id as u32);
    }
}

/// Handle a software timer interrupt
fn timer_interrupt() {
    debug!("Tick");
    unsafe {
        riscv::clear_sie_ssoft();
    };
}

/// Handle a external interrupt
///
/// Either a plic interrupt or a timer interrupt forwarded from
/// machine mode
#[unsafe(no_mangle)]
pub fn handle_interrupt(code: u32) {
    match code {
        9 => plic_interrupt(),
        1 => timer_interrupt(),
        _ => error!("Unknown Interrupt Code: {}", code),
    }
}
