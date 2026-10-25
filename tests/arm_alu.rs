//! Independent ARM data-processing vectors for all sixteen ALU opcodes.
//!
//! Arithmetic results/flags are recomputed with wide integer composition in the
//! test, logical carry comes from the documented shifter, and V preservation for
//! logical operations is checked explicitly. Encodings and expected results are
//! not derived from the crate decoder/interpreter.

use gba_native::bus::Bus;
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::decode::decode_arm;
use gba_native::interpret;
use gba_native::ir::{AluOpcode, Op, Src};

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn dp(opcode: u32, set_flags: bool, rn: u32, rd: u32, op2: u32) -> u32 {
    0xE000_0000 | (opcode << 21) | ((set_flags as u32) << 20) | (rn << 16) | (rd << 12) | op2
}

fn shift_imm(kind: u32, amount: u32, rm: u32) -> u32 {
    (amount << 7) | (kind << 5) | rm
}

fn spec_add(a: u32, b: u32, carry_in: bool) -> (u32, bool, bool) {
    let wide = u64::from(a) + u64::from(b) + u64::from(carry_in);
    let result = wide as u32;
    let overflow = ((a ^ result) & (b ^ result) & 0x8000_0000) != 0;
    (result, wide > u64::from(u32::MAX), overflow)
}

fn spec_sub(a: u32, b: u32, carry_in: bool) -> (u32, bool, bool) {
    let borrow = u64::from(!carry_in);
    let wide = u64::from(a).wrapping_sub(u64::from(b)).wrapping_sub(borrow);
    let result = wide as u32;
    let no_borrow = u64::from(a) >= u64::from(b) + borrow;
    let overflow = ((a ^ b) & (a ^ result) & 0x8000_0000) != 0;
    (result, no_borrow, overflow)
}

fn flags(n: bool, z: bool, c: bool, v: bool) -> (bool, bool, bool, bool) {
    (n, z, c, v)
}

fn run(raw: u32, rn: u32, rhs: u32, initial: (bool, bool, bool, bool)) -> Cpu {
    let mut bus = Bus::new(cart());
    let instr = decode_arm(0x0800_0000, raw).expect("ALU decodes");
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(0x0800_0000);
    cpu.set_reg(0, 0xDEAD_BEEF);
    cpu.set_reg(2, rn);
    cpu.set_reg(3, rhs);
    cpu.set_nzcv(initial.0, initial.1, initial.2, initial.3);
    interpret::execute(&mut cpu, &mut bus, instr).expect("ALU executes");
    cpu
}

fn arithmetic(
    opcode: u32,
    rn: u32,
    rhs: u32,
    carry_in: bool,
) -> (Option<u32>, (bool, bool, bool, bool)) {
    match opcode {
        2 => {
            let (r, c, v) = spec_sub(rn, rhs, true);
            (Some(r), flags(r & 0x8000_0000 != 0, r == 0, c, v))
        }
        3 => {
            let (r, c, v) = spec_sub(rhs, rn, true);
            (Some(r), flags(r & 0x8000_0000 != 0, r == 0, c, v))
        }
        4 => {
            let (r, c, v) = spec_add(rn, rhs, false);
            (Some(r), flags(r & 0x8000_0000 != 0, r == 0, c, v))
        }
        5 => {
            let (r, c, v) = spec_add(rn, rhs, carry_in);
            (Some(r), flags(r & 0x8000_0000 != 0, r == 0, c, v))
        }
        6 => {
            let (r, c, v) = spec_sub(rn, rhs, carry_in);
            (Some(r), flags(r & 0x8000_0000 != 0, r == 0, c, v))
        }
        7 => {
            let (r, c, v) = spec_sub(rhs, rn, carry_in);
            (Some(r), flags(r & 0x8000_0000 != 0, r == 0, c, v))
        }
        10 => {
            let (r, c, v) = spec_sub(rn, rhs, true);
            (None, flags(r & 0x8000_0000 != 0, r == 0, c, v))
        }
        11 => {
            let (r, c, v) = spec_add(rn, rhs, false);
            (None, flags(r & 0x8000_0000 != 0, r == 0, c, v))
        }
        _ => unreachable!(),
    }
}

fn expected_ir(opcode: u32) -> Option<AluOpcode> {
    Some(match opcode {
        0 => AluOpcode::And,
        1 => AluOpcode::Eor,
        3 => AluOpcode::Rsb,
        5 => AluOpcode::Adc,
        6 => AluOpcode::Sbc,
        7 => AluOpcode::Rsc,
        8 => AluOpcode::Tst,
        9 => AluOpcode::Teq,
        11 => AluOpcode::Cmn,
        12 => AluOpcode::Orr,
        14 => AluOpcode::Bic,
        15 => AluOpcode::Mvn,
        _ => return None,
    })
}

