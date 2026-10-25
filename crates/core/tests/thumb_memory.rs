//! Independent Thumb load/store vectors: register-offset word/byte and
//! half/signed forms, immediate word/byte/half, SP-relative word transfers and
//! the preserved literal-PC path.

use gba_native_core::bus::{Bus, EWRAM_BASE};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::decode::decode_thumb;
use gba_native_core::interpret;
use gba_native_core::ir::{MemWidth, Op};

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn thumb(halfword: u16) -> (Cpu, Bus) {
    let mut bus = Bus::new(cart());
    bus.write16(EWRAM_BASE, halfword)
        .expect("write instruction");
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_thumb(true);
    (cpu, bus)
}

fn reg_offset(opcode: u16, signed_half_form: bool, ro: u16, rb: u16, rd: u16) -> u16 {
    0x5000 | (opcode << 10) | (u16::from(signed_half_form) << 9) | (ro << 6) | (rb << 3) | rd
}

fn immediate(opcode: u16, imm5: u16, rb: u16, rd: u16) -> u16 {
    0x6000 | (opcode << 11) | ((imm5 & 0x1F) << 6) | (rb << 3) | rd
}

fn sp_relative(load: bool, rd: u16, imm8: u16) -> u16 {
    0x9000 | (u16::from(load) << 11) | (rd << 8) | (imm8 & 0xFF)
}

fn half_immediate(load: bool, imm5: u16, rb: u16, rd: u16) -> u16 {
    0x8000 | (u16::from(load) << 11) | ((imm5 & 0x1F) << 6) | (rb << 3) | rd
}

fn literal(rd: u16, imm8: u16) -> u16 {
    0x4800 | (rd << 8) | (imm8 & 0xFF)
}

#[test]
fn format7_register_offsets_use_unshifted_word_and_byte_widths() {
    // STR word at Rb+Ro.
    let (mut cpu, mut bus) = thumb(reg_offset(0, false, 2, 0, 1));
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 4);
    cpu.set_reg(1, 0x1122_3344);
    interpret::step(&mut cpu, &mut bus).expect("STR register offset");
    assert_eq!(bus.read32(EWRAM_BASE + 0x44), Ok(0x1122_3344));

    // STRB then LDRB: no word extraction or rotation.
    let (mut cpu, mut bus) = thumb(reg_offset(1, false, 2, 0, 1));
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 1);
    cpu.set_reg(1, 0xFFFF_FFAB);
    interpret::step(&mut cpu, &mut bus).expect("STRB register offset");
    assert_eq!(bus.read8(EWRAM_BASE + 0x41), Ok(0xAB));
    assert_eq!(bus.read16(EWRAM_BASE + 0x40), Ok(0xAB00));

    let (mut cpu, mut bus) = thumb(reg_offset(3, false, 2, 0, 1));
    bus.write16(EWRAM_BASE + 0x40, 0x1234).expect("seed");
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 1);
    interpret::step(&mut cpu, &mut bus).expect("LDRB register offset");
    assert_eq!(cpu.reg(1), 0x12);

    // LDR word.
    let (mut cpu, mut bus) = thumb(reg_offset(2, false, 2, 0, 1));
    bus.write32(EWRAM_BASE + 0x44, 0xCAFE_BABE).expect("seed");
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 4);
    interpret::step(&mut cpu, &mut bus).expect("LDR register offset");
    assert_eq!(cpu.reg(1), 0xCAFE_BABE);
    assert_eq!(cpu.pc(), EWRAM_BASE + 2);
}

#[test]
fn format8_register_offsets_cover_half_and_signed_forms() {
    // STRH.
    let (mut cpu, mut bus) = thumb(reg_offset(0, true, 2, 0, 1));
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 2);
    cpu.set_reg(1, 0xABCD_4321);
    interpret::step(&mut cpu, &mut bus).expect("STRH register offset");
    assert_eq!(bus.read16(EWRAM_BASE + 0x42), Ok(0x4321));

    // LDRH.
    let (mut cpu, mut bus) = thumb(reg_offset(2, true, 2, 0, 1));
    bus.write16(EWRAM_BASE + 0x42, 0xBEEF).expect("seed");
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 2);
    interpret::step(&mut cpu, &mut bus).expect("LDRH register offset");
    assert_eq!(cpu.reg(1), 0xBEEF);

    // LDSB.
    let (mut cpu, mut bus) = thumb(reg_offset(1, true, 2, 0, 1));
    bus.write8(EWRAM_BASE + 0x41, 0x80).expect("seed");
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 1);
    interpret::step(&mut cpu, &mut bus).expect("LDSB register offset");
    assert_eq!(cpu.reg(1), 0xFFFF_FF80);

    // LDSH at an even address sign-extends the halfword.
    let (mut cpu, mut bus) = thumb(reg_offset(3, true, 2, 0, 1));
    bus.write16(EWRAM_BASE + 0x40, 0x8001).expect("seed");
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 0);
    interpret::step(&mut cpu, &mut bus).expect("LDSH even");
    assert_eq!(cpu.reg(1), 0xFFFF_8001);

    // LDSH at an odd address reads the signed byte at that address.
    let (mut cpu, mut bus) = thumb(reg_offset(3, true, 2, 0, 1));
    bus.write8(EWRAM_BASE + 0x40, 0x34).expect("low");
    bus.write8(EWRAM_BASE + 0x41, 0x80).expect("high");
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 1);
    interpret::step(&mut cpu, &mut bus).expect("LDSH odd");
    assert_eq!(cpu.reg(1), 0xFFFF_FF80);
}

