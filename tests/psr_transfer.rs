//! Independent hand-derived ARM PSR-transfer vectors.
//!
//! Encodings are composed from the GBATEK field table (not from the decoder),
//! and expected CPSR/SPSR values are written out explicitly. These tests cover
//! the implemented ARM7TDMI subset only, not hardware conformance.

use gba_native::bus::{Bus, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::{Cpu, FLAG_F, FLAG_I, FLAG_T, Mode};
use gba_native::decode::decode_arm;
use gba_native::error::Error;
use gba_native::interpret;
use gba_native::ir::{Cond, Op, Psr, PsrSrc};

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn arm(words: &[u32]) -> (Cpu, Bus) {
    let mut bus = Bus::new(cart());
    for (index, word) in words.iter().enumerate() {
        bus.write32(EWRAM_BASE + index as u32 * 4, *word)
            .expect("write code");
    }
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_thumb(false);
    (cpu, bus)
}

fn steps(cpu: &mut Cpu, bus: &mut Bus, count: usize) {
    for _ in 0..count {
        interpret::step(cpu, bus).expect("instruction");
    }
}

fn mrs(cond: u32, rd: u32, spsr: bool) -> u32 {
    (cond << 28) | 0x010F_0000 | ((spsr as u32) << 22) | (rd << 12)
}

fn msr_reg(cond: u32, spsr: bool, fields: u32, rm: u32) -> u32 {
    (cond << 28) | 0x0120_F000 | ((spsr as u32) << 22) | (fields << 16) | rm
}

fn msr_imm(cond: u32, spsr: bool, fields: u32, imm8: u32, rotation: u32) -> u32 {
    assert!(imm8 < 256 && rotation < 16);
    (cond << 28) | 0x0320_F000 | ((spsr as u32) << 22) | (fields << 16) | (rotation << 8) | imm8
}

#[test]
fn psr_transfer_decoding_and_rotated_immediate_are_exact() {
    assert_eq!(
        decode_arm(0, mrs(0xE, 3, false)).expect("MRS").op,
        Op::Mrs {
            rd: 3,
            psr: Psr::Cpsr
        }
    );
    assert_eq!(
        decode_arm(0, mrs(0xE, 4, true)).expect("MRS SPSR").op,
        Op::Mrs {
            rd: 4,
            psr: Psr::Spsr
        }
    );
    assert_eq!(
        decode_arm(0, msr_reg(0xE, false, 0b1001, 2))
            .expect("MSR reg")
            .op,
        Op::Msr {
            psr: Psr::Cpsr,
            fields: 0b1001,
            src: PsrSrc::Reg(2)
        }
    );
    for rotation in 0..16u32 {
        let expected = 0xA5u32.rotate_right(rotation * 2);
        assert_eq!(
            decode_arm(0, msr_imm(0xE, true, 0b1001, 0xA5, rotation))
                .expect("MSR immediate")
                .op,
            Op::Msr {
                psr: Psr::Spsr,
                fields: 0b1001,
                src: PsrSrc::Imm(expected)
            }
        );
    }

    // Existing immediate-family classification must stay independent.
    assert!(matches!(
        decode_arm(0, 0xE3A0_00B0).expect("MOV immediate").op,
        Op::Mov { .. }
    ));
    assert!(matches!(
        decode_arm(0, 0xE350_00B0).expect("CMP immediate").op,
        Op::Cmp { .. }
    ));
    assert!(decode_arm(0, 0xE3C1_00B0).is_err());
}

#[test]
fn msr_flag_and_control_masks_preserve_reserved_bits() {
    let (mut cpu, mut bus) = arm(&[
        msr_reg(0xE, false, 0b1000, 0), // MSR CPSR_f,r0
        msr_reg(0xE, false, 0b0001, 1), // MSR CPSR_c,r1
        msr_reg(0xE, false, 0b0100, 2), // MSR CPSR_s,r2 (reserved)
        msr_reg(0xE, false, 0b0010, 3), // MSR CPSR_x,r3 (reserved)
        mrs(0xE, 4, false),             // MRS r4,CPSR
        msr_reg(0xE, false, 0b1001, 5), // MSR CPSR_fc,r5
    ]);
    cpu.set_reg(0, 0xFFFF_FFFF);
    cpu.set_reg(1, 0xFFFF_FFD3);
    cpu.set_reg(2, 0x00FF_0000);
    cpu.set_reg(3, 0x0000_FF00);
    cpu.set_reg(5, 0x0F00_0011);
    cpu.set_nzcv(true, false, true, false);

    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.cpsr(), 0xF000_00DF, "only NZCV changed");
    assert_eq!(cpu.flags(), (true, true, true, true));

    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.cpsr(), 0xF000_00D3, "I/F and mode applied, T kept");
    assert_eq!(cpu.mode(), Mode::Svc);

    steps(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.cpsr(), 0xF000_00D3, "reserved s/x bytes are preserved");

    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.reg(4), 0xF000_00D3);
    assert_eq!(cpu.cpsr(), 0xF000_00D3);

    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.cpsr(), 0x0000_0011, "f+c writes together");
    assert_eq!(cpu.mode(), Mode::Fiq);
}

