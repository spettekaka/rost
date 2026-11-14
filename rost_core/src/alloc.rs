use core::alloc::{AllocError, Layout};

use crate::{atomic_bitmap::AtomicPageAllocator, consts::PAGE_SIZE, slab::Slab};

pub const NUM_SLABS: usize = 6;
pub const MIN_SLAB_SIZE: usize = 4096;
pub const MIN_HEAP_SIZE: usize = MIN_SLAB_SIZE * NUM_SLABS;
const PAGES_PER_SLAB: usize = 1024;

pub enum HeapAllocator {
    Slab64,
    Slab128,
    Slab256,
    Slab512,
    Slab1024,
    Slab2048,
}

pub struct Heap<'a> {
    page_allocator: &'a AtomicPageAllocator,
    slabs: [Slab; NUM_SLABS],
}

impl<'a> Heap<'a> {
    /// # Safety
    ///
    /// The addresses are guaranteed to be correct by thec caller
    pub unsafe fn new(page_allocator: &'a AtomicPageAllocator) -> Self {
        Self {
            page_allocator,
            slabs: [
                Slab::new(64),
                Slab::new(128),
                Slab::new(256),
                Slab::new(512),
                Slab::new(1024),
                Slab::new(2048),
            ],
        }
    }

    pub fn init(&mut self) -> Result<(), AllocError> {
        for slab in self.slabs.iter_mut() {
            let ptr = self
                .page_allocator
                .alloc(PAGES_PER_SLAB)
                .ok_or(AllocError)?;
            unsafe { slab.grow(ptr, PAGES_PER_SLAB * PAGE_SIZE) };
        }
        Ok(())
    }

    pub fn allocate(&mut self, layout: Layout) -> Result<*mut u8, AllocError> {
        let effective_size = layout.size().max(layout.align());
        if let Some(allocator) = Self::layout_to_allocator(effective_size)
            && let Ok(ptr) = self.slabs[allocator as usize].allocate(layout)
        {
            return Ok(ptr);
        }

        if layout.align() > PAGE_SIZE {
            return Err(AllocError);
        }

        let pages = layout.size().div_ceil(PAGE_SIZE).max(1);
        self.page_allocator.alloc(pages).ok_or(AllocError)
    }

    pub fn deallocate(&mut self, ptr: *mut u8, layout: Layout) {
        let effective_size = layout.size().max(layout.align());
        if let Some(allocator) = Self::layout_to_allocator(effective_size) {
            self.slabs[allocator as usize].deallocate(ptr);
        } else {
            let pages = layout.size().div_ceil(PAGE_SIZE).max(1);
            self.page_allocator.dealloc(ptr, pages)
        }
    }

    fn layout_to_allocator(size: usize) -> Option<HeapAllocator> {
        match size {
            x if x <= 64 => Some(HeapAllocator::Slab64),
            x if x <= 128 => Some(HeapAllocator::Slab128),
            x if x <= 256 => Some(HeapAllocator::Slab256),
            x if x <= 512 => Some(HeapAllocator::Slab512),
            x if x <= 1024 => Some(HeapAllocator::Slab1024),
            x if x <= 2048 => Some(HeapAllocator::Slab2048),
            _ => None,
        }
    }
}
