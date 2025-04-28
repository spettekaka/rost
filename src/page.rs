use crate::mem::Region;
use crate::symbols::{HEAP_SIZE, HEAP_START};
use crate::{print, println};

use core::cell::SyncUnsafeCell;
use core::mem::size_of;
use core::ptr::null_mut;

use log::{info, warn};

pub const PAGE_SIZE: usize = 1 << PAGE_ORDER;
const PAGE_ORDER: usize = 12;
const PAGE_TABLE_ENTRIES: usize = 512;

static mut PAGES: usize = 0;
static mut PAGE_ALLOC_START: usize = 0;

pub static mut KERNEL_PAGE_TABLE: SyncUnsafeCell<PageTable> = SyncUnsafeCell::new(PageTable::new());

unsafe extern "C" {
    static _sheap: u8;
    static _eheap: u8;
    static _heap_size: u8;
}

pub const fn align_val(val: usize, order: usize) -> usize {
    let o = (1usize << order) - 1;
    (val + o) & !o
}

pub const fn align_val_down(val: usize, order: usize) -> usize {
    val & !((1usize << order) - 1)
}

pub const fn align_page_down(val: usize) -> usize {
    align_val_down(val, PAGE_ORDER)
}

pub fn init() {
    info!("Initiating paging");
    unsafe {
        PAGES = HEAP_SIZE() / PAGE_SIZE;
        let ptr = HEAP_START() as *mut Page;

        for i in 0..PAGES {
            (*ptr.add(i)).clear();
        }

        PAGE_ALLOC_START = align_val(HEAP_START() + PAGES * size_of::<Page>(), PAGE_ORDER);
    }
}

pub fn alloc(pages: usize) -> *mut u8 {
    assert!(pages > 0);
    unsafe {
        let ptr = PAGE_ALLOC_START as *mut Page;
        for i in 0..PAGES - pages {
            if !(*ptr.add(i)).is_free() {
                continue;
            }

            let slice = core::slice::from_raw_parts_mut(ptr.add(i) as *mut Page, pages);

            if !slice.iter().all(|page| page.is_free()) {
                continue;
            }

            slice.iter_mut().for_each(|page| page.set(PageFlag::Taken));
            slice[pages - 1].set(PageFlag::Last);

            return (PAGE_ALLOC_START + PAGE_SIZE * i) as *mut u8;
        }
    }
    warn!("alloc: ptr is NULL");
    null_mut()
}

pub fn zalloc(pages: usize) -> *mut u8 {
    let ptr = alloc(pages);
    if !ptr.is_null() {
        let len = (PAGE_SIZE * pages) / 8;
        let slice = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u64, len) };
        slice.iter_mut().for_each(|ptr| *ptr = 0);
    }
    ptr
}

