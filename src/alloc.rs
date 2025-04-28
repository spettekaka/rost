use core::alloc::{AllocError, GlobalAlloc, Layout};
use core::ops::Deref;
use core::ptr::null_mut;

use log::info;

use crate::slab::Slab;
use crate::sync::spinlock::Mutex;

pub const NUM_SLABS: usize = 7;
pub const MIN_SLAB_SIZE: usize = 4096;
pub const MIN_HEAP_SIZE: usize = MIN_SLAB_SIZE * NUM_SLABS;

pub enum HeapAllocator {
    Slab64,
    Slab128,
    Slab256,
    Slab512,
    Slab1024,
    Slab2048,
    Slab4096,
}

pub struct Heap {
    slabs: [Slab; NUM_SLABS],
}

impl Heap {
    pub unsafe fn new(start_addr: usize, heap_size: usize) -> Self {
        let slab_size = heap_size / NUM_SLABS;
        unsafe {
            Self {
                slabs: [
                    Slab::init(start_addr, slab_size, 64),
                    Slab::init(start_addr + slab_size, slab_size, 128),
                    Slab::init(start_addr + slab_size * 2, slab_size, 256),
                    Slab::init(start_addr + slab_size * 3, slab_size, 512),
                    Slab::init(start_addr + slab_size * 4, slab_size, 1024),
                    Slab::init(start_addr + slab_size * 5, slab_size, 2048),
                    Slab::init(start_addr + slab_size * 6, slab_size, 4096),
                ],
            }
        }
    }

    pub fn allocate(&mut self, layout: Layout) -> Result<*mut u8, AllocError> {
        match Self::layout_to_allocator(layout) {
            HeapAllocator::Slab64 => self.slabs[0].allocate(layout),
            HeapAllocator::Slab128 => self.slabs[1].allocate(layout),
            HeapAllocator::Slab256 => self.slabs[2].allocate(layout),
            HeapAllocator::Slab512 => self.slabs[3].allocate(layout),
            HeapAllocator::Slab1024 => self.slabs[4].allocate(layout),
            HeapAllocator::Slab2048 => self.slabs[5].allocate(layout),
            HeapAllocator::Slab4096 => self.slabs[6].allocate(layout),
        }
    }

    pub fn deallocate(&mut self, block: *mut u8, layout: Layout) {
        match Self::layout_to_allocator(layout) {
            HeapAllocator::Slab64 => self.slabs[0].deallocate(block),
            HeapAllocator::Slab128 => self.slabs[1].deallocate(block),
            HeapAllocator::Slab256 => self.slabs[2].deallocate(block),
            HeapAllocator::Slab512 => self.slabs[3].deallocate(block),
            HeapAllocator::Slab1024 => self.slabs[4].deallocate(block),
            HeapAllocator::Slab2048 => self.slabs[5].deallocate(block),
            HeapAllocator::Slab4096 => self.slabs[6].deallocate(block),
        }
    }

    fn layout_to_allocator(layout: Layout) -> HeapAllocator {
        match layout.size() {
            x if x <= 64 => HeapAllocator::Slab64,
            x if x <= 128 => HeapAllocator::Slab128,
            x if x <= 256 => HeapAllocator::Slab256,
            x if x <= 512 => HeapAllocator::Slab512,
            x if x <= 1024 => HeapAllocator::Slab1024,
            x if x <= 2048 => HeapAllocator::Slab2048,
            x if x <= 4096 => HeapAllocator::Slab4096,
            _ => unreachable!(),
        }
    }
}

pub struct LockedHeap(Mutex<Option<Heap>>);

impl LockedHeap {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

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
