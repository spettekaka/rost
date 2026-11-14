use core::alloc::{AllocError, Layout};
use core::ptr::NonNull;

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
    /// `base` must be non-null, aligned to `block_size`, and valid for writes across
    /// `slab_size` bytes.
    pub unsafe fn init(base: *mut u8, slab_size: usize, block_size: usize) -> Self {
        let mut slab = Self::new(block_size);
        unsafe { slab.grow(base, slab_size) };
        slab
    }

    /// Create a new empty slab
    pub const fn new(block_size: usize) -> Self {
        Self {
            capacity: 0,
            size: block_size,
            free_list: FreeList { len: 0, head: None },
        }
    }

    /// # Safety
    ///
    /// `base` must be non-null, aligned to `self.size`, and valid for writes across
    /// `bytes` bytes.
    pub unsafe fn grow(&mut self, base: *mut u8, bytes: usize) {
        let blocks = bytes / self.size;
        for i in (0..blocks).rev() {
            let ptr = unsafe { base.add(i * self.size) }.cast::<FreeBlock>();
            let block = NonNull::new(ptr).expect("slab block address cannot be null");
            unsafe {
                self.free_list.push(block);
            }
        }
        self.capacity += blocks;
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

        fn as_ptr(&self) -> *mut u8 {
            self.ptr
        }

        fn start_addr(&self) -> usize {
            self.ptr.addr()
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
        let slab = unsafe { Slab::init(mem.as_ptr(), SLAB_SIZE, BLOCK_SIZE) };

        assert_eq!(slab.capacity(), NUM_BLOCKS);
        assert_eq!(slab.size(), BLOCK_SIZE);
        assert_eq!(slab.usage(), 0);
    }

    #[test]
    fn test_non_multiple_slab_size() {
        let mem = SlabMemoryGuard::new(100, BLOCK_SIZE);
        let slab = unsafe { Slab::init(mem.as_ptr(), 100, BLOCK_SIZE) };

        assert_eq!(slab.capacity(), 1);
        assert_eq!(slab.size(), BLOCK_SIZE);
        assert_eq!(slab.usage(), 0);
    }

    #[test]
    fn test_allocate_and_deallocate_single() {
        let mem = SlabMemoryGuard::new(SLAB_SIZE, BLOCK_SIZE);
        let start = mem.start_addr();
        let mut slab = unsafe { Slab::init(mem.as_ptr(), SLAB_SIZE, BLOCK_SIZE) };
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
        let mut slab = unsafe { Slab::init(mem.as_ptr(), SLAB_SIZE, BLOCK_SIZE) };
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

    #[test]
    fn test_new_and_grow() {
        let mut slab = Slab::new(BLOCK_SIZE);
        let layout = Layout::from_size_align(BLOCK_SIZE, BLOCK_SIZE).unwrap();
        assert_eq!(slab.capacity(), 0);
        assert!(slab.allocate(layout).is_err());

        let mem1 = SlabMemoryGuard::new(SLAB_SIZE, BLOCK_SIZE);
        let mem2 = SlabMemoryGuard::new(SLAB_SIZE, BLOCK_SIZE);
        unsafe {
            slab.grow(mem1.as_ptr(), SLAB_SIZE);
            slab.grow(mem2.as_ptr(), SLAB_SIZE);
        }
        assert_eq!(slab.capacity(), NUM_BLOCKS * 2);

        let mut ptrs = Vec::with_capacity(NUM_BLOCKS * 2);
        for _ in 0..(NUM_BLOCKS * 2) {
            let ptr = slab.allocate(layout).expect("allocation should succeed");
            unsafe {
                ptr.write_bytes(0xCC, BLOCK_SIZE);
            }
            ptrs.push(ptr);
        }
        assert!(slab.allocate(layout).is_err());

        for ptr in ptrs {
            slab.deallocate(ptr);
        }
        assert_eq!(slab.usage(), 0);
    }
}
