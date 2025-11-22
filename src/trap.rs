use crate::arch;
use crate::interrupt;

use core::arch::{asm, global_asm};

use log::info;
use riscv::register;
use riscv::register::stvec::Stvec;

#[derive(Debug, Copy, Clone)]
pub enum Trap {
    UserSoftwareInterrupt,
    SupervisorSoftwareInterrupt,
    MachineSofrwareInterrupt,
    UserTimerInterrupt,
    SupervisorTimerInterrupt,
    MachineTimerInterrupt,
    UserExternalInterrupt,
    SupervisorExternalInterrupt,
    MachineExternalInterrupt,

    InstructionAddressMisaligned,
    InstructionAccesFault,
    IllegalInstruction,
    Breakpoint,
    LoadAddressMisaligned,
    LoadAccessFault,
    StoreAddressMisaligned,
    StoreAccessFault,
    UserModeEnvironmentCall,
    SupervisorModeEvironmentCall,
    MachineModeEnvironmentCall,
    InstructionPageFault,
    LoadPageFault,
    StorePageFault,
    Reserved,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn machine_trap() {
    let epc = register::sepc::read();
    let tval = register::stval::read();
    let cause = register::scause::read();
    let hart = arch::riscv::thread_pointer();
    let status = register::sstatus::read();
    let mut sstatus_bits: usize;
    unsafe {
        asm!("csrr {}, sstatus", out(reg) sstatus_bits);
    }

    if status.spp() != register::sstatus::SPP::Supervisor {
        panic!("not from supervisor mode,  hart {}", hart);
    }

    unsafe {
        register::sstatus::clear_sie();
    }

    if arch::riscv::intr_get() {
        panic!("interrupt not disabled");
    }

    if cause.is_interrupt() {
        // handle device interrupt from PLIC
        interrupt::handle_interrupt(cause.code() as u32);
    } else {
        // handle synchronous interrupt or exception
        match cause.code() {
            0 => panic!(
                "Instruction address misaligned CPU#{} -> 0x{:08x}: 0x{:08x}",
                hart, epc, tval
            ),
            1 => panic!("Instruction access fault CPU#{}", hart),
            2 => panic!(
                "Illegal instruction CPU#{} -> 0x{:08x}: 0x{:08x}",
                hart, epc, tval
            ),
            3 => panic!("Breakpoint CPU#{}", hart),
            4 => panic!("Load address misaligned CPU#{}", hart),
            5 => panic!("Load access fault CPU#{}", hart),
            8 => panic!(
                "Enviroment call from user mode CPU#{} -> 0x{:08x}",
                hart, epc
            ),
            9 => panic!(
                "Enviroment call from Supervisor mode CPU#{} -> 0x{:08x}",
                hart, epc
            ),
            11 => panic!(
                "Enviroment call from Machine mode CPU#{} -> 0x{:08x}",
                hart, epc
            ),
            12 => panic!(
                "Instruction Page fault CPU#{} -> 0x{:08x}: 0x{:08x}",
                hart, epc, tval
            ),
            13 => panic!(
                "Load Page fault CPU#{} -> 0x{:08x}: 0x{:08x}",
                hart, epc, tval
            ),
            15 => panic!(
                "Store Page fault CPU#{} -> 0x{:08x}: 0x{:08x}",
                hart, epc, tval
            ),
            _ => panic!(
                "Unhandled sync trap {}. CPU#{} -> 0x{:08x}: 0x{:08x}",
                cause.code(),
                hart,
                epc,
                tval
            ),
        }
    }

    unsafe {
        register::sstatus::set_sie();

        register::sepc::write(epc);
        asm!("csrw sstatus, {}", in(reg) sstatus_bits);
    }
}

/// Enable Interrupts
///
/// We handle all interrupts in supervisor mode
pub unsafe fn enable_interrupts() {
    info!("Enabling interrutps");
    // enable interrupts
    unsafe {
        register::sstatus::set_sie();
        register::sstatus::set_spp(register::sstatus::SPP::Supervisor);
        // enable software interrupt
        register::sie::set_ssoft();
        register::sie::set_sext();
        register::sie::set_stimer();
    }
}

/// Hart init
///
/// Set the vector for handling supervisor mode
pub unsafe fn hartinit() {
    let mut stvec_data = Stvec::from_bits(0);
    stvec_data.set_address(_start_trap as *const () as usize);
    stvec_data.set_trap_mode(register::stvec::TrapMode::Direct);
    unsafe { register::stvec::write(stvec_data) };
}

unsafe extern "C" {
    fn _start_trap();
}

global_asm!(
    r#"
.global _start_trap
.align 4
_start_trap:
    addi sp, sp, -256

    sd ra, 0(sp)
    sd sp, 8(sp)
    sd gp, 16(sp)
    sd tp, 24(sp)
    sd t0, 32(sp)
    sd t1, 40(sp)
    sd t2, 48(sp)
    sd s0, 56(sp)
    sd s1, 64(sp)
    sd a0, 72(sp)
    sd a1, 80(sp)
    sd a2, 88(sp)
    sd a3, 96(sp)
    sd a4, 104(sp)
    sd a5, 112(sp)
    sd a6, 120(sp)
    sd a7, 128(sp)
    sd s2, 136(sp)
    sd s3, 144(sp)
    sd s4, 152(sp)
    sd s5, 160(sp)
    sd s6, 168(sp)
    sd s7, 176(sp)
    sd s8, 184(sp)
    sd s9, 192(sp)
    sd s10, 200(sp)
    sd s11, 208(sp)
    sd t3, 216(sp)
    sd t4, 224(sp)
    sd t5, 232(sp)
    sd t6, 240(sp)

    call machine_trap

    ld ra, 0(sp)
    ld sp, 8(sp)
    ld gp, 16(sp)
    ld t0, 32(sp)
    ld t1, 40(sp)
    ld t2, 48(sp)
    ld s0, 56(sp)
    ld s1, 64(sp)
    ld a0, 72(sp)
    ld a1, 80(sp)
    ld a2, 88(sp)
    ld a3, 96(sp)
    ld a4, 104(sp)
    ld a5, 112(sp)
    ld a6, 120(sp)
    ld a7, 128(sp)
    ld s2, 136(sp)
    ld s3, 144(sp)
    ld s4, 152(sp)
    ld s5, 160(sp)
    ld s6, 168(sp)
    ld s7, 176(sp)
    ld s8, 184(sp)
    ld s9, 192(sp)
    ld s10, 200(sp)
    ld s11, 208(sp)
    ld t3, 216(sp)
    ld t4, 224(sp)
    ld t5, 232(sp)
    ld t6, 240(sp)

    addi sp, sp, 256

    sret
"#
);

global_asm!(
    r#"
.global timervec
.align 4
timervec:
    # scratch[0, 8, 16] : register save area
    # scratch[24] : address of MTIMECMP
    # scratch[32] : desired interval between interrupts

    csrrw a0, mscratch, a0
    sd a1, 0(a0)
    sd a2, 8(a0)
    sd a3, 16(a0)

    # schedule next timer interrupt
    # this is done by adding `interval` to mtimecmp
    ld a1, 24(a0) # MTIMECMP current hart
    ld a2, 32(a0) # interval
    ld a3, 0(a1)
    add a3, a3, a2
    sd a3, 0(a1)

    # raise supervisor software interrupt
    li a1, 0x02
    csrw sip, a1

    ld a3, 16(a0)
    ld a2, 8(a0)
    ld a1, 0(a0)
    csrrw a0, mscratch, a0

    mret
"#
);
