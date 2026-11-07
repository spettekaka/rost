extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BinaryHeap;
use core::sync::atomic::{AtomicUsize, Ordering};

pub type TimerId = usize;

static CURRENT_TIMER_ID: AtomicUsize = AtomicUsize::new(1);

pub enum TimerAction {
    /// Execute a callback directly
    Callback(Box<dyn FnOnce() + Send>),

    /// Execute a callback periodically
    Periodic(u64, Box<dyn Fn() + Send>),
}

pub struct TimerEntry {
    pub id: TimerId,
    pub deadline: u64,
    pub action: TimerAction,
}

impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline
    }
}

impl Eq for TimerEntry {}

// Invert ordering so BinaryHeap acts as a Min-Heap
impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        other.deadline.cmp(&self.deadline)
    }
}

impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Allocates a globally unique, non-zero TimerId
pub fn allocate_timer_id() -> TimerId {
    // Relaxed ordering is sufficient here because the ID's only guarantee
    // is uniqueness across threads/cores, not memory synchronization.
    let id = CURRENT_TIMER_ID.fetch_add(1, Ordering::Relaxed);

    // Safeguard against reaching 0 on rollover if used with a smaller type
    if id == 0 {
        panic!("Timer ID overflow!");
    }

    id
}

pub struct TimerQueue {
    heap: BinaryHeap<TimerEntry>,
}

impl Default for TimerQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerQueue {
    pub const fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
        }
    }

    pub fn push(&mut self, entry: TimerEntry) {
        self.heap.push(entry);
    }

    pub fn peek(&self) -> Option<&TimerEntry> {
        self.heap.peek()
    }

    /// Returns the earliset deadline in the queue
    pub fn next_deadline(&self) -> Option<u64> {
        self.heap.peek().map(|e| e.deadline)
    }

    pub fn pop_expired(&mut self, time: u64) -> Option<TimerEntry> {
        self.heap.pop_if(|entry| entry.deadline <= time)
    }

    pub fn cancel(&mut self, id: TimerId) -> bool {
        let original_len = self.heap.len();
        self.heap.retain(|entry| entry.id != id);

        original_len != self.heap.len()
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}
