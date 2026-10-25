//! Independent ARM single data-transfer vectors (word/byte, immediate and
//! shifted-register offsets, pre/post indexing, writeback, PC base/source).
//!
//! Encodings are composed from the documented field layout, and expected
//! addresses/values are hand-derived. No shared decoder or interpreter helper
//! is used to compute an oracle.

use gba_native_core::bus::{Bus, EWRAM_BASE, OAM_BASE, PALETTE_BASE, VRAM_BASE};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::{Cpu, FLAG_C, Mode};
use gba_native_core::decode::decode_arm;
use gba_native_core::error::Error;
use gba_native_core::interpret;
use gba_native_core::ir::{MemOffset, MemWidth, Op, Shift, ShiftKind};

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn arm(word: u32) -> (Cpu, Bus) {
    let mut bus = Bus::new(cart());
    bus.write32(EWRAM_BASE, word).expect("write instruction");
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_thumb(false);
    (cpu, bus)
}

/// Immediate-offset single transfer.
#[allow(clippy::too_many_arguments)]
fn imm(cond: u32, p: u32, u: u32, b: u32, bit21: u32, l: u32, rn: u32, rd: u32, off: u32) -> u32 {
    (cond << 28)
        | 0x0400_0000
        | (p << 24)
        | (u << 23)
        | (b << 22)
        | (bit21 << 21)
        | (l << 20)
        | (rn << 16)
        | (rd << 12)
        | (off & 0xFFF)
}

/// Register-shifted immediate-offset single transfer.
#[allow(clippy::too_many_arguments)]
fn reg(
    cond: u32,
    p: u32,
    u: u32,
    b: u32,
    bit21: u32,
    l: u32,
    rn: u32,
    rd: u32,
    kind: ShiftKind,
    amount: u32,
    rm: u32,
) -> u32 {
    let kind_bits = match kind {
        ShiftKind::Lsl => 0,
        ShiftKind::Lsr => 1,
        ShiftKind::Asr => 2,
        ShiftKind::Ror => 3,
    };
    (cond << 28)
        | 0x0600_0000
        | (p << 24)
        | (u << 23)
        | (b << 22)
        | (bit21 << 21)
        | (l << 20)
        | (rn << 16)
        | (rd << 12)
        | ((amount & 0x1F) << 7)
        | (kind_bits << 5)
        | (rm & 0xF)
}

#[test]
fn word_store_aligns_down_and_word_load_rotates_unaligned() {
    let (mut cpu, mut bus) = arm(imm(0xE, 1, 1, 0, 0, 0, 1, 2, 0));
    cpu.set_reg(1, EWRAM_BASE + 0x42);
    cpu.set_reg(2, 0x1122_3344);
    let flags = cpu.flags();
    interpret::step(&mut cpu, &mut bus).expect("unaligned word store");
    assert_eq!(bus.read32(EWRAM_BASE + 0x40), Ok(0x1122_3344));
    assert_eq!(cpu.reg(2), 0x1122_3344);
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
    assert_eq!(cpu.flags(), flags, "memory transfers never change flags");

    let (mut cpu, mut bus) = arm(imm(0xE, 1, 1, 0, 0, 1, 1, 2, 0));
    bus.write32(EWRAM_BASE + 0x40, 0x1122_3344).expect("seed");
    cpu.set_reg(1, EWRAM_BASE + 0x41);
    interpret::step(&mut cpu, &mut bus).expect("unaligned word load");
    assert_eq!(cpu.reg(2), 0x4411_2233, "ROR by (address & 3) * 8");
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
}

#[test]
fn byte_transfers_use_byte_width_and_documented_region_rules() {
    let (mut cpu, mut bus) = arm(imm(0xE, 1, 1, 1, 0, 0, 1, 2, 0));
    cpu.set_reg(1, EWRAM_BASE + 1);
    cpu.set_reg(2, 0xFFFF_FFAB);
    interpret::step(&mut cpu, &mut bus).expect("byte store");
    assert_eq!(bus.read8(EWRAM_BASE + 1), Ok(0xAB));
    assert_eq!(bus.read16(EWRAM_BASE), Ok(0xAB00));

    // Loads read a byte, not a rotated word.
    let (mut cpu, mut bus) = arm(imm(0xE, 1, 1, 1, 0, 1, 1, 2, 0));
    bus.write16(EWRAM_BASE + 0x40, 0x1234).expect("seed");
    cpu.set_reg(1, EWRAM_BASE + 0x41);
    interpret::step(&mut cpu, &mut bus).expect("byte load");
    assert_eq!(cpu.reg(2), 0x12);

    // Palette/VRAM byte stores duplicate into both halves; OAM ignores bytes.
    for (base, value, expected) in [
        (PALETTE_BASE + 1, 0xCDu32, 0xCDCDu16),
        (VRAM_BASE + 3, 0xEFu32, 0xEFEFu16),
    ] {
        let (mut cpu, mut bus) = arm(imm(0xE, 1, 1, 1, 0, 0, 1, 2, 0));
        cpu.set_reg(1, base);
        cpu.set_reg(2, value);
        interpret::step(&mut cpu, &mut bus).expect("video byte store");
        assert_eq!(bus.read16(base & !1), Ok(expected));
    }
    let (mut cpu, mut bus) = arm(imm(0xE, 1, 1, 1, 0, 0, 1, 2, 0));
    cpu.set_reg(1, OAM_BASE + 1);
    cpu.set_reg(2, 0xFF);
    interpret::step(&mut cpu, &mut bus).expect("OAM byte store is ignored");
    assert_eq!(bus.read16(OAM_BASE), Ok(0));
}

