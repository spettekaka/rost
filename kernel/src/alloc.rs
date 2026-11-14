use core::alloc::{GlobalAlloc, Layout};
use core::ops::Deref;
use core::ptr::null_mut;

use rost_core::alloc::Heap;
use rost_core::sync::spinlock::Mutex;

use log::info;

use crate::page;

pub struct LockedHeap<'a>(Mutex<Option<Heap<'a>>>);

impl<'a> Default for LockedHeap<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> LockedHeap<'a> {
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
            *ALLOCATOR.0.lock().unwrap() = Some(Heap::new(page::allocator()));
        }
    }
}

impl<'a> Deref for LockedHeap<'a> {
    type Target = Mutex<Option<Heap<'a>>>;

    fn deref(&self) -> &Mutex<Option<Heap<'a>>> {
        &self.0
    }
}

unsafe impl<'a> GlobalAlloc for LockedHeap<'a> {
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
