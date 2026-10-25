//! Empty MSR field-mask policy.
//!
//! GBATEK documents that at least one of the `f`/`s`/`x`/`c` field bits must be
//! set; an empty mask is unpredictable. This prototype rejects that encoding at
//! decode and in the public `Cpu::write_psr` path instead of reporting a
//! successful no-op. The encodings below are composed from the documented field
//! table, and expected behavior is explicit.

use gba_native_core::bus::{Bus, EWRAM_BASE};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::{Cpu, Mode};
use gba_native_core::decode::decode_arm;
use gba_native_core::error::Error;
use gba_native_core::interpret;
use gba_native_core::ir::{Op, PSR_FIELD_C, PSR_FIELD_F, Psr, PsrSrc};

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn msr_reg(cond: u32, spsr: bool, fields: u32, rm: u32) -> u32 {
    (cond << 28) | 0x0120_F000 | ((spsr as u32) << 22) | (fields << 16) | rm
}

fn msr_imm(cond: u32, spsr: bool, fields: u32, imm8: u32, rotation: u32) -> u32 {
    assert!(imm8 < 256 && rotation < 16);
    (cond << 28) | 0x0320_F000 | ((spsr as u32) << 22) | (fields << 16) | (rotation << 8) | imm8
}

#[test]
fn empty_msr_field_mask_is_rejected_for_both_psrs_and_both_source_forms() {
    for spsr in [false, true] {
        for raw in [
            msr_reg(0xE, spsr, 0, 0),
            msr_reg(0xE, spsr, 0, 14),
            msr_imm(0xE, spsr, 0, 0x1F, 0),
            msr_imm(0xE, spsr, 0, 0xF0, 4),
        ] {
            let error = decode_arm(0, raw).expect_err("empty MSR field mask rejected");
            match error {
                Error::UnsupportedInstruction {
                    raw: rejected,
                    reason,
                    ..
                } => {
                    assert_eq!(rejected, raw);
                    assert!(reason.contains("empty field mask"), "{reason}");
                }
                other => panic!("unexpected error: {other}"),
            }
        }
    }
}

#[test]
fn empty_field_mask_is_not_a_conditional_noop_at_decode_or_execution() {
    // The unsupported *encoding* is rejected before condition evaluation, so a
    // false condition does not turn it into a successful instruction.
    let raw = msr_reg(0x0, false, 0, 0); // MSREQ CPSR_<none>,r0
    assert!(decode_arm(EWRAM_BASE, raw).is_err());

    let mut bus = Bus::new(cart());
    bus.write32(EWRAM_BASE, raw).expect("write instruction");
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_reg(0, 0xFFFF_FFFF);
    cpu.set_nzcv(false, false, false, false); // EQ false
    let before = cpu.clone();
    let bus_before = bus.state_digest();
    let error = interpret::step(&mut cpu, &mut bus).expect_err("decode rejects the encoding");
    assert!(matches!(error, Error::UnsupportedInstruction { .. }));
    assert_eq!(cpu, before, "rejection must not mutate CPU state");
    assert_eq!(cpu.pc(), EWRAM_BASE, "PC must not advance on rejection");
    assert_eq!(bus.state_digest(), bus_before, "no bus mutation");
}

#[test]
fn public_write_psr_rejects_empty_mask_before_mutation() {
    let mut cpu = Cpu::direct_entry();
    cpu.set_nzcv(true, false, true, false);
    let before = cpu.clone();
    let error = cpu
        .write_psr(Psr::Cpsr, 0, 0xFFFF_FFFF)
        .expect_err("empty CPSR mask");
    assert!(matches!(error, Error::Psr(_)));
    assert_eq!(cpu, before);

    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, Mode::Irq.bits())
        .expect("switch to IRQ so SPSR exists");
    let before = cpu.clone();
    let error = cpu
        .write_psr(Psr::Spsr, 0, 0xFFFF_FFFF)
        .expect_err("empty SPSR mask");
    assert!(matches!(error, Error::Psr(_)));
    assert_eq!(cpu, before);

    // A mask with at least one documented bit still works.
    cpu.write_psr(Psr::Spsr, PSR_FIELD_F, 0xF000_0000)
        .expect("non-empty SPSR field mask");
    assert_eq!(cpu.read_psr(Psr::Spsr), Ok(0xF000_0000));
}

#[test]
fn nonempty_msr_field_masks_still_decode_and_execute() {
    for fields in 1..=15u32 {
        assert_eq!(
            decode_arm(0, msr_reg(0xE, false, fields, 0))
                .expect("register MSR")
                .op,
            Op::Msr {
                psr: Psr::Cpsr,
                fields: fields as u8,
                src: PsrSrc::Reg(0)
            }
        );
        assert_eq!(
            decode_arm(0, msr_imm(0xE, true, fields, 0x13, 0))
                .expect("immediate MSR")
                .op,
            Op::Msr {
                psr: Psr::Spsr,
                fields: fields as u8,
                src: PsrSrc::Imm(0x13)
            }
        );
    }

    let mut bus = Bus::new(cart());
    bus.write32(EWRAM_BASE, msr_reg(0xE, false, 0b1000, 0))
        .expect("write MSR");
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_reg(0, 0xF000_0000);
    interpret::step(&mut cpu, &mut bus).expect("flags-only MSR executes");
    assert_eq!(cpu.flags(), (true, true, true, true));
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
}
