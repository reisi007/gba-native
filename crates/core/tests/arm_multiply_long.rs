//! ARM `UMULL`/`UMLAL`/`SMULL`/`SMLAL` vectors written as raw encodings.

use gba_native_core::bus::{Bus, EWRAM_BASE};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::error::Error;
use gba_native_core::interpret;

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

fn multiply_long(
    signed: bool,
    accumulate: bool,
    set_flags: bool,
    rd_hi: u32,
    rd_lo: u32,
    rs: u32,
    rm: u32,
) -> u32 {
    0xE000_0000
        | 0x0080_0000
        | (u32::from(signed) << 22)
        | (u32::from(accumulate) << 21)
        | (u32::from(set_flags) << 20)
        | (rd_hi << 16)
        | (rd_lo << 12)
        | (rs << 8)
        | 0x90
        | rm
}

#[test]
fn umull_writes_the_unsigned_64_bit_product() {
    let (mut cpu, mut bus) = arm(&[multiply_long(false, false, false, 3, 2, 1, 0)]);
    cpu.set_reg(0, 0xFFFF_FFFF);
    cpu.set_reg(1, 2);
    interpret::step(&mut cpu, &mut bus).expect("umull");
    assert_eq!(cpu.reg(2), 0xFFFF_FFFE);
    assert_eq!(cpu.reg(3), 1);
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
}

#[test]
fn umlal_adds_the_unsigned_accumulator() {
    let (mut cpu, mut bus) = arm(&[multiply_long(false, true, false, 3, 2, 1, 0)]);
    cpu.set_reg(0, 0xFFFF_FFFF);
    cpu.set_reg(1, 2);
    cpu.set_reg(2, 5);
    cpu.set_reg(3, 0);
    interpret::step(&mut cpu, &mut bus).expect("umlal");
    assert_eq!(cpu.reg(2), 3);
    assert_eq!(cpu.reg(3), 2);
}

#[test]
fn smull_sign_extends_both_operands() {
    let (mut cpu, mut bus) = arm(&[multiply_long(true, false, false, 3, 2, 1, 0)]);
    cpu.set_reg(0, 0xFFFF_FFFF); // -1
    cpu.set_reg(1, 2);
    interpret::step(&mut cpu, &mut bus).expect("smull");
    assert_eq!(cpu.reg(2), 0xFFFF_FFFE);
    assert_eq!(cpu.reg(3), 0xFFFF_FFFF);
}

#[test]
fn smlal_adds_a_signed_64_bit_accumulator() {
    let (mut cpu, mut bus) = arm(&[multiply_long(true, true, false, 3, 2, 1, 0)]);
    cpu.set_reg(0, 0xFFFF_FFFF); // -1
    cpu.set_reg(1, 2);
    cpu.set_reg(2, 0xFFFF_FFFF); // accumulator low -1
    cpu.set_reg(3, 0xFFFF_FFFF); // accumulator high -1 => -1
    interpret::step(&mut cpu, &mut bus).expect("smlal");
    // -2 + -1 = -3.
    assert_eq!(cpu.reg(2), 0xFFFF_FFFD);
    assert_eq!(cpu.reg(3), 0xFFFF_FFFF);
}

#[test]
fn set_flags_uses_the_full_64_bit_result_and_preserves_carry_and_overflow() {
    let (mut cpu, mut bus) = arm(&[multiply_long(false, false, true, 3, 2, 1, 0)]);
    cpu.set_reg(0, 0xFFFF_FFFF);
    cpu.set_reg(1, 0xFFFF_FFFF);
    cpu.set_nzcv(false, false, true, true);
    interpret::step(&mut cpu, &mut bus).expect("umulls");
    assert_eq!(cpu.reg(2), 0x0000_0001);
    assert_eq!(cpu.reg(3), 0xFFFF_FFFE);
    assert_eq!(cpu.flags(), (true, false, true, true));

    let (mut cpu, mut bus) = arm(&[multiply_long(false, false, true, 3, 2, 1, 0)]);
    cpu.set_reg(0, 0);
    cpu.set_reg(1, 5);
    cpu.set_nzcv(true, false, false, true);
    interpret::step(&mut cpu, &mut bus).expect("umulls zero");
    assert_eq!(cpu.flags(), (false, true, false, true));
}

#[test]
fn rd_hi_equal_to_rd_lo_and_r15_operands_are_rejected() {
    for word in [
        multiply_long(false, false, false, 2, 2, 1, 0),
        multiply_long(false, false, false, 3, 2, 1, 15),
    ] {
        let (mut cpu, mut bus) = arm(&[word]);
        let error = interpret::step(&mut cpu, &mut bus).expect_err("invalid encoding");
        assert!(matches!(
            error,
            Error::UnsupportedInstruction { thumb: false, .. }
        ));
        assert_eq!(cpu.pc(), EWRAM_BASE);
    }
}
