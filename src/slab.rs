use core::alloc::{AllocError, Layout};
use core::ptr;

pub struct Slab {
    capacity: usize,
    size: usize,
    free_list: FreeList,
}

#[derive(Default)]
struct FreeList {
    len: usize,
    head: Option<&'static mut FreeBlock>,
}

#[derive(Default)]
struct FreeBlock {
    next: Option<&'static mut FreeBlock>,
}

impl Slab {
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
            Some(block) => Ok(block.addr() as *mut u8),
            None => Err(AllocError),
        }
    }

    pub fn deallocate(&mut self, ptr: *mut u8) {
        let ptr = ptr as *mut FreeBlock;
        unsafe {
            self.free_list.push(&mut *ptr);
        }
    }
}

impl FreeList {
    unsafe fn new(start_addr: usize, blocks: usize, block_size: usize) -> Self {
        let mut free_list = Self { len: 0, head: None };

        for i in (0..blocks).rev() {
            let block = (start_addr + i * block_size) as *mut FreeBlock;
            unsafe { free_list.push(&mut *block) };
        }
        free_list
    }

    fn push(&mut self, block: &'static mut FreeBlock) {
        block.next = self.head.take();
        self.len += 1;
        self.head = Some(block);
    }

    fn pop(&mut self) -> Option<&'static mut FreeBlock> {
        self.head.take().map(|block| {
            self.head = block.next.take();
            self.len -= 1;
            block
        })
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

impl FreeBlock {
    fn addr(&self) -> usize {
        ptr::addr_of!(self) as usize
    }
}
