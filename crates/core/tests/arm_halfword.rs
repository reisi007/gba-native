//! Independent ARM halfword-transfer and PSR-precedence vectors.
//!
//! Encodings are composed here from the documented field layout (condition,
//! P/U/I/W/L, Rn, Rd, imm4H, 1SH1, imm4L); expected offsets and memory effects
//! are hand-derived. These tests cover the implemented pre-indexed immediate
//! `LDRH`/`STRH` subset only, not hardware conformance.

use gba_native_core::bus::{Bus, EWRAM_BASE};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::decode::decode_arm;
use gba_native_core::error::Error;
use gba_native_core::interpret;
use gba_native_core::ir::{AluOpcode, Cond, Op, Psr, PsrSrc};

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

/// Pre-indexed (`P=1`), immediate (`I=1`), no-writeback (`W=0`) `LDRH`/`STRH`
/// with bits 7..4 = `1011b` and a zero `S` bit.
fn halfword(cond: u32, load: bool, up: bool, rn: u32, rd: u32, magnitude: u32) -> u32 {
    assert!(cond < 15, "condition 1111b is not a conditional encoding");
    assert!(rn < 16 && rd < 16);
    assert!(magnitude <= 0xFF);
    (cond << 28)
        | (1 << 24)
        | ((up as u32) << 23)
        | (1 << 22)
        | ((load as u32) << 20)
        | (rn << 16)
        | (rd << 12)
        | ((magnitude >> 4) << 8)
        | (0b1011 << 4)
        | (magnitude & 0xF)
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

fn arm(word: u32) -> (Cpu, Bus) {
    let mut bus = Bus::new(cart());
    bus.write32(EWRAM_BASE, word).expect("write instruction");
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_thumb(false);
    (cpu, bus)
}

#[test]
fn reported_negative_strh_words_and_all_offset_signs_are_halfword_transfers() {
    // The exact regression words: pre-indexed negative STRH used to be taken by
    // the coarse PSR predicate and rejected as a malformed MRS.
    assert_eq!(
        decode_arm(0, 0xE143_22B0).expect("STRH r2,[r3,#-0x20]").op,
        Op::StrHalf {
            rd: 2,
            rn: 3,
            offset: -0x20
        }
    );
    assert_eq!(
        decode_arm(0, 0xE140_00B0).expect("STRH r0,[r0]").op,
        Op::StrHalf {
            rd: 0,
            rn: 0,
            offset: 0
        }
    );
    // U=0 with a zero magnitude is still a valid encoding, not a malformed one.
    assert_eq!(
        decode_arm(0, 0xE140_10B0).expect("STRH r1,[r0]").op,
        Op::StrHalf {
            rd: 1,
            rn: 0,
            offset: 0
        }
    );
    // The symmetric negative LDRH worked before and must keep working.
    assert_eq!(
        decode_arm(0, 0xE153_22B0).expect("LDRH r2,[r3,#-0x20]").op,
        Op::LdrHalf {
            rd: 2,
            rn: 3,
            offset: -0x20
        }
    );
    // Positive counterparts, independently composed.
    assert_eq!(
        decode_arm(0, 0xE1C3_22B0).expect("STRH r2,[r3,#+0x20]").op,
        Op::StrHalf {
            rd: 2,
            rn: 3,
            offset: 0x20
        }
    );
    assert_eq!(
        decode_arm(0, 0xE1D3_22B0).expect("LDRH r2,[r3,#+0x20]").op,
        Op::LdrHalf {
            rd: 2,
            rn: 3,
            offset: 0x20
        }
    );

    for magnitude in [0u32, 1, 2, 0x0F, 0x10, 0x20, 0x7F, 0x80, 0xFF] {
        let signed = magnitude as i32;
        assert_eq!(
            decode_arm(0, halfword(0xE, false, false, 3, 2, magnitude))
                .expect("negative STRH")
                .op,
            Op::StrHalf {
                rd: 2,
                rn: 3,
                offset: -signed
            },
            "STRH magnitude {magnitude:#X}"
        );
        assert_eq!(
            decode_arm(0, halfword(0xE, true, false, 3, 2, magnitude))
                .expect("negative LDRH")
                .op,
            Op::LdrHalf {
                rd: 2,
                rn: 3,
                offset: -signed
            },
            "LDRH magnitude {magnitude:#X}"
        );
        assert_eq!(
            decode_arm(0, halfword(0xE, false, true, 3, 2, magnitude))
                .expect("positive STRH")
                .op,
            Op::StrHalf {
                rd: 2,
                rn: 3,
                offset: signed
            }
        );
        assert_eq!(
            decode_arm(0, halfword(0xE, true, true, 3, 2, magnitude))
                .expect("positive LDRH")
                .op,
            Op::LdrHalf {
                rd: 2,
                rn: 3,
                offset: signed
            }
        );
    }
}

#[test]
fn every_condition_code_keeps_the_halfword_classification() {
    for cond in 0..=14u32 {
        for load in [false, true] {
            for up in [false, true] {
                let raw = halfword(cond, load, up, 3, 2, 0x20);
                let instruction = decode_arm(0, raw).expect("conditional halfword");
                assert_eq!(instruction.cond, Cond::from_bits(cond).expect("condition"));
                let offset = if up { 0x20 } else { -0x20 };
                let expected = if load {
                    Op::LdrHalf {
                        rd: 2,
                        rn: 3,
                        offset,
                    }
                } else {
                    Op::StrHalf {
                        rd: 2,
                        rn: 3,
                        offset,
                    }
                };
                assert_eq!(instruction.op, expected, "cond={cond} raw={raw:#010X}");
            }
        }
    }
    // Condition 1111b is the unconditional/newer space, not a conditional
    // halfword transfer.
    let unconditional = (halfword(0xE, false, false, 3, 2, 0) & 0x0FFF_FFFF) | (0xF << 28);
    assert!(decode_arm(0, unconditional).is_err());
}

#[test]
fn condition_false_halfwords_are_noops_that_advance_pc() {
    let target = EWRAM_BASE + 0x40;
    for load in [false, true] {
        let (mut cpu, mut bus) = arm(halfword(0x0, load, false, 3, 2, 0x20));
        cpu.set_reg(3, target + 0x20);
        cpu.set_reg(2, 0xCAFE_BABE);
        // EQ is false: Z=0.
        cpu.set_nzcv(false, false, false, false);
        let before = cpu.clone();
        let bus_before = bus.state_digest();
        bus.write16(target, 0xBEEF).expect("seed target");
        let seeded_digest = bus.state_digest();

        interpret::step(&mut cpu, &mut bus).expect("conditional no-op");
        assert_eq!(cpu.pc(), EWRAM_BASE + 4);
        assert_eq!(cpu.reg(2), 0xCAFE_BABE);
        assert_eq!(cpu.flags(), before.flags());
        assert_eq!(bus.read16(target), Ok(0xBEEF));
        assert_eq!(bus.state_digest(), seeded_digest);
        assert_ne!(seeded_digest, bus_before);
    }
}

#[test]
fn condition_true_halfwords_use_the_signed_effective_address() {
    let base = EWRAM_BASE + 0x60;
    let target = base - 0x20;

    // STRH with LT true (N=1, V=0) stores at base - 0x20.
    let (mut cpu, mut bus) = arm(halfword(0xB, false, false, 3, 2, 0x20));
    cpu.set_reg(3, base);
    cpu.set_reg(2, 0x1234_ABCD);
    cpu.set_nzcv(true, false, false, false);
    interpret::step(&mut cpu, &mut bus).expect("LT STRH executes");
    assert_eq!(bus.read16(target), Ok(0xABCD));
    assert_eq!(bus.read16(base + 0x20), Ok(0), "wrong sign must not write");
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);

    // LDRH with MI true (N=1) reads the same negative offset.
    let (mut cpu, mut bus) = arm(halfword(0x4, true, false, 3, 2, 0x20));
    cpu.set_reg(3, base);
    cpu.set_reg(2, 0);
    cpu.set_nzcv(true, false, false, false);
    bus.write16(target, 0x5A5A).expect("seed target");
    interpret::step(&mut cpu, &mut bus).expect("MI LDRH executes");
    assert_eq!(cpu.reg(2), 0x5A5A);
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);

    // A false MI (N=0) leaves the destination register untouched.
    let (mut cpu, mut bus) = arm(halfword(0x4, true, false, 3, 2, 0x20));
    cpu.set_reg(3, base);
    cpu.set_reg(2, 0xDEAD_BEEF);
    cpu.set_nzcv(false, false, false, false);
    bus.write16(target, 0x5A5A).expect("seed target");
    interpret::step(&mut cpu, &mut bus).expect("MI false");
    assert_eq!(cpu.reg(2), 0xDEAD_BEEF);
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
}

