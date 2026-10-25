//! Independent ARM barrel-shifter vectors.
//!
//! Every expected value/carry below is derived from the documented shift rules
//! (including 0/32/>32 and the low-eight-bit register amount), not from the
//! crate's shift helpers. Decode shape, result and full NZCV are checked.

use gba_native::bus::Bus;
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::decode::decode_arm;
use gba_native::interpret;
use gba_native::ir::{Op, Shift, ShiftKind, Src};

const VALUES: [u32; 10] = [
    0x0000_0000,
    0x0000_0001,
    0x8000_0000,
    0x8000_0001,
    0xFFFF_FFFF,
    0x4000_0000,
    0xAAAA_AAAA,
    0x5555_5555,
    0x7FFF_FFFF,
    0xDEAD_BEEF,
];

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

/// MOVS rd,Rm,<shift>.
fn movs(rd: u32, op2: u32) -> u32 {
    0xE000_0000 | (13 << 21) | (1 << 20) | (rd << 12) | op2
}

fn shift_imm(kind: u32, amount: u32, rm: u32) -> u32 {
    (amount << 7) | (kind << 5) | rm
}

fn shift_reg(kind: u32, rs: u32, rm: u32) -> u32 {
    (rs << 8) | (kind << 5) | (1 << 4) | rm
}

fn spec_imm(kind: u32, amount: u32, value: u32, carry: bool) -> (u32, Option<bool>) {
    match kind {
        0 => {
            if amount == 0 {
                (value, None)
            } else {
                (
                    value.wrapping_shl(amount),
                    Some((value >> (32 - amount)) & 1 != 0),
                )
            }
        }
        1 => {
            let n = if amount == 0 { 32 } else { amount };
            let result = if n == 32 { 0 } else { value >> n };
            (result, Some((value >> (n - 1)) & 1 != 0))
        }
        2 => {
            let n = if amount == 0 { 32 } else { amount };
            let result = if n == 32 {
                ((value as i32) >> 31) as u32
            } else {
                ((value as i32) >> n) as u32
            };
            (result, Some((value >> (n - 1)) & 1 != 0))
        }
        _ => {
            if amount == 0 {
                (
                    (u32::from(carry) << 31) | (value >> 1),
                    Some(value & 1 != 0),
                )
            } else {
                (
                    value.rotate_right(amount),
                    Some((value >> (amount - 1)) & 1 != 0),
                )
            }
        }
    }
}

fn spec_reg(kind: u32, amount: u32, value: u32, _carry: bool) -> (u32, Option<bool>) {
    let n = amount & 0xFF;
    match kind {
        0 => {
            if n == 0 {
                (value, None)
            } else if n < 32 {
                (value.wrapping_shl(n), Some((value >> (32 - n)) & 1 != 0))
            } else if n == 32 {
                (0, Some(value & 1 != 0))
            } else {
                (0, Some(false))
            }
        }
        1 => {
            if n == 0 {
                (value, None)
            } else if n < 32 {
                (value >> n, Some((value >> (n - 1)) & 1 != 0))
            } else if n == 32 {
                (0, Some(value >> 31 != 0))
            } else {
                (0, Some(false))
            }
        }
        2 => {
            if n == 0 {
                (value, None)
            } else if n < 32 {
                (
                    ((value as i32) >> n) as u32,
                    Some((value >> (n - 1)) & 1 != 0),
                )
            } else {
                (((value as i32) >> 31) as u32, Some(value >> 31 != 0))
            }
        }
        _ => {
            if n == 0 {
                (value, None)
            } else {
                let rotate = n % 32;
                if rotate == 0 {
                    (value, Some(value >> 31 != 0))
                } else {
                    (
                        value.rotate_right(rotate),
                        Some((value >> (rotate - 1)) & 1 != 0),
                    )
                }
            }
        }
    }
}

fn run(raw: u32, addr: u32, value: u32, amount: u32, carry: bool) -> Cpu {
    let mut bus = Bus::new(cart());
    let instr = decode_arm(addr, raw).expect("shift decodes");
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(addr);
    cpu.set_reg(1, value);
    cpu.set_reg(3, amount);
    cpu.set_nzcv(true, true, carry, true);
    interpret::execute(&mut cpu, &mut bus, instr).expect("shift executes");
    cpu
}