#[test]
fn format9_immediate_word_offsets_scale_by_four_and_byte_offsets_do_not() {
    let (mut cpu, mut bus) = thumb(immediate(0, 2, 0, 1));
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    cpu.set_reg(1, 0xDEAD_BEEF);
    interpret::step(&mut cpu, &mut bus).expect("STR word immediate");
    assert_eq!(bus.read32(EWRAM_BASE + 0x48), Ok(0xDEAD_BEEF));

    let (mut cpu, mut bus) = thumb(immediate(1, 2, 0, 1));
    bus.write32(EWRAM_BASE + 0x48, 0x1234_5678).expect("seed");
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    interpret::step(&mut cpu, &mut bus).expect("LDR word immediate");
    assert_eq!(cpu.reg(1), 0x1234_5678);

    let (mut cpu, mut bus) = thumb(immediate(2, 3, 0, 1));
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    cpu.set_reg(1, 0x1AB);
    interpret::step(&mut cpu, &mut bus).expect("STRB immediate");
    assert_eq!(bus.read8(EWRAM_BASE + 0x43), Ok(0xAB));

    let (mut cpu, mut bus) = thumb(immediate(3, 3, 0, 1));
    bus.write8(EWRAM_BASE + 0x43, 0x7F).expect("seed");
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    interpret::step(&mut cpu, &mut bus).expect("LDRB immediate");
    assert_eq!(cpu.reg(1), 0x7F);
}

#[test]
fn format10_immediate_halfword_shape_and_encoding_are_preserved() {
    assert_eq!(
        decode_thumb(0, u32::from(half_immediate(true, 2, 0, 3)))
            .expect("LDRH")
            .op,
        Op::LdrHalf {
            rd: 3,
            rn: 0,
            offset: 4
        }
    );
    assert_eq!(
        decode_thumb(0, u32::from(half_immediate(false, 0, 0, 3)))
            .expect("STRH")
            .op,
        Op::StrHalf {
            rd: 3,
            rn: 0,
            offset: 0
        }
    );
}

#[test]
fn format11_sp_relative_word_transfers_use_sp_without_writeback() {
    let (mut cpu, mut bus) = thumb(sp_relative(false, 1, 2));
    cpu.set_reg(13, EWRAM_BASE + 0x100);
    cpu.set_reg(1, 0xA5A5_5A5A);
    interpret::step(&mut cpu, &mut bus).expect("STR SP-relative");
    assert_eq!(bus.read32(EWRAM_BASE + 0x108), Ok(0xA5A5_5A5A));
    assert_eq!(cpu.reg(13), EWRAM_BASE + 0x100);

    let (mut cpu, mut bus) = thumb(sp_relative(true, 2, 2));
    bus.write32(EWRAM_BASE + 0x108, 0x1122_3344).expect("seed");
    cpu.set_reg(13, EWRAM_BASE + 0x100);
    interpret::step(&mut cpu, &mut bus).expect("LDR SP-relative");
    assert_eq!(cpu.reg(2), 0x1122_3344);
    assert_eq!(cpu.reg(13), EWRAM_BASE + 0x100);
}

#[test]
fn literal_pc_load_is_still_the_documented_aligned_pc_plus_four_form() {
    let instruction = decode_thumb(0x0800_0000, u32::from(literal(1, 2))).expect("LDR literal");
    assert_eq!(
        instruction.op,
        Op::LdrLiteral {
            rd: 1,
            addr: 0x0800_000C
        }
    );
}

#[test]
fn typed_ir_marks_width_and_never_changes_flags() {
    let instruction =
        decode_thumb(0, u32::from(reg_offset(2, false, 2, 0, 1))).expect("LDR register");
    assert!(matches!(
        instruction.op,
        Op::MemSingle {
            load: true,
            width: MemWidth::Word,
            signed: false,
            rd: 1,
            rn: 0,
            ..
        }
    ));
    let instruction =
        decode_thumb(0, u32::from(reg_offset(3, true, 2, 0, 1))).expect("LDSH register");
    assert!(matches!(
        instruction.op,
        Op::MemSingle {
            load: true,
            width: MemWidth::Half,
            signed: true,
            ..
        }
    ));

    let (mut cpu, mut bus) = thumb(immediate(1, 0, 0, 1));
    bus.write32(EWRAM_BASE + 0x40, 0xCAFE_BABE).expect("seed");
    cpu.set_reg(0, EWRAM_BASE + 0x40);
    cpu.set_nzcv(true, true, true, true);
    interpret::step(&mut cpu, &mut bus).expect("load");
    assert_eq!(cpu.reg(1), 0xCAFE_BABE);
    assert_eq!(cpu.flags(), (true, true, true, true));
}