#[test]
fn all_sixteen_opcodes_match_independent_immediate_and_register_semantics() {
    let arithmetic_ops = [2u32, 3, 4, 5, 6, 7, 10, 11];
    let test_ops = [8u32, 9, 10, 11];
    let initial = (true, true, false, true);

    for opcode in 0..=15u32 {
        let is_test = test_ops.contains(&opcode);
        let rn = if matches!(opcode, 13 | 15) { 0 } else { 2 };
        let rd = if is_test { 0 } else { 4 };
        for set_flags in [false, true] {
            if is_test && !set_flags {
                continue;
            }
            let rhs = if opcode == 13 || opcode == 15 {
                0x8000_0001
            } else {
                0x1234_5678
            };
            let raw = dp(opcode, set_flags, rn, rd, 3);
            let cpu = run(raw, 0x7FFF_FFFF, rhs, initial);
            let ir = decode_arm(0, raw).expect("IR").op;
            match opcode {
                0 => assert_eq!(
                    ir,
                    Op::Alu {
                        opcode: AluOpcode::And,
                        rd: Some(4),
                        rn: 2,
                        rhs: Src::Reg(3),
                        set_flags
                    }
                ),
                1 => assert_eq!(
                    ir,
                    Op::Alu {
                        opcode: AluOpcode::Eor,
                        rd: Some(4),
                        rn: 2,
                        rhs: Src::Reg(3),
                        set_flags
                    }
                ),
                2 => assert_eq!(
                    ir,
                    Op::Sub {
                        rd: 4,
                        rn: 2,
                        rhs: Src::Reg(3),
                        set_flags
                    }
                ),
                4 => assert_eq!(
                    ir,
                    Op::Add {
                        rd: 4,
                        rn: 2,
                        rhs: Src::Reg(3),
                        set_flags
                    }
                ),
                10 => assert_eq!(
                    ir,
                    Op::Cmp {
                        rn: 2,
                        rhs: Src::Reg(3)
                    }
                ),
                13 => assert_eq!(
                    ir,
                    Op::Mov {
                        rd: 4,
                        src: Src::Reg(3),
                        set_flags
                    }
                ),
                _ => {
                    let expected = expected_ir(opcode).expect("opcode");
                    assert_eq!(
                        ir,
                        Op::Alu {
                            opcode: expected,
                            rd: if is_test { None } else { Some(4) },
                            rn: if opcode == 15 { 0 } else { 2 },
                            rhs: Src::Reg(3),
                            set_flags
                        }
                    );
                }
            }

            if arithmetic_ops.contains(&opcode) {
                let (expected_result, expected_flags) = arithmetic(opcode, 0x7FFF_FFFF, rhs, false);
                if let Some(result) = expected_result {
                    assert_eq!(cpu.reg(4), result, "opcode={opcode}");
                }
                if set_flags {
                    assert_eq!(cpu.flags(), expected_flags, "opcode={opcode}");
                } else {
                    assert_eq!(
                        cpu.flags(),
                        initial,
                        "opcode={opcode} S=0 must preserve flags"
                    );
                }
            } else {
                let result = match opcode {
                    0 | 8 => 0x7FFF_FFFF & rhs,
                    1 | 9 => 0x7FFF_FFFF ^ rhs,
                    12 => 0x7FFF_FFFF | rhs,
                    13 => rhs,
                    14 => 0x7FFF_FFFF & !rhs,
                    15 => !rhs,
                    _ => unreachable!(),
                };
                if !is_test {
                    assert_eq!(cpu.reg(4), result, "opcode={opcode}");
                }
                if set_flags {
                    assert_eq!(
                        cpu.flags(),
                        flags(result & 0x8000_0000 != 0, result == 0, initial.2, initial.3),
                        "logical V/C preservation, opcode={opcode}"
                    );
                } else {
                    assert_eq!(cpu.flags(), initial);
                }
            }
        }
    }
}

#[test]
fn adc_sbc_rsc_cover_carry_in_overflow_and_borrow_edges() {
    let carry_cases = [
        (0x7FFF_FFFFu32, 0u32),
        (0xFFFF_FFFF, 0),
        (0x8000_0000, 0xFFFF_FFFF),
        (0x0000_0000, 0x0000_0001),
        (0x8000_0000, 0x7FFF_FFFF),
        (0x1234_5678, 0x8765_4321),
    ];
    for opcode in [5u32, 6, 7] {
        for (rn, rhs) in carry_cases {
            for carry_in in [false, true] {
                let initial = (false, false, carry_in, false);
                let raw = dp(opcode, true, 2, 4, 3);
                let cpu = run(raw, rn, rhs, initial);
                let (expected_result, expected_flags) = arithmetic(opcode, rn, rhs, carry_in);
                assert_eq!(
                    cpu.reg(4),
                    expected_result.expect("write"),
                    "opcode={opcode} rn={rn:#010X} rhs={rhs:#010X} c={carry_in}"
                );
                assert_eq!(
                    cpu.flags(),
                    expected_flags,
                    "opcode={opcode} rn={rn:#010X} rhs={rhs:#010X} c={carry_in}"
                );
            }
        }
    }
}