fn expected_flags(
    result: u32,
    carry: Option<bool>,
    old_c: bool,
    old_v: bool,
) -> (bool, bool, bool, bool) {
    (
        result & 0x8000_0000 != 0,
        result == 0,
        carry.unwrap_or(old_c),
        old_v,
    )
}

#[test]
fn register_shift_matrix_matches_documented_rules() {
    let amounts = [0u32, 1, 31, 32, 33, 63, 64, 255, 256];
    for kind in 0..=3u32 {
        for amount in amounts {
            for value in VALUES {
                for carry in [false, true] {
                    let raw = movs(2, shift_reg(kind, 3, 1));
                    let instr = decode_arm(0x0800_0000, raw).expect("register shift");
                    let expected_shift = match kind {
                        0 => ShiftKind::Lsl,
                        1 => ShiftKind::Lsr,
                        2 => ShiftKind::Asr,
                        _ => ShiftKind::Ror,
                    };
                    assert_eq!(
                        instr.op,
                        Op::Mov {
                            rd: 2,
                            src: Src::RegShift {
                                rm: 1,
                                shift: Shift::Reg {
                                    kind: expected_shift,
                                    rm: 3
                                }
                            },
                            set_flags: true
                        }
                    );
                    let (result, shifter_carry) = spec_reg(kind, amount, value, carry);
                    let cpu = run(raw, 0x0800_0000, value, amount, carry);
                    assert_eq!(
                        cpu.reg(2),
                        result,
                        "kind={kind} amount={amount} value={value:#010X}"
                    );
                    assert_eq!(
                        cpu.flags(),
                        expected_flags(result, shifter_carry, carry, true),
                        "kind={kind} amount={amount} value={value:#010X} carry={carry}"
                    );
                }
            }
        }
    }
}

#[test]
fn immediate_shift_matrix_includes_all_zero_specials() {
    for kind in 0..=3u32 {
        for amount in [0u32, 1, 15, 31] {
            for value in VALUES {
                for carry in [false, true] {
                    let raw = movs(2, shift_imm(kind, amount, 1));
                    let instr = decode_arm(0x0800_0000, raw).expect("immediate shift");
                    if kind == 0 && amount == 0 {
                        assert_eq!(
                            instr.op,
                            Op::Mov {
                                rd: 2,
                                src: Src::Reg(1),
                                set_flags: true
                            },
                            "LSL#0 is the unshifted register form"
                        );
                    } else {
                        assert!(matches!(
                            instr.op,
                            Op::Mov {
                                src: Src::RegShift {
                                    shift: Shift::Imm { .. },
                                    ..
                                },
                                ..
                            }
                        ));
                    }
                    let (result, shifter_carry) = spec_imm(kind, amount, value, carry);
                    let cpu = run(raw, 0x0800_0000, value, 0, carry);
                    assert_eq!(
                        cpu.reg(2),
                        result,
                        "kind={kind} amount={amount} value={value:#010X}"
                    );
                    assert_eq!(
                        cpu.flags(),
                        expected_flags(result, shifter_carry, carry, true),
                        "kind={kind} amount={amount} value={value:#010X} carry={carry}"
                    );
                }
            }
        }
    }
}

#[test]
fn rrx_uses_the_old_carry_and_produces_both_sign_cases() {
    for value in [0x0000_0000u32, 0x0000_0001, 0x8000_0000, 0x8000_0001] {
        for carry in [false, true] {
            let raw = movs(2, shift_imm(3, 0, 1));
            let cpu = run(raw, 0x0800_0000, value, 0, carry);
            let expected = (u32::from(carry) << 31) | (value >> 1);
            assert_eq!(cpu.reg(2), expected, "value={value:#010X} carry={carry}");
            assert_eq!(
                cpu.flags(),
                (
                    expected & 0x8000_0000 != 0,
                    expected == 0,
                    value & 1 != 0,
                    true
                )
            );
        }
    }
}

#[test]
fn register_shift_by_pc_is_an_explicit_error() {
    let raw = movs(2, shift_reg(0, 15, 1));
    let error = decode_arm(0x0800_0000, raw).expect_err("Rs=15 is unpredictable");
    assert!(format!("{error}").contains("Rs=15"), "{error}");
}
