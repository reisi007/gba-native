//! Independent ARM halfword/signed-transfer vectors: pre/post indexing,
//! immediate/register offsets, PC base/source, sign extension and the ARM7
//! odd-address rules. Expected values are hand-derived; the shared decoder and
//! interpreter are not used as oracles.

use gba_native_core::bus::{Bus, EWRAM_BASE};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::decode::decode_arm;
use gba_native_core::error::Error;
use gba_native_core::interpret;
use gba_native_core::ir::{MemOffset, MemWidth, Op};

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

/// Halfword/signed transfer. `imm=1` encodes a byte magnitude; `imm=0` encodes
/// `rm` in the low nibble with bits 11-8 zero.
#[allow(clippy::too_many_arguments)]
fn half(
    cond: u32,
    p: u32,
    u: u32,
    imm: u32,
    bit21: u32,
    l: u32,
    rn: u32,
    rd: u32,
    s: u32,
    h: u32,
    magnitude: u32,
) -> u32 {
    (cond << 28)
        | (p << 24)
        | (u << 23)
        | (imm << 22)
        | (bit21 << 21)
        | (l << 20)
        | (rn << 16)
        | (rd << 12)
        | (((magnitude >> 4) & 0xF) << 8)
        | (1 << 7)
        | (s << 6)
        | (h << 5)
        | (1 << 4)
        | (magnitude & 0xF)
}

#[test]
fn previously_supported_pre_indexed_immediate_shape_is_preserved() {
    assert_eq!(
        decode_arm(0, half(0xE, 1, 1, 1, 0, 1, 3, 2, 0, 1, 0x20))
            .expect("LDRH")
            .op,
        Op::LdrHalf {
            rd: 2,
            rn: 3,
            offset: 0x20
        }
    );
    assert_eq!(
        decode_arm(0, half(0xE, 1, 0, 1, 0, 0, 3, 2, 0, 1, 0x20))
            .expect("STRH")
            .op,
        Op::StrHalf {
            rd: 2,
            rn: 3,
            offset: -0x20
        }
    );
}

#[test]
fn pre_and_post_immediate_and_register_offsets_transfer_expected_widths() {
    // Post-index immediate load: access base, then base += 4.
    let (mut cpu, mut bus) = arm(half(0xE, 0, 1, 1, 0, 1, 3, 2, 0, 1, 4));
    bus.write16(EWRAM_BASE + 0x40, 0xBEEF).expect("seed");
    cpu.set_reg(3, EWRAM_BASE + 0x40);
    interpret::step(&mut cpu, &mut bus).expect("post LDRH");
    assert_eq!(cpu.reg(2), 0xBEEF);
    assert_eq!(cpu.reg(3), EWRAM_BASE + 0x44);

    // Pre-index register offset (unshifted).
    let (mut cpu, mut bus) = arm(half(0xE, 1, 1, 0, 0, 1, 3, 2, 0, 1, 4));
    bus.write16(EWRAM_BASE + 0x48, 0x1234).expect("seed");
    cpu.set_reg(3, EWRAM_BASE + 0x40);
    cpu.set_reg(4, 8);
    interpret::step(&mut cpu, &mut bus).expect("register LDRH");
    assert_eq!(cpu.reg(2), 0x1234);

    // Pre-index negative register offset: base - Rm.
    let (mut cpu, mut bus) = arm(half(0xE, 1, 0, 0, 0, 1, 3, 2, 0, 1, 4));
    bus.write16(EWRAM_BASE + 0x40, 0x2468).expect("seed");
    cpu.set_reg(3, EWRAM_BASE + 0x48);
    cpu.set_reg(4, 8);
    interpret::step(&mut cpu, &mut bus).expect("negative register LDRH");
    assert_eq!(cpu.reg(2), 0x2468);

    // Post-index negative store: access base, then base -= 2.
    let (mut cpu, mut bus) = arm(half(0xE, 0, 0, 1, 0, 0, 3, 2, 0, 1, 2));
    cpu.set_reg(3, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 0xABCD_1234);
    interpret::step(&mut cpu, &mut bus).expect("post STRH");
    assert_eq!(bus.read16(EWRAM_BASE + 0x40), Ok(0x1234));
    assert_eq!(cpu.reg(3), EWRAM_BASE + 0x3E);

    // Pre-index immediate store with writeback.
    let (mut cpu, mut bus) = arm(half(0xE, 1, 1, 1, 1, 0, 3, 2, 0, 1, 2));
    cpu.set_reg(3, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 0x9999);
    interpret::step(&mut cpu, &mut bus).expect("pre STRH writeback");
    assert_eq!(bus.read16(EWRAM_BASE + 0x42), Ok(0x9999));
    assert_eq!(cpu.reg(3), EWRAM_BASE + 0x42);
}

