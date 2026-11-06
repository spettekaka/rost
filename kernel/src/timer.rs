extern crate alloc;

use rost_core::sync::spinlock::Mutex;
use rost_core::timer::{TimerAction, TimerEntry, TimerId, TimerQueue, allocate_timer_id};

use crate::arch;

pub static TIMER_QUEUE: Mutex<TimerQueue> = Mutex::new(TimerQueue::new());

pub fn register_timer(delay: u64, action: TimerAction) -> TimerId {
    let now = arch::riscv::time() as u64;
    let deadline = now.saturating_add(delay);
    let id = allocate_timer_id();

    let entry = TimerEntry {
        id,
        deadline,
        action,
    };

    crate::interrupt::free(|| {
        TIMER_QUEUE.with_lock(|queue| {
            let reprogram_needed = queue.peek().map_or(true, |next| deadline < next.deadline);

            queue.push(entry);

            if reprogram_needed {
                arch::riscv::set_time(deadline.try_into().unwrap());
            }
        })
    });

    id
}