#[test]
fn adc_and_sbc_evaluate_rrx_before_updating_flags() {
    // ADCS r4,r2,r3,ROR#0 (RRX): the old C is both the RRX input and the ADC
    // carry-in. r3=2 with C=1 gives RRX(2)=0x80000001 and shifter carry 0,
    // so a wrong ordering would produce 0x80000001 instead of 0x80000002.
    let raw = dp(5, true, 2, 4, shift_imm(3, 0, 3));
    let cpu = run(raw, 0, 0x0000_0002, (false, false, true, false));
    assert_eq!(cpu.reg(4), 0x8000_0002);
    assert_eq!(cpu.flags(), (true, false, false, false));

    // SBCS r4,r2,r3,ROR#0 with old C=1: RRX(2)=0x80000001 and borrow-in 0,
    // giving 0xFFFFFFFF-0x80000001 = 0x7FFFFFFE. Using the shifter carry (0)
    // as the borrow-in would give 0x7FFFFFFD.
    let raw = dp(6, true, 2, 4, shift_imm(3, 0, 3));
    let cpu = run(raw, 0xFFFF_FFFF, 0x0000_0002, (false, false, true, false));
    assert_eq!(cpu.reg(4), 0x7FFF_FFFE);
    assert_eq!(cpu.flags(), (false, false, true, false));
}

#[test]
fn logical_operations_take_carry_from_the_shifter_not_arithmetic() {
    // ANDS r4,r2,r3,LSR#1: C is bit 0 of r3, V is preserved.
    let raw = dp(0, true, 2, 4, shift_imm(1, 1, 3));
    let cpu = run(raw, 0xF000_0000, 0x0000_0003, (false, false, true, true));
    assert_eq!(cpu.reg(4), 0);
    assert_eq!(cpu.flags(), (false, true, true, true));

    // ORRS with ASR#31: C is bit 30 of r3, result uses the sign-filled operand.
    let raw = dp(12, true, 2, 4, shift_imm(2, 31, 3));
    let cpu = run(raw, 0x0000_0000, 0x8000_0002, (true, true, false, false));
    assert_eq!(cpu.reg(4), 0xFFFF_FFFF);
    assert_eq!(cpu.flags(), (true, false, false, false));

    // ADDS r4,r2,r3,LSR#1: the shifter carry of 2 LSR#1 is 0, but the addition
    // 0xFFFFFFFF+1 produces carry-out 1. Arithmetic flags must win.
    let raw = dp(4, true, 2, 4, shift_imm(1, 1, 3));
    let cpu = run(raw, 0xFFFF_FFFF, 0x0000_0002, (false, false, false, false));
    assert_eq!(cpu.reg(4), 0);
    assert_eq!(cpu.flags(), (false, true, true, false));
}

#[test]
fn immediate_forms_and_carry_preservation_for_logical_rotation() {
    // ANDS r4,r2,#0xFF000000 (imm8=0xFF, rotate right 8) sets C from bit 31.
    let raw = dp(0, true, 2, 4, (1 << 25) | (4 << 8) | 0xFF);
    let instr = decode_arm(0, raw).expect("AND immediate");
    assert_eq!(
        instr.op,
        Op::Alu {
            opcode: AluOpcode::And,
            rd: Some(4),
            rn: 2,
            rhs: Src::Imm {
                value: 0xFF00_0000,
                carry: Some(true)
            },
            set_flags: true
        }
    );
    let cpu = run(raw, 0xFFFF_FFFF, 0, (false, false, false, true));
    assert_eq!(cpu.reg(4), 0xFF00_0000);
    assert_eq!(cpu.flags(), (true, false, true, true));

    // MOVS r4,#0xF0000000 rotates by 4: carry is the rotated sign bit.
    let raw = dp(13, true, 0, 4, (1 << 25) | (4 << 8) | 0xF0);
    let instr = decode_arm(0, raw).expect("MOV rotated immediate");
    assert_eq!(
        instr.op,
        Op::Mov {
            rd: 4,
            src: Src::Imm {
                value: 0xF000_0000,
                carry: Some(true)
            },
            set_flags: true
        }
    );
    let cpu = run(raw, 0, 0, (false, false, false, true));
    assert_eq!(cpu.reg(4), 0xF000_0000);
    assert_eq!(cpu.flags(), (true, false, true, true));
}

#[test]
fn mov_mvn_and_test_ops_keep_their_encoding_restrictions() {
    assert!(decode_arm(0, dp(13, false, 1, 4, 3)).is_err(), "MOV Rn!=0");
    assert!(decode_arm(0, dp(15, false, 1, 4, 3)).is_err(), "MVN Rn!=0");
    assert!(
        decode_arm(0, dp(8, false, 2, 0, 3)).is_err(),
        "TST S=0 is PSR space"
    );
    assert!(decode_arm(0, dp(8, true, 2, 4, 3)).is_err(), "TST Rd!=0");
    assert!(
        decode_arm(0, dp(10, true, 2, 15, 3)).is_err(),
        "CMP P-variant"
    );
}
