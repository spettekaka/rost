#![allow(non_snake_case)]
use crate::println;

unsafe extern "C" {
    static __sheap: u8;
    static __eheap: u8;
    static _heap_size: u8;
    static __sbss: u8;
    static __ebss: u8;
    static __srodata: u8;
    static __sdata: u8;
    static __edata: u8;
    static __sstack: u8;
    static __estack: u8;
    static __stext: u8;
}

pub fn KERNEL_STACK_START() -> usize {
    unsafe {
        return &__sstack as *const u8 as usize;
    }
}

pub fn KERNEL_STACK_END() -> usize {
    unsafe {
        return &__estack as *const u8 as usize;
    }
}

pub fn HEAP_START() -> usize {
    unsafe {
        return &__sheap as *const u8 as usize;
    }
}

pub fn HEAP_END() -> usize {
    unsafe {
        return &__eheap as *const u8 as usize;
    }
}

pub fn HEAP_SIZE() -> usize {
    unsafe {
        return &_heap_size as *const u8 as usize;
    }
}

pub fn TEXT_START() -> usize {
    unsafe {
        return &__stext as *const u8 as usize;
    }
}

pub fn RODATA_START() -> usize {
    unsafe {
        return &__sdata as *const u8 as usize;
    }
}

pub fn RODATA_END() -> usize {
    unsafe {
        return &__edata as *const u8 as usize;
    }
}
pub fn DATA_START() -> usize {
    unsafe {
        return &__sdata as *const u8 as usize;
    }
}

pub fn DATA_END() -> usize {
    unsafe {
        return &__edata as *const u8 as usize;
    }
}

pub fn BSS_START() -> usize {
    unsafe {
        return &__sbss as *const u8 as usize;
    }
}

pub fn BSS_END() -> usize {
    unsafe {
        return &__ebss as *const u8 as usize;
    }
}

pub fn dump_symbols() {
    println!("Symbols:");
    println!("\tHeap start:         0x{:X}", HEAP_START());
    println!("\tHeap end:           0x{:X}", HEAP_END());
    println!("\tHeap size:          0x{:X}", HEAP_SIZE());
    println!("\tKernel stack start: 0x{:X}", KERNEL_STACK_START());
    println!("\tKernel stack end:   0x{:X}", KERNEL_STACK_END());
    println!("\tText start:         0x{:X}", TEXT_START());
    println!("\tRO Data start:      0x{:X}", RODATA_START());
    println!("\tRO Data end:        0x{:X}", RODATA_END());
    println!("\tData start:         0x{:X}", DATA_START());
    println!("\tData end:           0x{:X}", DATA_END());
    println!("\tBss start:          0x{:X}", BSS_START());
    println!("\tBss end:            0x{:X}", BSS_END());
}