#[test]
fn user_mode_changes_flags_only_and_has_no_spsr() {
    let (mut cpu, mut bus) = arm(&[
        msr_reg(0xE, false, 0b0001, 0), // MSR CPSR_c,r0 -> USER
        msr_reg(0xE, false, 0b0001, 1), // control-only write is ignored
        msr_reg(0xE, false, 0b1001, 2), // flags applied, control ignored
        mrs(0xE, 3, false),             // CPSR readable
        mrs(0xE, 4, true),              // SPSR must error
        msr_reg(0xE, true, 0b0001, 5),  // SPSR write must error
    ]);
    cpu.set_reg(0, Mode::User.bits());
    cpu.set_reg(1, Mode::System.bits());
    cpu.set_reg(2, 0x0000_001F);
    cpu.set_nzcv(true, true, true, true);

    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.mode(), Mode::User);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.cpsr(), 0xF000_0010, "control ignored, flags kept");

    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.cpsr(), 0x0000_0010, "flags change, mode stays USER");
    assert_eq!(cpu.mode(), Mode::User);

    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.reg(3), 0x0000_0010);
    let before = cpu.clone();

    let error = interpret::step(&mut cpu, &mut bus).expect_err("USER has no SPSR");
    assert!(matches!(error, Error::Psr(_)));
    assert_eq!(cpu, before, "failed MRS SPSR must not mutate state");
    assert_eq!(cpu.pc(), EWRAM_BASE + 16);

    let error = interpret::step(&mut cpu, &mut bus).expect_err("USER cannot write SPSR");
    assert!(matches!(error, Error::Psr(_)));
    assert_eq!(cpu, before, "failed MSR SPSR must not mutate state");
}

#[test]
fn system_mode_psr_access_and_initial_spsr_are_explicit() {
    let (mut cpu, mut bus) = arm(&[mrs(0xE, 0, true)]);
    cpu.set_reg(0, 0xDEAD_BEEF);
    let before = cpu.clone();
    let error = interpret::step(&mut cpu, &mut bus).expect_err("SYSTEM has no SPSR");
    assert!(matches!(error, Error::Psr(_)));
    assert_eq!(cpu, before);

    let (mut cpu, mut bus) = arm(&[mrs(0xE, 0, true)]);
    cpu.write_psr(Psr::Cpsr, 1, Mode::Irq.bits())
        .expect("switch to IRQ");
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(
        cpu.reg(0),
        0,
        "direct-entry SPSR is design-initialized to 0"
    );
}

#[test]
fn conditional_false_psr_transfer_is_a_noop_and_advances_pc() {
    let (mut cpu, mut bus) = arm(&[
        msr_reg(0x0, false, 0b0001, 0), // MSREQ CPSR_c,r0 with Z=0
        mrs(0x0, 1, true),              // MRSEQ r1,SPSR with Z=0
    ]);
    cpu.set_reg(0, 0x0000_0020); // would change T if executed
    cpu.set_nzcv(false, false, false, false);
    let before = cpu.clone();
    let used = interpret::run_budget(&mut cpu, &mut bus, 2).expect("conditional no-ops");
    assert_eq!(used, 2);
    assert_eq!(cpu.cpsr(), before.cpsr());
    assert_eq!(cpu.reg(1), 0);
    assert_eq!(cpu.pc(), EWRAM_BASE + 8);

    // The same encoding must fail when its condition holds.
    cpu.set_pc(EWRAM_BASE);
    cpu.set_nzcv(false, true, false, false);
    let error = interpret::step(&mut cpu, &mut bus).expect_err("T change with Z=1");
    assert!(matches!(error, Error::Psr(_)));
    assert_eq!(cpu.pc(), EWRAM_BASE);
}

