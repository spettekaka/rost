use core::alloc::{GlobalAlloc, Layout};
use core::ops::Deref;
use core::ptr::null_mut;

use rost_core::alloc::Heap;
use rost_core::sync::spinlock::Mutex;

use log::info;

pub struct LockedHeap(Mutex<Option<Heap>>);

impl Default for LockedHeap {
    fn default() -> Self {
        Self::new()
    }
}

impl LockedHeap {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// # Safety
    ///
    /// The kernel gurantees the addresses are correct
    pub unsafe fn init(start_addr: usize, size: usize) {
        info!(
            "Initiating allocator. Start addr: 0x{:x}, size: {}",
            start_addr, size
        );

        unsafe {
            *ALLOCATOR.0.lock().unwrap() = Some(Heap::new(start_addr, size));
        }
    }
}

impl Deref for LockedHeap {
    type Target = Mutex<Option<Heap>>;

    fn deref(&self) -> &Mutex<Option<Heap>> {
        &self.0
    }
}

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Some(ref mut heap) = *self.0.lock().unwrap() {
            match heap.allocate(layout) {
                Ok(ptr) => return ptr,
                Err(_) => return null_mut(),
            }
        }
        null_mut()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(ref mut heap) = *self.0.lock().unwrap() {
            heap.deallocate(ptr, layout);
        }
    }
}

#[global_allocator]
pub static ALLOCATOR: LockedHeap = LockedHeap::new();
