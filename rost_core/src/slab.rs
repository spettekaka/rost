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
