use crate::arch::riscv;
use crate::plic::{self, InterruptId};
use crate::uart::uart_interrupt;

use log::{debug, error};

/// Handle an interrupt from PLIC
fn plic_interrupt() {
    let plic = plic::plic();
    if let Some(id) = plic.next() {
        match id {
            InterruptId::Uart0 => uart_interrupt(),
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