#[test]
fn signed_loads_sign_extend_and_ldrsh_odd_reads_the_signed_byte_at_the_address() {
    // Even LDRSH: 0x8001 -> 0xFFFF8001.
    let (mut cpu, mut bus) = arm(half(0xE, 1, 1, 1, 0, 1, 3, 2, 1, 1, 0));
    bus.write16(EWRAM_BASE + 0x40, 0x8001).expect("seed");
    cpu.set_reg(3, EWRAM_BASE + 0x40);
    interpret::step(&mut cpu, &mut bus).expect("even LDRSH");
    assert_eq!(cpu.reg(2), 0xFFFF_8001);

    // Odd LDRSH is LDRSB at the odd address, not a rotated halfword.
    let (mut cpu, mut bus) = arm(half(0xE, 1, 1, 1, 0, 1, 3, 2, 1, 1, 0));
    bus.write8(EWRAM_BASE + 0x40, 0x34).expect("low byte");
    bus.write8(EWRAM_BASE + 0x41, 0x80).expect("high byte");
    cpu.set_reg(3, EWRAM_BASE + 0x41);
    interpret::step(&mut cpu, &mut bus).expect("odd LDRSH");
    assert_eq!(cpu.reg(2), 0xFFFF_FF80);

    // Odd LDRH still rotates the 32-bit zero-extended halfword right by 8.
    let (mut cpu, mut bus) = arm(half(0xE, 1, 1, 1, 0, 1, 3, 2, 0, 1, 0));
    bus.write16(EWRAM_BASE + 0x40, 0x8034).expect("seed");
    cpu.set_reg(3, EWRAM_BASE + 0x41);
    interpret::step(&mut cpu, &mut bus).expect("odd LDRH");
    assert_eq!(cpu.reg(2), 0x3400_0080);

    // LDRSB at the same odd byte gives the same signed value.
    let (mut cpu, mut bus) = arm(half(0xE, 1, 1, 1, 0, 1, 3, 2, 1, 0, 0));
    bus.write8(EWRAM_BASE + 0x40, 0x7F).expect("positive byte");
    bus.write8(EWRAM_BASE + 0x41, 0x80).expect("negative byte");
    cpu.set_reg(3, EWRAM_BASE + 0x41);
    interpret::step(&mut cpu, &mut bus).expect("LDRSB");
    assert_eq!(cpu.reg(2), 0xFFFF_FF80);

    let (mut cpu, mut bus) = arm(half(0xE, 1, 1, 1, 0, 1, 3, 2, 1, 0, 0));
    bus.write8(EWRAM_BASE + 0x40, 0x7F).expect("positive byte");
    cpu.set_reg(3, EWRAM_BASE + 0x40);
    interpret::step(&mut cpu, &mut bus).expect("positive LDRSB");
    assert_eq!(cpu.reg(2), 0x7F);
}

#[test]
fn strh_forces_alignment_and_pc_base_pc_source_follow_pipeline_offsets() {
    let (mut cpu, mut bus) = arm(half(0xE, 1, 1, 1, 0, 0, 3, 2, 0, 1, 0));
    cpu.set_reg(3, EWRAM_BASE + 0x41);
    cpu.set_reg(2, 0xABCD_5678);
    interpret::step(&mut cpu, &mut bus).expect("odd STRH");
    assert_eq!(bus.read16(EWRAM_BASE + 0x40), Ok(0x5678));
    assert_eq!(bus.read16(EWRAM_BASE + 0x42), Ok(0));

    // LDRH r0,[pc,#4]: base = instruction + 8, target = instruction + 12.
    let (mut cpu, mut bus) = arm(half(0xE, 1, 1, 1, 0, 1, 15, 0, 0, 1, 4));
    bus.write16(EWRAM_BASE + 12, 0x1357).expect("seed");
    interpret::step(&mut cpu, &mut bus).expect("PC-based LDRH");
    assert_eq!(cpu.reg(0), 0x1357);

    // STRH pc,[r1] stores the low half of instruction address + 12.
    let (mut cpu, mut bus) = arm(half(0xE, 1, 1, 1, 0, 0, 1, 15, 0, 1, 0));
    cpu.set_reg(1, EWRAM_BASE + 0x40);
    interpret::step(&mut cpu, &mut bus).expect("PC source STRH");
    assert_eq!(
        bus.read16(EWRAM_BASE + 0x40),
        Ok(((EWRAM_BASE + 12) & 0xFFFF) as u16)
    );

    // Narrow loads into PC remain explicit unsupported.
    for (s, h) in [(0, 1), (1, 0), (1, 1)] {
        let raw = half(0xE, 1, 1, 1, 0, 1, 3, 15, s, h, 0);
        let (mut cpu, mut bus) = arm(raw);
        assert!(matches!(
            interpret::step(&mut cpu, &mut bus),
            Err(Error::UnsupportedInstruction { .. })
        ));
    }
}