#[test]
fn shifted_register_offsets_use_immediate_shifts_and_old_carry_for_rrx() {
    // LSL #2: offset 4 -> 16.
    let (mut cpu, mut bus) = arm(reg(0xE, 1, 1, 0, 0, 1, 1, 0, ShiftKind::Lsl, 2, 2));
    bus.write32(EWRAM_BASE + 0x50, 0xCAFE_BABE).expect("seed");
    cpu.set_reg(1, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 4);
    interpret::step(&mut cpu, &mut bus).expect("shifted load");
    assert_eq!(cpu.reg(0), 0xCAFE_BABE);

    // LSR/ASR/ROR of 0x10 by 1 all produce offset 8.
    for kind in [ShiftKind::Lsr, ShiftKind::Asr, ShiftKind::Ror] {
        let (mut cpu, mut bus) = arm(reg(0xE, 1, 1, 0, 0, 0, 1, 2, kind, 1, 3));
        cpu.set_reg(1, EWRAM_BASE + 0x40);
        cpu.set_reg(3, 0x10);
        cpu.set_reg(2, 0x5566_7788);
        interpret::step(&mut cpu, &mut bus).expect("shifted store");
        assert_eq!(bus.read32(EWRAM_BASE + 0x48), Ok(0x5566_7788), "{kind:?}");
    }

    // RRX with old C=1 and Rm=0x11 gives 0x80000008. Base 0x820000F8 + that
    // offset wraps to EWRAM_BASE+0x100; the carry is consumed, not updated.
    let (mut cpu, mut bus) = arm(reg(0xE, 1, 1, 0, 0, 1, 1, 0, ShiftKind::Ror, 0, 2));
    bus.write32(EWRAM_BASE + 0x100, 0x00C0_FFEE).expect("seed");
    cpu.set_reg(1, 0x8200_00F8);
    cpu.set_reg(2, 0x11);
    cpu.set_nzcv(false, false, true, false);
    interpret::step(&mut cpu, &mut bus).expect("RRX load");
    assert_eq!(cpu.reg(0), 0x00C0_FFEE);
    assert!(
        cpu.cpsr() & FLAG_C != 0,
        "a memory offset shift must not update C"
    );
}

