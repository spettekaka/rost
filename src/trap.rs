use crate::arch;
use crate::interrupt;

use log::warn;
use riscv::register;

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

#[no_mangle]
extern "C" fn machine_trap() {
    let epc = register::mepc::read();
    let tval = register::mtval::read();
    let cause = register::mcause::read();
    let hart = register::mhartid::read();
    let status = register::mstatus::read();
    let mut sstatus_bits: usize;
    unsafe {
        asm!("csrr {}, mstatus", out(reg) sstatus_bits);
    }

    let is_interrupt = cause.is_interrupt();
    let cause = cause.code();

    if status.spp() != register::mstatus::SPP::Supervisor {
        warn!("not from supervisor mode,  hart {}", hart);
    }

    unsafe {
        register::mstatus::clear_sie();
    }

    if arch::riscv::intr_get() {
        panic!("interrupt not disabled");
    }

    if is_interrupt {
        // handle device interrupt from PLIC
        interrupt::handle_interrupt(cause as u32);
    } else {
        // handle synchronous interrupt or exception
        match cause {
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
                cause, hart, epc, tval
            ),
        }
    }

    unsafe {
        register::mstatus::set_sie();
    }

    register::sepc::write(epc);
    unsafe {
        asm!("csrw sstatus, {}", in(reg) sstatus_bits);
    }
}

pub unsafe fn hartinit() {
    register::stvec::write(_start_trap as usize, register::stvec::TrapMode::Direct);
    register::mtvec::write(_start_trap as usize, register::stvec::TrapMode::Direct);
}

extern "C" {
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

    mret
"#
);
