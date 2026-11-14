use crate::clint::{CLINT_BASE, CLINT_SIZE};
use crate::page::{self, PageTable, PteFlags};
use crate::plic::PLIC_BASE;
use crate::symbols::*;
use crate::uart;

use rost_core::arch::{self, riscv::SatpMode};
use rost_core::consts::PAGE_SIZE;
use rost_core::sync::spinlock::Mutex;
use rost_core::virtio::{VIRTIO_MMIO_BASE, VIRTIO_MMIO_STRIDE, VIRTIO_SLOTS};

use log::info;

use core::arch::asm;

static KERNEL_PAGE_TABLE: Mutex<PageTable> = Mutex::new(PageTable::new());

pub struct Region {
    start: usize,
    end: usize,
    flags: PteFlags,
    name: &'static str,
}

#[allow(clippy::len_without_is_empty)]
impl Region {
    pub fn new(start: usize, end: usize, flags: PteFlags, name: &'static str) -> Self {
        Self {
            start,
            end,
            flags,
            name,
        }
    }

    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn start_addr(&self) -> usize {
        self.start
    }

    pub fn end_addr(&self) -> usize {
        self.end
    }

    pub fn flags(&self) -> PteFlags {
        self.flags
    }

    pub fn name(&self) -> &str {
        self.name
    }
}

/// # Safety
///
/// Valid for target riscv. Assumes all symbols are available in the kernel
pub unsafe fn init() {
    info!("Initiating memory");
    page::init();

    let regions = [
        Region::new(TEXT_START(), RODATA_START(), PteFlags::KERNEL_RX, "Text"),
        Region::new(RODATA_START(), RODATA_END(), PteFlags::KERNEL_RO, "RODATA"),
        Region::new(DATA_START(), DATA_END(), PteFlags::KERNEL_RW, "DATA"),
        Region::new(BSS_START(), BSS_END(), PteFlags::KERNEL_RW, "BSS"),
        Region::new(
            KERNEL_STACK_END(),
            KERNEL_STACK_START(),
            PteFlags::KERNEL_RW,
            "KERNEL_STACK",
        ),
        Region::new(HEAP_START(), HEAP_END(), PteFlags::KERNEL_RW, "Heap"),
        Region::new(
            uart::UART_BASE_ADDR,
            uart::UART_BASE_ADDR + PAGE_SIZE,
            PteFlags::MMIO,
            "Uart",
        ),
        Region::new(
            PLIC_BASE,
            PLIC_BASE + 0x400_000, // QEMU PLIC range
            PteFlags::MMIO,
            "PLIC_BASE",
        ),
        Region::new(CLINT_BASE, CLINT_BASE + CLINT_SIZE, PteFlags::MMIO, "CLINT"),
        Region::new(
            VIRTIO_MMIO_BASE,
            VIRTIO_MMIO_BASE + (VIRTIO_SLOTS * VIRTIO_MMIO_STRIDE),
            PteFlags::MMIO,
            "VirtIO",
        ),
    ];

    info!("Mapping the kernel");
    KERNEL_PAGE_TABLE.with_lock(|pgtable| {
        pgtable
            .id_map_ranges(regions.iter())
            .expect("Failed to map kernel memory regions");
    });
}

pub fn enable_mmu() {
    info!("Enabling mmu");
    let root_paddr = KERNEL_PAGE_TABLE.with_lock(|pgtable| pgtable as *const PageTable as usize);
    let satp_val = arch::riscv::build_satp(SatpMode::Sv39, 0, root_paddr);
    unsafe {
        asm!("csrw satp, {}", in(reg) satp_val);
        riscv::asm::sfence_vma(0, 0);
    }
}