#[test]
fn pre_and_post_writeback_and_error_atomicity() {
    // Post-index store uses the old source value, then updates the base.
    let (mut cpu, mut bus) = arm(imm(0xE, 0, 1, 0, 0, 0, 1, 1, 4));
    cpu.set_reg(1, EWRAM_BASE + 0x40);
    interpret::step(&mut cpu, &mut bus).expect("post store");
    assert_eq!(bus.read32(EWRAM_BASE + 0x40), Ok(EWRAM_BASE + 0x40));
    assert_eq!(cpu.reg(1), EWRAM_BASE + 0x44);

    // Negative post-index word store: access the old base, then subtract.
    let (mut cpu, mut bus) = arm(imm(0xE, 0, 0, 0, 0, 0, 1, 1, 4));
    cpu.set_reg(1, EWRAM_BASE + 0x44);
    interpret::step(&mut cpu, &mut bus).expect("negative post store");
    assert_eq!(bus.read32(EWRAM_BASE + 0x44), Ok(EWRAM_BASE + 0x44));
    assert_eq!(cpu.reg(1), EWRAM_BASE + 0x40);

    // Negative pre-index word load without writeback.
    let (mut cpu, mut bus) = arm(imm(0xE, 1, 0, 0, 0, 1, 1, 0, 4));
    bus.write32(EWRAM_BASE + 0x40, 0x0BAD_CAFE).expect("seed");
    cpu.set_reg(1, EWRAM_BASE + 0x44);
    interpret::step(&mut cpu, &mut bus).expect("negative pre load");
    assert_eq!(cpu.reg(0), 0x0BAD_CAFE);
    assert_eq!(cpu.reg(1), EWRAM_BASE + 0x44);

    // Negative byte offset uses the exact byte address.
    let (mut cpu, mut bus) = arm(imm(0xE, 1, 0, 1, 0, 1, 1, 0, 1));
    bus.write8(EWRAM_BASE + 0x40, 0x5A).expect("seed");
    cpu.set_reg(1, EWRAM_BASE + 0x41);
    interpret::step(&mut cpu, &mut bus).expect("negative byte load");
    assert_eq!(cpu.reg(0), 0x5A);

    // Pre-index store with writeback stores the old source at base+8.
    let (mut cpu, mut bus) = arm(imm(0xE, 1, 1, 0, 1, 0, 1, 1, 8));
    cpu.set_reg(1, EWRAM_BASE + 0x40);
    interpret::step(&mut cpu, &mut bus).expect("pre writeback store");
    assert_eq!(bus.read32(EWRAM_BASE + 0x48), Ok(EWRAM_BASE + 0x40));
    assert_eq!(cpu.reg(1), EWRAM_BASE + 0x48);

    // Post-index load updates the base only after the read succeeded.
    let (mut cpu, mut bus) = arm(imm(0xE, 0, 1, 0, 0, 1, 1, 0, 4));
    bus.write32(EWRAM_BASE + 0x40, 0x1234_5678).expect("seed");
    cpu.set_reg(1, EWRAM_BASE + 0x40);
    interpret::step(&mut cpu, &mut bus).expect("post load");
    assert_eq!(cpu.reg(0), 0x1234_5678);
    assert_eq!(cpu.reg(1), EWRAM_BASE + 0x44);

    // A failed store does not fabricate a writeback.
    let (mut cpu, mut bus) = arm(imm(0xE, 0, 1, 0, 0, 0, 1, 2, 4));
    cpu.set_reg(1, 0x0800_0000);
    cpu.set_reg(2, 0xFFFF_FFFF);
    let before = cpu.clone();
    let error = interpret::step(&mut cpu, &mut bus).expect_err("ROM write");
    assert!(matches!(error, Error::RomWrite { .. }));
    assert_eq!(cpu.reg(1), before.reg(1));
    assert_eq!(cpu.pc(), EWRAM_BASE);

    // A failed load leaves both destination and base untouched.
    let (mut cpu, mut bus) = arm(imm(0xE, 0, 1, 0, 0, 1, 1, 0, 4));
    cpu.set_reg(1, 0x0F00_0000);
    cpu.set_reg(0, 0xDEAD_BEEF);
    let error = interpret::step(&mut cpu, &mut bus).expect_err("unmapped load");
    assert!(matches!(error, Error::Unmapped { .. }));
    assert_eq!(cpu.reg(0), 0xDEAD_BEEF);
    assert_eq!(cpu.reg(1), 0x0F00_0000);
    assert_eq!(cpu.pc(), EWRAM_BASE);
}

#[test]
fn unpredictable_alias_and_pc_writeback_forms_are_rejected_before_side_effects() {
    for raw in [
        imm(0xE, 0, 1, 0, 0, 1, 1, 1, 4),  // LDR r1,[r1],#4
        imm(0xE, 1, 1, 0, 1, 1, 1, 1, 4),  // LDR r1,[r1,#4]!
        imm(0xE, 1, 1, 0, 1, 1, 15, 0, 4), // LDR r0,[pc,#4]! (PC writeback)
        imm(0xE, 0, 1, 0, 0, 1, 15, 0, 4), // LDR r0,[pc],#4 (PC writeback)
        imm(0xE, 1, 1, 1, 0, 1, 1, 15, 0), // LDRB pc,[r1]
    ] {
        let (mut cpu, mut bus) = arm(raw);
        cpu.set_reg(1, EWRAM_BASE + 0x40);
        cpu.set_reg(0, 0xAAAA_AAAA);
        let before = cpu.clone();
        let digest = bus.state_digest();
        let error = interpret::step(&mut cpu, &mut bus).expect_err("unpredictable form");
        assert!(
            matches!(error, Error::UnsupportedInstruction { .. }),
            "{raw:08X}: {error}"
        );
        assert_eq!(cpu.pc(), before.pc());
        assert_eq!(cpu.reg(0), before.reg(0));
        assert_eq!(bus.state_digest(), digest);
    }

    // A word LDR PC is implemented, force-aligned, and keeps ARM state.
    let (mut cpu, mut bus) = arm(imm(0xE, 1, 1, 0, 0, 1, 1, 15, 0));
    bus.write32(EWRAM_BASE + 0x40, 0x0800_1235).expect("seed");
    cpu.set_reg(1, EWRAM_BASE + 0x40);
    interpret::step(&mut cpu, &mut bus).expect("word LDR PC");
    assert_eq!(cpu.pc(), 0x0800_1234);
    assert!(
        !cpu.thumb(),
        "ARMv4 LDR PC does not switch instruction state"
    );
}

