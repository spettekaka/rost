extern crate alloc;

use crate::arch::riscv;
use crate::interrupt;
use crate::plic::{self, InterruptId};
use crate::random::handle_rng_interrupt;
use crate::timer::TIMER_QUEUE;
use crate::trap::{disable_interrupts, restore_interrupts};
use crate::uart::uart_interrupt;

use rost_core::timer::TimerAction;

use log::{debug, error};

struct InterruptGuard {
    was_enabled: bool,
}

impl InterruptGuard {
    pub fn new() -> Self {
        Self {
            was_enabled: disable_interrupts(),
        }
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        restore_interrupts(self.was_enabled);
    }
}

/// Executes the closure `f` with local interrupts disabled on the current hart.
pub fn free<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = InterruptGuard::new();
    f()
}

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
            InterruptId::Virtio7 => handle_rng_interrupt(),
            InterruptId::Unknown => error!("Unkown PLIC interrupt ({:?}", id),
        }
        plic.complete(id as u32);
    }
}

/// Handle a software timer interrupt
fn timer_interrupt() {
    debug!("Tick");

    let mut expired = alloc::vec::Vec::new();
    let now = riscv::time_u64();

    interrupt::free(|| {
        TIMER_QUEUE.with_lock(|queue| {
            while let Some(entry) = queue.pop_expired(now) {
                expired.push(entry);
            }

            // Reprogram comparator to the next pending deadline
            if let Some(next) = queue.peek() {
                riscv::set_time(next.deadline.try_into().unwrap());
            } else {
                riscv::set_time(usize::MAX);
            }
        });
    });

    for entry in expired {
        match entry.action {
            TimerAction::Callback(cb) => cb(),
            TimerAction::Periodic(_, ref cb) => {
                cb();
                crate::timer::register_timer_periodic(entry.action);
            }
        }
    }
}

/// Handle a external interrupt
///
/// Either a plic interrupt or a timer interrupt forwarded from
/// machine mode
#[unsafe(no_mangle)]
pub fn handle_interrupt(code: u32) {
    match code {
        9 => plic_interrupt(),
        5 => timer_interrupt(),
        _ => error!("Unknown Interrupt Code: {}", code),
    }
}
