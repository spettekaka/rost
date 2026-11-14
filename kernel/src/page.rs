use crate::mem::Region;
use crate::symbols::{HEAP_SIZE, HEAP_START};
use crate::{print, println};

use rost_core::atomic_bitmap::AtomicPageAllocator;
use rost_core::consts::{PAGE_ORDER, PAGE_SIZE};

use core::ptr::null_mut;

use log::{info, warn};

const PAGE_TABLE_ENTRIES: usize = 512;
const PTE_PPN_MASK: usize = 0x003f_ffff_ffff_fc00; // Sv39 PTE bits 53..10
const PTE_FLAG_MASK: usize = 0x3ff;

const LEVEL_SIZES: [usize; 3] = [
    1 << 12, // level 0: 4 KiB
    1 << 21, // level 1: 2 MiB megapage
    1 << 30, // level 2: 1 GiB gigapage
];

static PAGE_ALLOCATOR: AtomicPageAllocator = AtomicPageAllocator::new();

pub fn allocator() -> &'static AtomicPageAllocator {
    &PAGE_ALLOCATOR
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
    PAGE_ALLOCATOR.init(HEAP_START(), HEAP_START() + HEAP_SIZE());
}

pub fn alloc(pages: usize) -> *mut u8 {
    PAGE_ALLOCATOR.alloc(pages).unwrap_or_else(|| {
        warn!("alloc: out of memory");
        null_mut()
    })
}

pub fn zalloc(pages: usize) -> *mut u8 {
    let ptr = alloc(pages);
    if !ptr.is_null() {
        let len = (PAGE_SIZE * pages) / 8;
        let slice = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u64, len) };
        slice.iter_mut().for_each(|word| *word = 0);
    }
    ptr
}