#[test]
fn typed_ir_records_width_sign_and_offsets() {
    let instruction =
        decode_arm(0, half(0xE, 0, 1, 0, 0, 1, 3, 2, 1, 1, 4)).expect("post register LDRSH");
    assert_eq!(
        instruction.op,
        Op::MemSingle {
            load: true,
            width: MemWidth::Half,
            signed: true,
            rd: 2,
            rn: 3,
            offset: MemOffset::RegShift {
                rm: 4,
                shift: gba_native_core::ir::Shift::Imm {
                    kind: gba_native_core::ir::ShiftKind::Lsl,
                    amount: 0
                }
            },
            add: true,
            pre_index: false,
            writeback: true,
            user_access: false,
        }
    );
    let instruction = decode_arm(0, half(0xE, 1, 1, 1, 0, 1, 3, 2, 1, 0, 0x20)).expect("LDRSB");
    assert_eq!(
        instruction.op,
        Op::MemSingle {
            load: true,
            width: MemWidth::Byte,
            signed: true,
            rd: 2,
            rn: 3,
            offset: MemOffset::Imm(0x20),
            add: true,
            pre_index: true,
            writeback: false,
            user_access: false,
        }
    );
}

#[test]
fn reserved_register_fields_doubleword_and_signed_stores_are_rejected() {
    // Register form with nonzero bits 11-8.
    assert!(decode_arm(0, half(0xE, 1, 1, 0, 0, 1, 3, 2, 0, 1, 0x10)).is_err());
    // Rm=15 is not an offset register.
    assert!(decode_arm(0, half(0xE, 1, 1, 0, 0, 1, 3, 2, 0, 1, 15)).is_err());
    // L=0,S=1 is the ARMv5 LDRD/STRD space, not STRH.
    for h in [0, 1] {
        let error = decode_arm(0, half(0xE, 1, 1, 1, 0, 0, 3, 2, 1, h, 0))
            .expect_err("doubleword store rejected");
        assert!(format!("{error}").contains("doubleword"), "{error}");
    }
    // Reserved L=1,S=0,H=0 load opcode.
    assert!(decode_arm(0, half(0xE, 1, 1, 1, 0, 1, 3, 2, 0, 0, 0)).is_err());
    // Post-index bit 21 is reserved and must be zero for this encoding.
    assert!(decode_arm(0, half(0xE, 0, 1, 1, 1, 1, 3, 2, 0, 1, 0)).is_err());
}

#[test]
fn alias_and_pc_writeback_forms_are_rejected_and_failed_accesses_do_not_commit() {
    for raw in [
        half(0xE, 1, 1, 1, 1, 1, 3, 3, 0, 1, 4), // pre LDRH r3,[r3,#4]!
        half(0xE, 0, 1, 1, 0, 1, 3, 3, 0, 1, 4), // post LDRH r3,[r3],#4
        half(0xE, 1, 1, 1, 1, 1, 15, 0, 0, 1, 4), // PC base + writeback
        half(0xE, 0, 1, 1, 0, 1, 15, 0, 0, 1, 4), // PC base post-index
    ] {
        let (mut cpu, mut bus) = arm(raw);
        cpu.set_reg(3, EWRAM_BASE + 0x40);
        let before = cpu.clone();
        let digest = bus.state_digest();
        assert!(matches!(
            interpret::step(&mut cpu, &mut bus),
            Err(Error::UnsupportedInstruction { .. })
        ));
        assert_eq!(cpu.pc(), before.pc());
        assert_eq!(cpu.reg(3), before.reg(3));
        assert_eq!(bus.state_digest(), digest);
    }

    // A failed writeback load leaves the base unchanged.
    let (mut cpu, mut bus) = arm(half(0xE, 1, 1, 1, 1, 1, 3, 2, 0, 1, 4));
    cpu.set_reg(3, 0x0F00_0000);
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::Unmapped { .. })
    ));
    assert_eq!(cpu.reg(3), 0x0F00_0000);
    assert_eq!(cpu.reg(2), 0);

    // A failed ROM halfword store does not update the base.
    let (mut cpu, mut bus) = arm(half(0xE, 1, 1, 1, 1, 0, 3, 2, 0, 1, 4));
    cpu.set_reg(3, 0x0800_0000);
    cpu.set_reg(2, 0xFFFF);
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::RomWrite { .. })
    ));
    assert_eq!(cpu.reg(3), 0x0800_0000);
}

#[test]
fn false_condition_halfword_access_is_a_noop_that_advances_pc() {
    let (mut cpu, mut bus) = arm(half(0x0, 1, 1, 1, 0, 1, 3, 2, 0, 1, 0));
    bus.write16(EWRAM_BASE + 0x40, 0xCAFE).expect("seed");
    cpu.set_reg(3, EWRAM_BASE + 0x40);
    cpu.set_reg(2, 0x1111_2222);
    cpu.set_nzcv(false, false, false, false); // EQ false
    let digest = bus.state_digest();
    let flags = cpu.flags();
    interpret::step(&mut cpu, &mut bus).expect("false LDRH");
    assert_eq!(cpu.reg(2), 0x1111_2222);
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
    assert_eq!(cpu.flags(), flags);
    assert_eq!(bus.state_digest(), digest);
}