pub fn dealloc(ptr: *mut u8) {
    assert!(!ptr.is_null());

    unsafe {
        let addr = HEAP_START() + (ptr as usize - PAGE_ALLOC_START) / PAGE_SIZE;
        assert!(addr >= HEAP_START() && addr < PAGE_ALLOC_START);
        let mut p = addr as *mut Page;
        assert!(!(*p).is_free());
        while !(*p).is_free() && !(*p).is_last() {
            (*p).clear();
            p = p.add(1);
        }

        assert!((*p).is_last());
        (*p).clear();
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PageFlag {
    Empty = 0,
    Taken = 1 << 0,
    Last = 1 << 1,
}

impl PageFlag {
    pub const fn value(self) -> u8 {
        self as u8
    }
}

#[repr(C)]
#[repr(align(4096))]
pub struct Page {
    pub flag: u8,
}

impl Page {
    pub fn new() -> Self {
        Self {
            flag: PageFlag::Empty.value(),
        }
    }

    pub fn clear(&mut self) {
        self.flag = PageFlag::Empty.value();
    }

    pub fn clear_flag(&mut self, flag: PageFlag) {
        self.flag &= !flag.value();
    }

    pub fn set(&mut self, flag: PageFlag) {
        self.flag |= flag.value();
    }

    pub const fn is_last(&self) -> bool {
        self.flag & PageFlag::Last.value() != 0
    }

    pub const fn is_free(&self) -> bool {
        self.flag & PageFlag::Taken.value() == 0
    }
}

#[repr(C)]
#[repr(align(4096))]
pub struct PageTable {
    pub entries: [Entry; PAGE_TABLE_ENTRIES],
}

#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct Entry(usize);

/* Virtual Page Number */
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct VPN(usize);

/* Physical Page Number */
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct PPN(usize);

#[derive(Debug, Copy, Clone)]
pub enum Attribute {
    Dirty = 1 << 7,
    Accessed = 1 << 6,
    Global = 1 << 5,
    User = 1 << 4,
    Execute = 1 << 3,
    Write = 1 << 2,
    Read = 1 << 1,
    Valid = 1 << 0,
    ReadWrite = 0b11 << 1,
    ReadExecute = 0b101 << 1,
    UserRead = 0b10010,
    UserReadWrite = 0b10110,
    UserReadExecute = 0b11010,
}

impl Entry {
    pub const fn new(ppn: usize, flags: usize) -> Self {
        Self(((ppn & !0xfff) >> 2) | flags)
    }

    pub const fn dirty(&self) -> bool {
        self.0 & Attribute::Dirty as usize != 0
    }

    pub const fn accessed(&self) -> bool {
        self.0 & Attribute::Accessed as usize != 0
    }

    pub const fn global(&self) -> bool {
        self.0 & Attribute::Global as usize != 0
    }

    pub const fn user(&self) -> bool {
        self.0 & Attribute::User as usize != 0
    }

    pub const fn executable(&self) -> bool {
        self.0 & Attribute::Execute as usize != 0
    }

    pub const fn writable(&self) -> bool {
        self.0 & Attribute::Write as usize != 0
    }

    pub const fn readable(&self) -> bool {
        self.0 & Attribute::Read as usize != 0
    }

    pub const fn valid(&self) -> bool {
        self.0 & Attribute::Valid as usize != 0
    }

    pub const fn is_leaf(&self) -> bool {
        self.0 & 0xe != 0
    }

    pub const fn is_branch(&self) -> bool {
        !self.is_leaf()
    }

    pub const fn physical_addr(&self) -> PPN {
        PPN((self.0 & !0x3ff) << 2)
    }

    pub const fn flags(&self) -> usize {
        self.0 & 0x3ff
    }

    pub const fn get_entry(&self) -> usize {
        self.0
    }
}

impl VPN {
    pub const fn vpn0(&self) -> usize {
        (self.0 >> 12) & 0x1ff
    }
    pub const fn vpn1(&self) -> usize {
        (self.0 >> 21) & 0x1ff
    }
    pub const fn vpn2(&self) -> usize {
        (self.0 >> 30) & 0x1ff
    }
    pub fn index(&self, id: usize) -> usize {
        match id {
            0 => self.vpn0(),
            1 => self.vpn1(),
            2 => self.vpn2(),
            _ => unreachable!(),
        }
    }
}

impl PPN {
    pub const fn ppn0(&self) -> usize {
        (self.0 >> 12) & 0x1ff
    }

    pub const fn ppn1(&self) -> usize {
        (self.0 >> 21) & 0x1ff
    }

    pub const fn ppn2(&self) -> usize {
        (self.0 >> 30) & 0x3ff_ffff
    }

    pub fn index(&self, id: usize) -> usize {
        match id {
            0 => self.ppn0(),
            1 => self.ppn1(),
            2 => self.ppn2(),
            _ => unreachable!(),
        }
    }
}

impl PageTable {
    pub const fn new() -> Self {
        Self {
            entries: [Entry(0); PAGE_TABLE_ENTRIES],
        }
    }

    pub const fn len() -> usize {
        PAGE_TABLE_ENTRIES
    }

    pub fn kernel_map(&mut self, vaddr: usize, paddr: usize, flags: usize) {
        if flags & Attribute::User as usize != 0 {
            panic!("Trying to map user page to kernel page table");
        }
        self.map_addr(vaddr, paddr, flags, 0);
    }

    fn map_addr(&mut self, vaddr: usize, paddr: usize, flags: usize, level: usize) {
        assert!(
            paddr % PAGE_SIZE == 0,
            "physical address {:x} not aligned",
            paddr
        );
        assert!(
            vaddr % PAGE_SIZE == 0,
            "virtual address {:x} not aligned",
            paddr
        );

        let vpn = VPN(vaddr);
        let mut v = &mut self.entries[vpn.vpn2()];
        for lvl in (level..2).rev() {
            if !v.valid() {
                let page = zalloc(1);
                *v = Entry::new(page as usize, Attribute::Valid as usize);
            }
            let entry = v.physical_addr().0 as *mut Entry;
            v = unsafe { entry.add(vpn.index(lvl)).as_mut().unwrap() };
        }
        *v = Entry::new(paddr, flags | Attribute::Valid as usize);
    }

    pub fn map_range(&mut self, start: usize, end: usize, vstart: usize, flags: usize) {
        info!("\t{}->{} ({})", start, end, end - start);
        let mut memaddr = start & !(PAGE_SIZE - 1);
        let mut vstart = vstart & !(PAGE_SIZE - 1);

        let pages = (align_val_down(end, 12) - memaddr) / PAGE_SIZE;

        for _ in 0..pages {
            self.map_addr(vstart, memaddr, flags, 0);
            memaddr += 1 << 12;
            vstart += 1 << 12;
        }
    }

    pub fn id_map_range(&mut self, region: &Region) {
        info!(
            "\t{}: {:X}->{:X} ({})",
            region.name(),
            region.start_addr(),
            region.end_addr(),
            region.len()
        );
        let mut memaddr = align_val_down(region.start_addr(), PAGE_ORDER);
        let num_pages = (align_val(region.end_addr(), PAGE_ORDER) - memaddr) / PAGE_SIZE;
        (0..num_pages).for_each(|_| {
            self.map_addr(memaddr, memaddr, region.flags() as usize, 0);
            memaddr += PAGE_SIZE;
        });
    }

    pub fn id_map_ranges<'a, I>(&mut self, arr: I)
    where
        I: Iterator<Item = &'a Region>,
    {
        arr.into_iter().for_each(|region| {
            self.id_map_range(region);
        });
    }

    pub fn map_ranges<I>(&mut self, iter: I)
    where
        I: Iterator<Item = (usize, usize, usize, usize)>,
    {
        iter.for_each(|range| {
            let (start, end, vstart, flags) = range;
            self.map_range(start, end, vstart, flags);
        });
    }

    pub fn mark(&mut self, start: usize, end: usize, flags: usize) {
        let mut memaddr = align_val(start, PAGE_ORDER);
        let mut start = unsafe { start & !(PAGE_ALLOC_START - 1) };

        let pages = (align_val(end, 12) - memaddr) / PAGE_ORDER;

        for _ in 0..pages {
            self.map_addr(start, memaddr, flags, 0);
            memaddr += 1 << 12;
            start += 1 << 12;
        }
    }

    pub fn phy_addr_of(&self, vaddr: usize) -> Option<usize> {
        assert!(
            vaddr % PAGE_SIZE == 0,
            "virtual address {:x} not aligned",
            vaddr
        );

        let vpn = VPN(vaddr);
        let mut v = &self.entries[vpn.vpn2()];
        for lvl in (0..2).rev() {
            if !v.valid() {
                return None;
            }
            let entry = v.physical_addr().0 as *mut Entry;
            v = unsafe { entry.add(vpn.index(lvl)).as_mut().unwrap() };
        }
        Some(v.physical_addr().0)
    }

    pub fn dump(&self) {
        info!("----- Dumping Page Table -----");
        self._dump(2, 0);
        info!("----- End of Page Table -----");
    }

    fn _dump(&self, level: usize, vpn: usize) {
        for (i, &v) in self.entries.iter().enumerate().filter(|(_, v)| v.valid()) {
            if v.is_leaf() {
                let u_flag = if v.user() { "U" } else { "-" };
                let r_flag = if v.readable() { "R" } else { "-" };
                let w_flag = if v.writable() { "W" } else { "-" };
                let x_flag = if v.executable() { "X" } else { "-" };
                let vaddr = (vpn << 9 | i) << 12;
                if vaddr != v.physical_addr().0 || true {
                    for _ in 0..(2 - level) {
                        print!(".");
                    }
                    println!(
                        "{}: 0x{:X} -> 0x{:X} {}{}{}{}",
                        i,
                        vaddr,
                        v.physical_addr().0,
                        u_flag,
                        r_flag,
                        w_flag,
                        x_flag,
                    );
                }
            } else {
                for _ in 0..(2 - level) {
                    print!(".");
                }
                println!(
                    "{}: 0x{:X} -> 0x{:X}",
                    i,
                    (vpn << 0 | i) << (9 * level + 12),
                    v.physical_addr().0
                );

                let table = v.physical_addr().0 as *const Self;
                let table = unsafe { table.as_ref().unwrap() };
                if level != 0 {
                    table._dump(level - 1, (vpn << 9) | i);
                }
            }
        }
    }

    pub fn unmap(root: &mut PageTable) {
        for lv2 in 0..PageTable::len() {
            let ref entry_lv2 = root.entries[lv2];
            if entry_lv2.valid() && entry_lv2.is_branch() {
                let memaddr_lv1 = (entry_lv2.get_entry() & !0x3ff) << 2;

                let table_lv1 = unsafe { (memaddr_lv1 as *mut PageTable).as_mut().unwrap() };

                for lv1 in 0..PageTable::len() {
                    let ref entry_lv1 = table_lv1.entries[lv1];
                    if entry_lv1.valid() && entry_lv1.is_branch() {
                        let memaddr_lv0 = (entry_lv1.get_entry() & !0x3ff) << 2;
                        dealloc(memaddr_lv0 as *mut u8);
                    }
                }
                dealloc(memaddr_lv1 as *mut u8);
            }
        }
    }
}