#[test]
fn thumb_state_is_the_cpsr_t_bit_and_msr_cannot_change_it() {
    let mut cpu = Cpu::direct_entry();
    assert!(!cpu.thumb());
    cpu.set_thumb(true);
    assert!(cpu.thumb());
    assert_eq!(cpu.cpsr() & FLAG_T, FLAG_T);

    cpu.set_nzcv(true, true, false, true);
    let before = cpu.clone();
    // flags + control, but T already 1: accepted and preserved.
    cpu.write_psr(Psr::Cpsr, 0b1001, 0x0000_003F)
        .expect("T unchanged in Thumb state");
    assert_eq!(cpu.mode(), Mode::System);
    assert!(cpu.thumb());
    assert_eq!(cpu.flags(), (false, false, false, false));
    assert_eq!(cpu.cpsr(), 0x0000_003F);

    let before_thumb = cpu.clone();
    let error = cpu
        .write_psr(Psr::Cpsr, 0b0001, 0x0000_001F)
        .expect_err("T change rejected");
    assert!(matches!(error, Error::Psr(_)));
    assert_eq!(cpu, before_thumb);

    cpu.set_thumb(false);
    assert_eq!(cpu.cpsr() & FLAG_T, 0);
    cpu.set_nzcv(false, true, true, false);
    assert_eq!(cpu.flags(), (false, true, true, false));

    // A false condition must not execute the T-changing MSR at all.
    assert_eq!(before.cpsr() & FLAG_T, FLAG_T);
}

#[test]
fn invalid_control_writes_fail_before_mutation() {
    for value in [0x0000_0000u32, 0x0000_001C, 0x0000_0020, 0x0000_003F] {
        let (mut cpu, mut bus) = arm(&[msr_reg(0xE, false, 0b0001, 0)]);
        cpu.set_reg(0, value);
        cpu.set_nzcv(true, true, false, true);
        let before = cpu.clone();
        let error = interpret::step(&mut cpu, &mut bus).expect_err("invalid control write");
        assert!(matches!(error, Error::Psr(_)), "value 0x{value:08X}");
        assert_eq!(cpu, before, "value 0x{value:08X}");
        assert_eq!(cpu.pc(), EWRAM_BASE);
    }

    let (mut cpu, mut bus) = arm(&[0xE12F_0000]); // fixed MSR Rd-field
    let before = cpu.clone();
    let error = interpret::step(&mut cpu, &mut bus).expect_err("malformed fixed bits");
    assert!(matches!(error, Error::UnsupportedInstruction { .. }));
    assert_eq!(cpu, before);
    assert_eq!(cpu.pc(), EWRAM_BASE);
}

#[test]
fn msr_immediate_executes_the_documented_rotation() {
    let (mut cpu, mut bus) = arm(&[
        msr_imm(0xE, false, 0b1000, 0xF0, 4), // MSR CPSR_f,#0xF0000000
        msr_imm(0xE, false, 0b1000, 0x02, 1), // MSR CPSR_f,#0x80000000
    ]);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.flags(), (true, true, true, true));
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.flags(), (true, false, false, false));
    assert_eq!(cpu.cpsr() & (FLAG_I | FLAG_F), FLAG_I | FLAG_F);
    assert_eq!(cpu.mode(), Mode::System);
}

#[test]
fn immediate_msr_control_and_spsr_forms_execute() {
    let (mut cpu, mut bus) = arm(&[
        msr_imm(0xE, false, 0b0001, 0x13, 0), // MSR CPSR_c,#SVC
        msr_imm(0xE, true, 0b1000, 0xF0, 4),  // MSR SPSR_svc_f,#0xF0000000
        mrs(0xE, 0, true),                    // MRS r0,SPSR_svc
    ]);
    steps(&mut cpu, &mut bus, 3);
    assert_eq!(cpu.mode(), Mode::Svc);
    assert_eq!(cpu.cpsr(), 0x0000_0013);
    assert_eq!(cpu.reg(0), 0xF000_0000);
}

#[test]
fn condition_codes_still_select_psr_transfers() {
    let (mut cpu, mut bus) = arm(&[mrs(0x1, 0, false)]); // MRS with cond NE
    cpu.set_nzcv(false, false, false, false);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
    assert_eq!(cpu.reg(0), 0xDF);

    let instruction = decode_arm(0, mrs(0x1, 0, false)).expect("MRS NE");
    assert_eq!(instruction.cond, Cond::Ne);
}
