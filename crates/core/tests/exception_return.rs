//! Independent ARM `S=1, Rd=PC` exception-return tests.
//!
//! GBATEK `#arm-opcodes-data-processing-alu` documents that a data-processing
//! instruction with `S=1` and `Rd=R15` restores the CPSR from the current mode's
//! SPSR and takes the computed result as the new PC. Expected values here are
//! hand-derived from that rule: the restored CPSR replaces the ordinary N/Z/C/V
//! result, the restored T bit decides the target alignment and the restored mode
//! decides which bank is visible. Direct-entry USER/SYSTEM has no SPSR and must
//! fail explicitly instead of fabricating one.

use gba_native_core::bus::Bus;
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::{Cpu, FLAG_T, Mode, USER_STACK};
use gba_native_core::decode::decode_arm;
use gba_native_core::error::Error;
use gba_native_core::interpret;
use gba_native_core::ir::{PSR_FIELD_C, PSR_FIELD_F, Psr};

const ADDR: u32 = 0x0800_0100;
/// `MOVS PC,R0`.
const MOVS_PC_R0: u32 = 0xE1B0_F000;
/// `SUBS PC,LR,#4`, the documented IRQ epilogue shape.
const SUBS_PC_LR_4: u32 = 0xE25E_F004;
/// `SUBS PC,PC,#4`: the computed target is the following instruction.
const SUBS_PC_PC_4: u32 = 0xE25F_F004;

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn cpu_in(mode: Mode, addr: u32, thumb: bool) -> Cpu {
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(addr);
    cpu.set_thumb(thumb);
    let control = mode.bits() | if thumb { FLAG_T } else { 0 };
    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, control)
        .expect("enter mode");
    cpu
}

/// Write the current mode's SPSR with flags and control fields.
fn set_spsr(cpu: &mut Cpu, value: u32) {
    cpu.write_psr(Psr::Spsr, PSR_FIELD_F | PSR_FIELD_C, value)
        .expect("SPSR write");
}

/// Write only the SPSR flags byte, leaving its (possibly invalid) mode intact.
fn set_spsr_flags(cpu: &mut Cpu, value: u32) {
    cpu.write_psr(Psr::Spsr, PSR_FIELD_F, value)
        .expect("SPSR flag write");
}

/// Decode and execute one ARM instruction against an unbounded synthetic bus.
fn execute(cpu: &mut Cpu, raw: u32) -> Result<(), Error> {
    let mut bus = Bus::new(cart());
    let instruction = decode_arm(cpu.pc(), raw).expect("ARM decode");
    interpret::execute(cpu, &mut bus, instruction)
}

#[test]
fn movs_pc_restores_cpsr_and_the_saved_register_bank() {
    let mut cpu = cpu_in(Mode::Svc, ADDR, false);
    cpu.set_reg(13, 0x0300_7FE0); // SVC bank
    set_spsr(&mut cpu, 0xF000_001F); // SYSTEM, ARM, N=Z=C=V=1
    cpu.set_reg(0, 0x0800_0206);

    execute(&mut cpu, MOVS_PC_R0).expect("exception return");

    assert_eq!(
        cpu.cpsr(),
        0xF000_001F,
        "the whole CPSR comes from SPSR_svc; the MOV result must not clear N"
    );
    assert_eq!(cpu.mode(), Mode::System, "the restored mode is visible");
    assert!(!cpu.thumb(), "T comes from the restored SPSR");
    assert_eq!(cpu.pc(), 0x0800_0204, "ARM target is word-aligned");
    assert_eq!(
        cpu.reg(13),
        USER_STACK,
        "the USER/SYSTEM bank is visible after the return"
    );
    assert_eq!(cpu.reg(0), 0x0800_0206, "Rd=R15 never writes a GPR");
    assert!(
        cpu.read_psr(Psr::Spsr).is_err(),
        "SYSTEM has no SPSR to read after the return"
    );
}

#[test]
fn exception_return_honors_a_thumb_spsr_written_by_a_real_swi_frame() {
    // An SPSR with T=1 cannot be produced by MSR (that path may not change T),
    // so this uses the documented Thumb SWI frame, which records the interrupted
    // Thumb CPSR in SPSR_svc, and then returns through the ALU form instead of
    // `MOVS PC,LR`.
    let mut rom = vec![0u8; 0x200];
    rom[..2].copy_from_slice(&0xDF08u16.to_le_bytes()); // Thumb SWI 0x08 (Sqrt)
    let mut bus = Bus::new(Cartridge::from_bytes(rom).expect("synthetic cart"));
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(0x0800_0000);
    cpu.set_thumb(true);
    cpu.set_reg(0, 4_294_836_225);
    interpret::step(&mut cpu, &mut bus).expect("Thumb SWI");
    assert!(
        cpu.thumb(),
        "the SWI returns to the interrupted Thumb state"
    );
    let interrupted = cpu.cpsr();

    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, Mode::Svc.bits() | FLAG_T)
        .expect("SVC keeps the current T bit");
    assert_eq!(
        cpu.read_psr(Psr::Spsr).expect("SWI frame SPSR"),
        interrupted,
        "the interrupted Thumb CPSR is the saved status"
    );
    cpu.set_pc(ADDR);
    cpu.set_reg(13, 0x1111_1111); // SVC bank
    cpu.set_reg(14, 0x0800_0207); // LR_svc

    execute(&mut cpu, SUBS_PC_LR_4).expect("Thumb exception return");

    assert_eq!(cpu.pc(), 0x0800_0202, "(LR-4) & !1");
    assert!(cpu.thumb(), "the saved T bit switches to Thumb");
    assert_eq!(cpu.cpsr(), interrupted);
    assert_eq!(cpu.mode(), Mode::System);
    assert_eq!(cpu.reg(13), USER_STACK, "the SVC bank is no longer visible");
}