#[test]
fn psr_classification_is_unchanged_by_halfword_precedence() {
    for rd in 0..=14u32 {
        assert_eq!(
            decode_arm(0, mrs(0xE, rd, false)).expect("MRS CPSR").op,
            Op::Mrs {
                rd: rd as u8,
                psr: Psr::Cpsr
            }
        );
        assert_eq!(
            decode_arm(0, mrs(0xE, rd, true)).expect("MRS SPSR").op,
            Op::Mrs {
                rd: rd as u8,
                psr: Psr::Spsr
            }
        );
    }
    for rm in 0..=14u32 {
        assert_eq!(
            decode_arm(0, msr_reg(0xE, false, 0b1001, rm))
                .expect("MSR CPSR_fc")
                .op,
            Op::Msr {
                psr: Psr::Cpsr,
                fields: 0b1001,
                src: PsrSrc::Reg(rm as u8)
            }
        );
    }
    for rotation in 0..=15u32 {
        assert_eq!(
            decode_arm(0, msr_imm(0xE, true, 0b1000, 0xA5, rotation))
                .expect("MSR SPSR_f immediate")
                .op,
            Op::Msr {
                psr: Psr::Spsr,
                fields: 0b1000,
                src: PsrSrc::Imm(0xA5u32.rotate_right(rotation * 2))
            }
        );
    }

    // Fixed-field validation still wins over any halfword/PSR alias.
    for raw in [
        0xE10F_F000, // MRS PC
        0xE10F_0001, // MRS low bits nonzero
        0xE10E_0000, // valid MRS shape, fixed Rn != 1111 (must error as MRS)
        0xE12F_0000, // MSR fixed Rd-field != 1111
        0xE129_F010, // register MSR bits 11-4 nonzero
        0xE129_F00F, // MSR from PC
    ] {
        let error = decode_arm(0, raw).expect_err("malformed PSR encoding");
        assert!(matches!(error, Error::UnsupportedInstruction { .. }));
    }
    // 0xE10E0000 has the valid MRS field shape (bit20=0, immediate=0) but a
    // fixed Rn of 1110b instead of 1111b. It must be diagnosed as malformed MRS,
    // not as TST or a halfword transfer.
    let error = decode_arm(0, 0xE10E_0000).expect_err("MRS fixed Rn");
    let message = format!("{error}");
    assert!(message.contains("MRS"), "{message}");
    assert!(message.contains("Rn=1111"), "{message}");

    // The earlier bit-25 immediate regression must stay intact.
    assert!(matches!(
        decode_arm(0, 0xE3A0_00B0).expect("MOV immediate").op,
        Op::Mov { .. }
    ));
    assert!(matches!(
        decode_arm(0, 0xE350_00B0).expect("CMP immediate").op,
        Op::Cmp { .. }
    ));
    assert!(matches!(
        decode_arm(0, 0xE3C1_00B0).expect("BIC immediate").op,
        Op::Alu {
            opcode: AluOpcode::Bic,
            ..
        }
    ));
}