#[test]
fn pc_base_pc_source_and_conditional_pc_load_are_computed_from_the_instruction() {
    // STRB r0,[pc,#4] uses base = instruction address + 8.
    let (mut cpu, mut bus) = arm(imm(0xE, 1, 1, 1, 0, 0, 15, 0, 4));
    cpu.set_reg(0, 0xAB);
    interpret::step(&mut cpu, &mut bus).expect("PC-based byte store");
    assert_eq!(bus.read8(EWRAM_BASE + 12), Ok(0xAB));

    // STR pc,[r1] stores instruction address + 12.
    let (mut cpu, mut bus) = arm(imm(0xE, 1, 1, 0, 0, 0, 1, 15, 0));
    cpu.set_reg(1, EWRAM_BASE + 0x40);
    interpret::step(&mut cpu, &mut bus).expect("PC source store");
    assert_eq!(bus.read32(EWRAM_BASE + 0x40), Ok(EWRAM_BASE + 12));

    // A false conditional LDR PC must not branch or access memory.
    let (mut cpu, mut bus) = arm(imm(0x0, 1, 1, 0, 0, 1, 1, 15, 0));
    cpu.set_reg(1, EWRAM_BASE + 0x40);
    cpu.set_nzcv(false, false, false, false); // EQ is false
    let digest = bus.state_digest();
    interpret::step(&mut cpu, &mut bus).expect("false LDR PC");
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
    assert_eq!(bus.state_digest(), digest);
}

#[test]
fn post_index_t_bit_does_not_change_gba_cpu_mode() {
    let (mut cpu, mut bus) = arm(imm(0xE, 0, 1, 0, 1, 1, 1, 0, 4));
    bus.write32(EWRAM_BASE + 0x40, 0x0BAD_F00D).expect("seed");
    cpu.set_reg(1, EWRAM_BASE + 0x40);
    assert_eq!(cpu.mode(), Mode::System);
    interpret::step(&mut cpu, &mut bus).expect("T post-index load");
    assert_eq!(cpu.reg(0), 0x0BAD_F00D);
    assert_eq!(cpu.reg(1), EWRAM_BASE + 0x44);
    assert_eq!(cpu.mode(), Mode::System, "flat bus has no privilege split");
}

#[test]
fn decoded_ir_is_typed_and_reserved_shift_fields_are_rejected() {
    let instruction = decode_arm(0, imm(0xE, 1, 1, 0, 1, 1, 2, 3, 0x20)).expect("LDR");
    assert_eq!(
        instruction.op,
        Op::MemSingle {
            load: true,
            width: MemWidth::Word,
            signed: false,
            rd: 3,
            rn: 2,
            offset: MemOffset::Imm(0x20),
            add: true,
            pre_index: true,
            writeback: true,
            user_access: false,
        }
    );
    let instruction =
        decode_arm(0, reg(0xE, 1, 1, 0, 0, 0, 2, 3, ShiftKind::Ror, 0, 4)).expect("STR RRX");
    assert_eq!(
        instruction.op,
        Op::MemSingle {
            load: false,
            width: MemWidth::Word,
            signed: false,
            rd: 3,
            rn: 2,
            offset: MemOffset::RegShift {
                rm: 4,
                shift: Shift::Imm {
                    kind: ShiftKind::Ror,
                    amount: 0
                }
            },
            add: true,
            pre_index: true,
            writeback: false,
            user_access: false,
        }
    );

    // Reserved bit 4 and Rm=15 are rejected, as is the reserved condition 1111.
    assert!(
        decode_arm(
            0,
            reg(0xE, 1, 1, 0, 0, 1, 2, 3, ShiftKind::Lsl, 1, 4) | 0x10
        )
        .is_err()
    );
    assert!(decode_arm(0, reg(0xE, 1, 1, 0, 0, 1, 2, 3, ShiftKind::Lsl, 1, 15)).is_err());
    assert!(
        decode_arm(
            0,
            (imm(0xE, 1, 1, 0, 0, 1, 2, 3, 0) & 0x0FFF_FFFF) | 0xF000_0000
        )
        .is_err()
    );
}
