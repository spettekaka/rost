use core::ptr::read_volatile;

unsafe extern "C" {
    fn probe_trap_entry();
}

core::arch::global_asm!(
    r#"
    .globl probe_trap_entry
    .align 4
    probe_trap_entry:
        # Save clobbed registers to the stack
        addi sp, sp, -16
        sd a0, 0(sp)
        sd a1, 8(sp)

        csrr a0, scause
        li a1, 5                  # Load Access Fault (5)
        bne a0, a1, 1f            # Other error, jump to unhandle trap
        
        # Fault recovered: skip faulting 'lw' instruction (4 bytes)
        csrr a0, sepc
        addi a0, a0, 4
        csrw sepc, a0
        
        li a0, 0                  # Return 0 to signal unpopulated slot
        sd a0, 0(sp)

        # Restore registers and return from trap
        ld a0, 0(sp)
        ld a1, 8(sp)
        addi sp, sp, 16
        sret
        
    1:
        # Jump to default trap handler for other causes
        # Restore registers to exact pre-trap state
        ld a0, 0(sp)
        ld a1, 8(sp)
        addi sp, sp, 16

        # Jump directly to your main kernel trap handler entry point
        j _start_trap
        # call _start_trap
        # call machine_trap
    "#
);

/// Safe mmio read
///
/// Reads a u32 from the specified address in a dedicated interrupt context to catch LOAD access
/// faults. Allows probing MMIO address spaces and recovery if address is not mapped.
pub unsafe fn safe_mmio_read_u32(addr: *const u32) -> Option<u32> {
    let old_stvec: usize;

    unsafe {
        // Save old stvec
        core::arch::asm!("csrr {}, stvec", out(reg) old_stvec);

        // Set new stvec for MMIO probing
        core::arch::asm!("csrw stvec, {}", in(reg) probe_trap_entry as *const () as usize);

        // Perform read
        let val = read_volatile(addr as *const u32);

        // Restore stvec
        core::arch::asm!("csrw stvec, {}", in(reg) old_stvec);

        if val == 0 { None } else { Some(val) }
    }
}
