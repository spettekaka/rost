use core::alloc::{AllocError, Layout};
use core::ptr::{self, NonNull};

pub struct Slab {
    capacity: usize,
    size: usize,
    free_list: FreeList,
}

#[derive(Default)]
struct FreeList {
    len: usize,
    head: Option<NonNull<FreeBlock>>,
}

// SAFETY: FreeList owns the memory blocks in the slab and is only accessed
// through &mut Slab (synchronized externally, e.g., via Mutex in LockedHeap).
unsafe impl Send for FreeList {}

#[derive(Default)]
struct FreeBlock {
    next: Option<NonNull<FreeBlock>>,
}

impl Slab {
    /// # Safety
    ///
    /// The addresses are guranteed by the caller to be correct
    pub unsafe fn init(start_addr: usize, slab_size: usize, block_size: usize) -> Self {
        let n_blocks = slab_size / block_size;
        Self {
            capacity: n_blocks,
            size: block_size,
            free_list: unsafe { FreeList::new(start_addr, n_blocks, block_size) },
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn usage(&self) -> usize {
        self.capacity - self.free_list.len()
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn allocate(&mut self, _layout: Layout) -> Result<*mut u8, AllocError> {
        match self.free_list.pop() {
            Some(block) => Ok(block.as_ptr().cast::<u8>()),
            None => Err(AllocError),
        }
    }

    pub fn deallocate(&mut self, ptr: *mut u8) {
        let block = NonNull::new(ptr.cast::<FreeBlock>()).expect("cannot deallocate null pointer");
        unsafe {
            self.free_list.push(block);
        }
    }
}

impl FreeList {
    unsafe fn new(start_addr: usize, blocks: usize, block_size: usize) -> Self {
        let mut free_list = Self { len: 0, head: None };

        for i in (0..blocks).rev() {
            let ptr = ptr::with_exposed_provenance_mut::<FreeBlock>(start_addr + i * block_size);
            let block = NonNull::new(ptr).expect("slab block address cannot be null");
            unsafe { free_list.push(block) };
        }
        free_list
    }

    unsafe fn push(&mut self, block: NonNull<FreeBlock>) {
        unsafe {
            block.as_ptr().write(FreeBlock {
                next: self.head.take(),
            });
        }
        self.len += 1;
        self.head = Some(block);
    }

    fn pop(&mut self) -> Option<NonNull<FreeBlock>> {
        let block = self.head.take()?;
        self.head = unsafe { (*block.as_ptr()).next.take() };
        self.len -= 1;
        Some(block)
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::{alloc, dealloc};
    use std::vec::Vec;

    const BLOCK_SIZE: usize = 64;
    const NUM_BLOCKS: usize = 8;
    const SLAB_SIZE: usize = BLOCK_SIZE * NUM_BLOCKS;

    struct SlabMemoryGuard {
        ptr: *mut u8,
        layout: Layout,
    }

    impl SlabMemoryGuard {
        fn new(size: usize, align: usize) -> Self {
            let layout = Layout::from_size_align(size, align).expect("invalid layout");
            let ptr = unsafe { alloc(layout) };
            assert!(!ptr.is_null(), "failed to allocate test slab memory");
            Self { ptr, layout }
        }

        fn start_addr(&self) -> usize {
            self.ptr.expose_provenance()
        }
    }

    impl Drop for SlabMemoryGuard {
        fn drop(&mut self) {
            unsafe {
                dealloc(self.ptr, self.layout);
            }
        }
    }

    #[test]
    fn test_init() {
        let mem = SlabMemoryGuard::new(SLAB_SIZE, BLOCK_SIZE);
        let slab = unsafe { Slab::init(mem.start_addr(), SLAB_SIZE, BLOCK_SIZE) };

        assert_eq!(slab.capacity(), NUM_BLOCKS);
        assert_eq!(slab.size(), BLOCK_SIZE);
        assert_eq!(slab.usage(), 0);
    }

    #[test]
    fn test_non_multiple_slab_size() {
        let mem = SlabMemoryGuard::new(100, BLOCK_SIZE);
        let slab = unsafe { Slab::init(mem.start_addr(), 100, BLOCK_SIZE) };

        assert_eq!(slab.capacity(), 1);
        assert_eq!(slab.size(), BLOCK_SIZE);
        assert_eq!(slab.usage(), 0);
    }

    #[test]
    fn test_allocate_and_deallocate_single() {
        let mem = SlabMemoryGuard::new(SLAB_SIZE, BLOCK_SIZE);
        let start = mem.start_addr();
        let mut slab = unsafe { Slab::init(start, SLAB_SIZE, BLOCK_SIZE) };
        let layout = Layout::from_size_align(BLOCK_SIZE, BLOCK_SIZE).unwrap();

        let ptr = slab.allocate(layout).expect("allocation should succeed");
        assert_eq!(slab.usage(), 1);
        assert_eq!(ptr.addr(), start);

        unsafe {
            ptr.write_bytes(0xAB, BLOCK_SIZE);
        }

        slab.deallocate(ptr);
        assert_eq!(slab.usage(), 0);

        let ptr2 = slab.allocate(layout).expect("re-allocation should succeed");
        assert_eq!(ptr2.addr(), start);
        slab.deallocate(ptr2);
    }

    #[test]
    fn test_exhaust_and_reuse() {
        let mem = SlabMemoryGuard::new(SLAB_SIZE, BLOCK_SIZE);
        let start = mem.start_addr();
        let mut slab = unsafe { Slab::init(start, SLAB_SIZE, BLOCK_SIZE) };
        let layout = Layout::from_size_align(BLOCK_SIZE, BLOCK_SIZE).unwrap();

        let mut ptrs = Vec::with_capacity(NUM_BLOCKS);

        for i in 0..NUM_BLOCKS {
            let ptr = slab.allocate(layout).expect("allocation should succeed");
            let addr = ptr.addr();

            assert!(addr >= start && addr + BLOCK_SIZE <= start + SLAB_SIZE);
            assert_eq!(addr % BLOCK_SIZE, 0);
            assert_eq!(addr, start + i * BLOCK_SIZE);

            // Fill the entire block with non-zero bytes to ensure FreeBlock metadata
            // is properly re-initialized upon deallocation.
            unsafe {
                ptr.write_bytes(0xFF, BLOCK_SIZE);
            }

            ptrs.push(ptr);
            assert_eq!(slab.usage(), i + 1);
        }

        // Next allocation must fail when exhausted
        assert!(slab.allocate(layout).is_err());
        assert_eq!(slab.usage(), NUM_BLOCKS);

        // Deallocate all blocks
        for ptr in ptrs {
            slab.deallocate(ptr);
        }
        assert_eq!(slab.usage(), 0);

        // Verify all blocks can be allocated again
        for _ in 0..NUM_BLOCKS {
            let ptr = slab.allocate(layout).expect("re-allocation should succeed");
            unsafe {
                ptr.write_bytes(0x55, BLOCK_SIZE);
            }
        }
        assert_eq!(slab.usage(), NUM_BLOCKS);
        assert!(slab.allocate(layout).is_err());
    }
}