#[test]
fn subs_pc_pc_4_returns_to_the_computed_address() {
    let mut cpu = cpu_in(Mode::Svc, ADDR, false);
    set_spsr(&mut cpu, 0x6000_001F); // SYSTEM, ARM, Z=C=1

    execute(&mut cpu, SUBS_PC_PC_4).expect("PC-relative exception return");

    assert_eq!(cpu.pc(), ADDR + 4, "PC reads as address+8, minus 4");
    assert_eq!(cpu.cpsr(), 0x6000_001F, "restored CPSR, not SUB flags");
    assert_eq!(cpu.mode(), Mode::System);
}

#[test]
fn missing_spsr_fails_before_any_state_change() {
    for mode in [Mode::System, Mode::User] {
        let mut cpu = cpu_in(mode, ADDR, false);
        cpu.set_reg(0, 0x0800_0206);
        let before = cpu.clone();
        let error = execute(&mut cpu, MOVS_PC_R0).expect_err("no SPSR to restore");
        assert!(matches!(error, Error::Psr(_)), "unexpected error: {error}");
        assert!(
            format!("{error}").contains("SPSR"),
            "the error names the missing SPSR: {error}"
        );
        assert_eq!(cpu, before, "{mode:?} state must be untouched");
    }
}

#[test]
fn invalid_saved_mode_fails_before_any_state_change() {
    for mode in [Mode::Fiq, Mode::Irq, Mode::Svc, Mode::Abt, Mode::Und] {
        // Direct-entry exception SPSRs start at 0, which is not a valid mode.
        let mut cpu = cpu_in(mode, ADDR, false);
        cpu.set_reg(0, 0x0800_0206);
        set_spsr_flags(&mut cpu, 0x4000_0000);
        assert_eq!(
            cpu.read_psr(Psr::Spsr).expect("SPSR exists"),
            0x4000_0000,
            "only the flags were written"
        );
        let before = cpu.clone();
        let error = execute(&mut cpu, MOVS_PC_R0).expect_err("invalid return mode");
        assert!(matches!(error, Error::Psr(_)), "unexpected error: {error}");
        assert!(
            format!("{error}").contains("invalid mode"),
            "the error names the invalid mode: {error}"
        );
        assert_eq!(cpu, before, "{mode:?} state must be untouched");
    }
}

#[test]
fn a_false_condition_never_needs_an_spsr() {
    // MOVSNE PC,R0 with Z=1 never executes, so the missing SPSR is irrelevant.
    let raw = (MOVS_PC_R0 & 0x0FFF_FFFF) | (0x1 << 28);
    let mut cpu = cpu_in(Mode::System, ADDR, false);
    cpu.set_nzcv(false, true, false, false);
    let before = cpu.clone();
    execute(&mut cpu, raw).expect("condition false");
    assert_eq!(cpu.pc(), ADDR + 4, "fall through");
    assert_eq!(cpu.cpsr(), before.cpsr());
    assert_eq!(cpu.reg(0), before.reg(0));
}

#[test]
fn exception_return_is_the_only_s1_pc_form_and_keeps_typed_metadata() {
    for raw in [MOVS_PC_R0, SUBS_PC_LR_4, SUBS_PC_PC_4] {
        let instruction = decode_arm(ADDR, raw).expect("typed data processing");
        assert!(instruction.op.restores_spsr(), "raw={raw:#010X}");
        assert!(instruction.writes_cpsr_control(), "raw={raw:#010X}");
        assert!(instruction.is_control_flow(), "raw={raw:#010X}");
    }
    // S=0 PC writes stay ordinary force-aligned PC writes.
    for raw in [MOVS_PC_R0 & !(1 << 20), SUBS_PC_LR_4 & !(1 << 20)] {
        let instruction = decode_arm(ADDR, raw).expect("S=0 PC write");
        assert!(!instruction.op.restores_spsr(), "raw={raw:#010X}");
        assert!(!instruction.writes_cpsr_control(), "raw={raw:#010X}");
        assert!(instruction.is_control_flow(), "raw={raw:#010X}");
    }
}
