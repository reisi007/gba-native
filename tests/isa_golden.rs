//! Independent ARM immediate-family golden vectors.
//!
//! The native differential test compares the emitter against the reference
//! interpreter, so a shared decode bug would pass it. These vectors are written
//! directly from the documented encoding (`imm8` rotated right by `2*rotation`)
//! and independently expected arithmetic/flags. They are ISA-subset evidence,
//! not hardware conformance or a complete ARMv4T proof.

use gba_native::bus::{Bus, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::decode::decode_arm;
use gba_native::error::Error;
use gba_native::interpret;
use gba_native::ir::{AluOpcode, Cond, Op, Src};

const INITIAL_FLAGS: (bool, bool, bool, bool) = (true, true, false, true);

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn encode_immediate(
    opcode: u32,
    set_flags: bool,
    rn: u32,
    rd: u32,
    imm8: u32,
    rotation: u32,
) -> u32 {
    assert!(imm8 < 256);
    assert!(rotation < 16);
    0xE000_0000
        | (1 << 25)
        | (opcode << 21)
        | ((set_flags as u32) << 20)
        | (rn << 16)
        | (rd << 12)
        | (rotation << 8)
        | imm8
}

/// Documented ARM immediate expansion: `imm8 ROR (2 * rotation)`.
fn expanded(imm8: u32, rotation: u32) -> u32 {
    if rotation == 0 {
        imm8
    } else {
        imm8.rotate_right(rotation * 2)
    }
}

fn expanded_carry(imm8: u32, rotation: u32) -> Option<bool> {
    if rotation == 0 {
        None
    } else {
        Some(expanded(imm8, rotation) & 0x8000_0000 != 0)
    }
}

fn new_cpu(rn: u32, rhs: u32) -> Cpu {
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(0x0800_0000);
    cpu.set_reg(2, rn);
    cpu.set_reg(3, rhs);
    cpu.set_nzcv(
        INITIAL_FLAGS.0,
        INITIAL_FLAGS.1,
        INITIAL_FLAGS.2,
        INITIAL_FLAGS.3,
    );
    cpu
}

fn spec_add(a: u32, b: u32) -> (u32, bool, bool) {
    let wide = u64::from(a) + u64::from(b);
    let result = wide as u32;
    let carry = wide > u64::from(u32::MAX);
    let overflow = ((a ^ result) & (b ^ result) & 0x8000_0000) != 0;
    (result, carry, overflow)
}

fn spec_sub(a: u32, b: u32) -> (u32, bool, bool) {
    let result = a.wrapping_sub(b);
    let no_borrow = a >= b;
    let overflow = ((a ^ b) & (a ^ result) & 0x8000_0000) != 0;
    (result, no_borrow, overflow)
}

fn expected_flags(result: u32, carry: bool, overflow: bool) -> (bool, bool, bool, bool) {
    (result & 0x8000_0000 != 0, result == 0, carry, overflow)
}

#[test]
fn arm_immediate_rotation_sweep_matches_independent_semantics() {
    let mut bus = Bus::new(cart());
    let digest_before = bus.state_digest();

    for imm8 in 0..=255u32 {
        for rotation in 0..=15u32 {
            let value = expanded(imm8, rotation);
            let carry = expanded_carry(imm8, rotation);

            for set_flags in [false, true] {
                // MOV rd, #imm
                let raw = encode_immediate(13, set_flags, 0, 3, imm8, rotation);
                let instr = decode_arm(0x0800_0000, raw).expect("MOV immediate decodes");
                assert_eq!(instr.cond, Cond::Al);
                assert_eq!(
                    instr.op,
                    Op::Mov {
                        rd: 3,
                        src: Src::Imm { value, carry },
                        set_flags
                    }
                );
                let mut cpu = new_cpu(0, 0xDEAD_BEEF);
                interpret::execute(&mut cpu, &mut bus, instr).expect("MOV executes");
                assert_eq!(cpu.reg(3), value);
                let n = value & 0x8000_0000 != 0;
                let z = value == 0;
                let expected = if set_flags {
                    (n, z, carry.unwrap_or(INITIAL_FLAGS.2), INITIAL_FLAGS.3)
                } else {
                    INITIAL_FLAGS
                };
                assert_eq!(
                    cpu.flags(),
                    expected,
                    "MOV imm8={imm8} rotation={rotation} s={set_flags}"
                );

                // ADD rd, rn, #imm
                let raw = encode_immediate(4, set_flags, 2, 3, imm8, rotation);
                let instr = decode_arm(0x0800_0000, raw).expect("ADD immediate decodes");
                assert_eq!(
                    instr.op,
                    Op::Add {
                        rd: 3,
                        rn: 2,
                        rhs: Src::Imm { value, carry },
                        set_flags
                    }
                );
                let rn = 0x7FFF_FFF0;
                let mut cpu = new_cpu(rn, 0xDEAD_BEEF);
                interpret::execute(&mut cpu, &mut bus, instr).expect("ADD executes");
                let (result, add_carry, overflow) = spec_add(rn, value);
                assert_eq!(cpu.reg(3), result);
                let expected = if set_flags {
                    expected_flags(result, add_carry, overflow)
                } else {
                    INITIAL_FLAGS
                };
                assert_eq!(
                    cpu.flags(),
                    expected,
                    "ADD imm8={imm8} rotation={rotation} s={set_flags}"
                );

                // SUB rd, rn, #imm
                let raw = encode_immediate(2, set_flags, 2, 3, imm8, rotation);
                let instr = decode_arm(0x0800_0000, raw).expect("SUB immediate decodes");
                assert_eq!(
                    instr.op,
                    Op::Sub {
                        rd: 3,
                        rn: 2,
                        rhs: Src::Imm { value, carry },
                        set_flags
                    }
                );
                let mut cpu = new_cpu(rn, 0xDEAD_BEEF);
                interpret::execute(&mut cpu, &mut bus, instr).expect("SUB executes");
                let (result, no_borrow, overflow) = spec_sub(rn, value);
                assert_eq!(cpu.reg(3), result);
                let expected = if set_flags {
                    expected_flags(result, no_borrow, overflow)
                } else {
                    INITIAL_FLAGS
                };
                assert_eq!(
                    cpu.flags(),
                    expected,
                    "SUB imm8={imm8} rotation={rotation} s={set_flags}"
                );
            }

            // CMP rn, #imm always updates flags and has its fixed Rd=0 encoding.
            let raw = encode_immediate(10, true, 2, 0, imm8, rotation);
            let instr = decode_arm(0x0800_0000, raw).expect("CMP immediate decodes");
            assert_eq!(
                instr.op,
                Op::Cmp {
                    rn: 2,
                    rhs: Src::Imm { value, carry }
                }
            );
            let rn = 0x7FFF_FFF0;
            let mut cpu = new_cpu(rn, 0xDEAD_BEEF);
            interpret::execute(&mut cpu, &mut bus, instr).expect("CMP executes");
            let (result, no_borrow, overflow) = spec_sub(rn, value);
            assert_eq!(cpu.reg(3), 0xDEAD_BEEF);
            assert_eq!(
                cpu.flags(),
                expected_flags(result, no_borrow, overflow),
                "CMP imm8={imm8} rotation={rotation}"
            );
        }
    }

    assert_eq!(
        bus.state_digest(),
        digest_before,
        "immediate data-processing instructions must not write memory"
    );
}

#[test]
fn exact_problem_encodings_are_classified_by_bit25_not_low_nibble() {
    // CMP r0,#0xB0: bit25=1, so bits 7-4 are immediate payload, not an opcode.
    let cmp = decode_arm(0x0800_0000, 0xE350_00B0).expect("CMP immediate");
    assert_eq!(
        cmp.op,
        Op::Cmp {
            rn: 0,
            rhs: Src::Imm {
                value: 0xB0,
                carry: None
            }
        }
    );

    // BIC r0,r1,#0xB0 must be a data-processing instruction, not STRH r0,[r1].
    assert_eq!(
        decode_arm(0x0800_0004, 0xE3C1_00B0)
            .expect("BIC immediate")
            .op,
        Op::Alu {
            opcode: AluOpcode::Bic,
            rd: Some(0),
            rn: 1,
            rhs: Src::Imm {
                value: 0xB0,
                carry: None
            },
            set_flags: false
        }
    );

    // CMP with Rd != 0 is malformed and must not alias STRH r1,[r0].
    let malformed = decode_arm(0x0800_0008, 0xE350_10B4).expect_err("malformed CMP rejected");
    assert!(matches!(
        malformed,
        Error::UnsupportedInstruction {
            raw: 0xE350_10B4,
            ..
        }
    ));
}

#[test]
fn rejected_alias_candidates_leave_guest_memory_unchanged() {
    let cases = [
        (0xE120_F000u32, 0u8), // empty-mask MSR would have aliased STRH r0,[r0]
        (0xE350_10B4u32, 0u8), // malformed CMP would have aliased STRH r1,[r0]
    ];
    for (raw, base_register) in cases {
        let mut bus = Bus::new(cart());
        bus.write32(EWRAM_BASE, raw).expect("write instruction");
        let target = EWRAM_BASE + 0x40;
        bus.write16(target, 0xBEEF).expect("seed target");
        let digest_before = bus.state_digest();

        let mut cpu = Cpu::direct_entry();
        cpu.set_pc(EWRAM_BASE);
        cpu.set_reg(base_register, target);
        let error = interpret::step(&mut cpu, &mut bus).expect_err("unsupported encoding");
        assert!(matches!(error, Error::UnsupportedInstruction { .. }));
        assert_eq!(cpu.pc(), EWRAM_BASE, "PC must not advance on rejection");
        assert_eq!(bus.read16(target), Ok(0xBEEF));
        assert_eq!(
            bus.state_digest(),
            digest_before,
            "rejected encoding must not mutate memory"
        );
    }
}

#[test]
fn cmp_with_s_clear_is_rejected() {
    // CMP (opcode 10) with S=0 is not an ARM CMP encoding.
    let raw = encode_immediate(10, false, 2, 0, 1, 0);
    assert!(decode_arm(0x0800_0000, raw).is_err());
}