pub fn dealloc(ptr: *mut u8, pages: usize) {
    PAGE_ALLOCATOR.dealloc(ptr, pages);
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PageError {
    OutOfMemory,
    UnalignedAddress,
    InvalidFlags,
    UserPageInKernelTable,
    SuperpageConflict,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct VirtAddr(pub usize);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct PhysAddr(pub usize);

impl VirtAddr {
    #[inline]
    pub const fn vpn(self, level: usize) -> usize {
        (self.0 >> (PAGE_ORDER + 9 * level)) & 0x1ff
    }

    #[inline]
    pub const fn offset(self, level: usize) -> usize {
        let mask = (1usize << (PAGE_ORDER + 9 * level)) - 1;
        self.0 & mask
    }

    #[inline]
    pub const fn is_aligned_to_level(self, level: usize) -> bool {
        self.offset(level) == 0
    }
}

impl PhysAddr {
    #[inline]
    pub const fn is_aligned_to_level(self, level: usize) -> bool {
        let mask = (1usize << (PAGE_ORDER + 9 * level)) - 1;
        (self.0 & mask) == 0
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(transparent)]
pub struct PteFlags(pub usize);

impl PteFlags {
    pub const VALID: Self = Self(1 << 0);
    pub const READ: Self = Self(1 << 1);
    pub const WRITE: Self = Self(1 << 2);
    pub const EXECUTE: Self = Self(1 << 3);
    pub const USER: Self = Self(1 << 4);
    pub const GLOBAL: Self = Self(1 << 5);
    pub const ACCESSED: Self = Self(1 << 6);
    pub const DIRTY: Self = Self(1 << 7);

    // Kernel presets include V, G, A (and D when writable) for Sv39 compliance.
    pub const KERNEL_RO: Self =
        Self(Self::VALID.0 | Self::READ.0 | Self::GLOBAL.0 | Self::ACCESSED.0);
    pub const KERNEL_RW: Self = Self(
        Self::VALID.0
            | Self::READ.0
            | Self::WRITE.0
            | Self::GLOBAL.0
            | Self::ACCESSED.0
            | Self::DIRTY.0,
    );
    pub const KERNEL_RX: Self =
        Self(Self::VALID.0 | Self::READ.0 | Self::EXECUTE.0 | Self::GLOBAL.0 | Self::ACCESSED.0);
    pub const MMIO: Self = Self::KERNEL_RW;

    pub const USER_RO: Self = Self(Self::VALID.0 | Self::USER.0 | Self::READ.0 | Self::ACCESSED.0);
    pub const USER_RW: Self = Self(
        Self::VALID.0
            | Self::USER.0
            | Self::READ.0
            | Self::WRITE.0
            | Self::ACCESSED.0
            | Self::DIRTY.0,
    );
    pub const USER_RX: Self =
        Self(Self::VALID.0 | Self::USER.0 | Self::READ.0 | Self::EXECUTE.0 | Self::ACCESSED.0);

    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Sv39 reserves leaf PTEs with W=1 and R=0.
    #[inline]
    pub const fn is_valid_leaf(self) -> bool {
        let rwx = self.0 & (Self::READ.0 | Self::WRITE.0 | Self::EXECUTE.0);
        rwx != 0 && !(self.contains(Self::WRITE) && !self.contains(Self::READ))
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

impl Entry {
    pub const fn new(paddr: PhysAddr, flags: PteFlags) -> Self {
        Self(((paddr.0 >> 2) & PTE_PPN_MASK) | (flags.0 & PTE_FLAG_MASK))
    }

    pub const fn dirty(self) -> bool {
        self.flags().contains(PteFlags::DIRTY)
    }

    pub const fn accessed(self) -> bool {
        self.flags().contains(PteFlags::ACCESSED)
    }

    pub const fn global(self) -> bool {
        self.flags().contains(PteFlags::GLOBAL)
    }

    pub const fn user(self) -> bool {
        self.flags().contains(PteFlags::USER)
    }

    pub const fn executable(self) -> bool {
        self.flags().contains(PteFlags::EXECUTE)
    }

    pub const fn writable(self) -> bool {
        self.flags().contains(PteFlags::WRITE)
    }

    pub const fn readable(self) -> bool {
        self.flags().contains(PteFlags::READ)
    }

    pub const fn valid(self) -> bool {
        self.flags().contains(PteFlags::VALID)
    }

    pub const fn is_leaf(self) -> bool {
        self.0 & 0xe != 0
    }

    pub const fn is_branch(self) -> bool {
        !self.is_leaf()
    }

    pub const fn physical_addr(self) -> PhysAddr {
        PhysAddr((self.0 & PTE_PPN_MASK) << 2)
    }

    pub const fn flags(self) -> PteFlags {
        PteFlags(self.0 & PTE_FLAG_MASK)
    }

    pub const fn get_entry(self) -> usize {
        self.0
    }
}

#[inline]
const fn phys_to_virt(paddr: PhysAddr) -> *mut PageTable {
    // Identity-mapped for now; update here if moving to a higher-half direct map.
    paddr.0 as *mut PageTable
}

impl Default for PageTable {
    fn default() -> Self {
        Self::new()
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

    pub fn kernel_map(
        &mut self,
        vaddr: VirtAddr,
        paddr: PhysAddr,
        flags: PteFlags,
    ) -> Result<(), PageError> {
        if flags.contains(PteFlags::USER) {
            return Err(PageError::UserPageInKernelTable);
        }
        self.map_addr(vaddr, paddr, flags.union(PteFlags::GLOBAL), 0)
    }

    pub fn map_addr(
        &mut self,
        vaddr: VirtAddr,
        paddr: PhysAddr,
        flags: PteFlags,
        level: usize,
    ) -> Result<(), PageError> {
        self.map_addr_inner(vaddr, paddr, flags, level, false)
    }

    fn map_addr_inner(
        &mut self,
        vaddr: VirtAddr,
        paddr: PhysAddr,
        flags: PteFlags,
        level: usize,
        merge_existing: bool,
    ) -> Result<(), PageError> {
        if level > 2 || !vaddr.is_aligned_to_level(level) || !paddr.is_aligned_to_level(level) {
            return Err(PageError::UnalignedAddress);
        }
        if !flags.is_valid_leaf() {
            return Err(PageError::InvalidFlags);
        }

        let mut table = self as *mut PageTable;
        for lvl in ((level + 1)..=2).rev() {
            let entry = unsafe { &mut (*table).entries[vaddr.vpn(lvl)] };
            if !entry.valid() {
                let page = zalloc(1);
                if page.is_null() {
                    return Err(PageError::OutOfMemory);
                }
                *entry = Entry::new(PhysAddr(page as usize), PteFlags::VALID);
            } else if entry.is_leaf() {
                return Err(PageError::SuperpageConflict);
            }
            table = phys_to_virt(entry.physical_addr());
        }

        let leaf = unsafe { &mut (*table).entries[vaddr.vpn(level)] };
        if leaf.valid() && leaf.is_branch() {
            return Err(PageError::SuperpageConflict);
        }

        let merged_flags =
            if merge_existing && leaf.valid() && leaf.is_leaf() && leaf.physical_addr() == paddr {
                leaf.flags().union(flags)
            } else {
                flags
            };
        *leaf = Entry::new(paddr, merged_flags.union(PteFlags::VALID));
        Ok(())
    }

    /// Maps `[start, end)` -> `[vstart, ...)` using 1 GiB / 2 MiB superpages where aligned.
    pub fn map_range(
        &mut self,
        start: usize,
        end: usize,
        vstart: usize,
        flags: PteFlags,
    ) -> Result<(), PageError> {
        self.map_range_inner(start, end, vstart, flags, false)
    }

    fn map_range_inner(
        &mut self,
        start: usize,
        end: usize,
        vstart: usize,
        flags: PteFlags,
        merge_existing: bool,
    ) -> Result<(), PageError> {
        if end <= start {
            return Ok(());
        }

        let mut paddr = align_val_down(start, PAGE_ORDER);
        let mut vaddr = align_val_down(vstart, PAGE_ORDER);
        let pend = align_val(end, PAGE_ORDER);

        while paddr < pend {
            let remaining = pend - paddr;
            let level = (1..=2)
                .rev()
                .find(|&lvl| {
                    remaining >= LEVEL_SIZES[lvl]
                        && PhysAddr(paddr).is_aligned_to_level(lvl)
                        && VirtAddr(vaddr).is_aligned_to_level(lvl)
                })
                .unwrap_or(0);

            self.map_addr_inner(
                VirtAddr(vaddr),
                PhysAddr(paddr),
                flags,
                level,
                merge_existing,
            )?;
            paddr += LEVEL_SIZES[level];
            vaddr += LEVEL_SIZES[level];
        }
        Ok(())
    }

    pub fn id_map_range(&mut self, region: &Region) -> Result<(), PageError> {
        info!(
            "\t{}: {:X}->{:X} ({})",
            region.name(),
            region.start_addr(),
            region.end_addr(),
            region.len()
        );
        // Merge permissions if adjacent linker sections share a 4 KiB boundary page.
        self.map_range_inner(
            region.start_addr(),
            region.end_addr(),
            region.start_addr(),
            region.flags(),
            true,
        )
    }

    pub fn id_map_ranges<'a, I>(&mut self, arr: I) -> Result<(), PageError>
    where
        I: Iterator<Item = &'a Region>,
    {
        for region in arr {
            self.id_map_range(region)?;
        }
        Ok(())
    }

    pub fn map_ranges<I>(&mut self, iter: I) -> Result<(), PageError>
    where
        I: Iterator<Item = (usize, usize, usize, PteFlags)>,
    {
        for (start, end, vstart, flags) in iter {
            self.map_range(start, end, vstart, flags)?;
        }
        Ok(())
    }

    pub fn mark(&mut self, start: usize, end: usize, flags: PteFlags) -> Result<(), PageError> {
        self.map_range(start, end, start, flags)
    }

    /// Translates any virtual address (including non-page-aligned and superpage addresses).
    pub fn phy_addr_of(&self, vaddr: VirtAddr) -> Option<PhysAddr> {
        let mut table = self as *const PageTable;
        for lvl in (0..=2).rev() {
            let entry = unsafe { (*table).entries[vaddr.vpn(lvl)] };
            if !entry.valid() {
                return None;
            }
            if entry.is_leaf() {
                return Some(PhysAddr(entry.physical_addr().0 | vaddr.offset(lvl)));
            }
            if lvl == 0 {
                return None;
            }
            table = phys_to_virt(entry.physical_addr()) as *const PageTable;
        }
        None
    }

    /// Unmaps a single leaf mapping containing `vaddr` and invalidates its TLB entry.
    pub fn unmap_addr(&mut self, vaddr: VirtAddr) -> bool {
        let mut table = self as *mut PageTable;
        for lvl in (0..=2).rev() {
            let entry = unsafe { &mut (*table).entries[vaddr.vpn(lvl)] };
            if !entry.valid() {
                return false;
            }
            if entry.is_leaf() {
                *entry = Entry(0);
                riscv::asm::sfence_vma(0, vaddr.0);
                return true;
            }
            if lvl == 0 {
                return false;
            }
            table = phys_to_virt(entry.physical_addr());
        }
        false
    }

    pub fn dump(&self) {
        info!("----- Dumping Page Table -----");
        self._dump(2, 0);
        info!("----- End of Page Table -----");
    }

    fn _dump(&self, level: usize, vpn_prefix: usize) {
        for (i, &v) in self.entries.iter().enumerate().filter(|(_, v)| v.valid()) {
            let full_vpn = (vpn_prefix << 9) | i;
            let raw_vaddr = full_vpn << (9 * level + PAGE_ORDER);
            // Sv39 canonical sign extension from bit 38.
            let vaddr = ((raw_vaddr as isize) << 25 >> 25) as usize;

            if v.is_leaf() {
                let u_flag = if v.user() { "U" } else { "-" };
                let r_flag = if v.readable() { "R" } else { "-" };
                let w_flag = if v.writable() { "W" } else { "-" };
                let x_flag = if v.executable() { "X" } else { "-" };
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
            } else {
                for _ in 0..(2 - level) {
                    print!(".");
                }
                println!("{}: 0x{:X} -> 0x{:X}", i, vaddr, v.physical_addr().0);

                if level != 0 {
                    let table = unsafe { &*phys_to_virt(v.physical_addr()) };
                    table._dump(level - 1, full_vpn);
                }
            }
        }
    }

    /// Frees all child page-table pages and clears every entry in `self`.
    pub fn clear(&mut self) {
        for lv2 in self.entries.iter_mut() {
            if lv2.valid() && lv2.is_branch() {
                let table_lv1 = unsafe { &mut *phys_to_virt(lv2.physical_addr()) };
                for lv1 in table_lv1.entries.iter_mut() {
                    if lv1.valid() && lv1.is_branch() {
                        dealloc(phys_to_virt(lv1.physical_addr()) as *mut u8, 1);
                        *lv1 = Entry(0);
                    }
                }
                dealloc(table_lv1 as *mut PageTable as *mut u8, 1);
            }
            *lv2 = Entry(0);
        }
    }
}
