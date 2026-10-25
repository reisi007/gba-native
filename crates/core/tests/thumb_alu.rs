//! Independent Thumb low-register ALU, shift, ADR/SP and high-register vectors.
//!
//! Expected arithmetic/flag values are hand-derived in this file. The tests also
//! assert typed IR (including the explicit shift representation) and the
//! documented flag-preservation rules.

use gba_native_core::bus::Bus;
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::decode::decode_thumb;
use gba_native_core::interpret;
use gba_native_core::ir::{AluOpcode, Op, Shift, ShiftKind, Src};

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn alu(kind: u16, rs: u16, rd: u16) -> u16 {
    0x4000 | (kind << 6) | (rs << 3) | rd
}

fn shift_imm(kind: u16, amount: u16, rs: u16, rd: u16) -> u16 {
    (kind << 11) | (amount << 6) | (rs << 3) | rd
}

fn hi(kind: u16, rd_hi: u16, rs_hi: u16, rs: u16, rd: u16) -> u16 {
    0x4400 | (kind << 8) | (rd_hi << 7) | (rs_hi << 6) | (rs << 3) | rd
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

fn spec_shift_imm(kind: u16, amount: u32, value: u32) -> (u32, Option<bool>) {
    if amount == 0 {
        match kind {
            0 => (value, None),
            1 => (0, Some(value >> 31 != 0)),
            _ => (((value as i32) >> 31) as u32, Some(value >> 31 != 0)),
        }
    } else {
        spec_shift(kind, amount, value)
    }
}

fn spec_shift(kind: u16, amount: u32, value: u32) -> (u32, Option<bool>) {
    let n = amount & 0xFF;
    match kind {
        0 => {
            if n == 0 {
                (value, None)
            } else if n < 32 {
                (value.wrapping_shl(n), Some((value >> (32 - n)) & 1 != 0))
            } else {
                (0, Some(n == 32 && value & 1 != 0))
            }
        }
        1 => {
            if n == 0 {
                (value, None)
            } else if n < 32 {
                (value >> n, Some((value >> (n - 1)) & 1 != 0))
            } else {
                (0, Some(n == 32 && value >> 31 != 0))
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

fn run(raw: u16, addr: u32, setup: impl FnOnce(&mut Cpu)) -> Cpu {
    let mut bus = Bus::new(cart());
    let instr = decode_thumb(addr, raw as u32).expect("Thumb decode");
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(addr);
    cpu.set_thumb(true);
    setup(&mut cpu);
    interpret::execute(&mut cpu, &mut bus, instr).expect("Thumb execute");
    cpu
}

fn nz(result: u32) -> (bool, bool) {
    (result & 0x8000_0000 != 0, result == 0)
}

#[test]
fn all_fifteen_register_alu_forms_match_documented_semantics() {
    for kind in 0..=15u16 {
        if kind == 13 {
            let raw = alu(kind, 1, 0);
            assert!(decode_thumb(0, raw as u32).is_err(), "MUL stays explicit");
            continue;
        }
        for carry_in in [false, true] {
            let rd_value = 0x8000_0005;
            let rs_value = 0x8000_0003;
            let raw = alu(kind, 3, 0);
            let cpu = run(raw, 0x0800_0000, |cpu| {
                cpu.set_reg(0, rd_value);
                cpu.set_reg(3, rs_value);
                cpu.set_nzcv(false, false, carry_in, true);
            });
            let logical = |result| {
                let (n, z) = nz(result);
                (result, (n, z, carry_in, true))
            };
            let (expected_result, expected_flags) = match kind {
                0 => logical(rd_value & rs_value),
                1 => logical(rd_value ^ rs_value),
                2..=4 | 7 => {
                    let shift_kind = match kind {
                        2 => 0,
                        3 => 1,
                        4 => 2,
                        _ => 3,
                    };
                    let (result, carry) = spec_shift(shift_kind, rs_value, rd_value);
                    let (n, z) = nz(result);
                    (result, (n, z, carry.unwrap_or(carry_in), true))
                }
                5 => {
                    let (r, c, v) = spec_add(rd_value, rs_value, carry_in);
                    let (n, z) = nz(r);
                    (r, (n, z, c, v))
                }
                6 => {
                    let (r, c, v) = spec_sub(rd_value, rs_value, carry_in);
                    let (n, z) = nz(r);
                    (r, (n, z, c, v))
                }
                8 => logical(rd_value & rs_value),
                9 => {
                    let (r, c, v) = spec_sub(0, rs_value, true);
                    let (n, z) = nz(r);
                    (r, (n, z, c, v))
                }
                10 => {
                    let (r, c, v) = spec_sub(rd_value, rs_value, true);
                    let (n, z) = nz(r);
                    (r, (n, z, c, v))
                }
                11 => {
                    let (r, c, v) = spec_add(rd_value, rs_value, false);
                    let (n, z) = nz(r);
                    (r, (n, z, c, v))
                }
                12 => logical(rd_value | rs_value),
                14 => logical(rd_value & !rs_value),
                15 => logical(!rs_value),
                _ => unreachable!(),
            };
            if matches!(kind, 8 | 10 | 11) {
                assert_eq!(cpu.reg(0), rd_value, "test opcode must not write Rd");
            } else {
                assert_eq!(cpu.reg(0), expected_result, "kind={kind} c={carry_in}");
            }
            assert_eq!(cpu.flags(), expected_flags, "kind={kind} c={carry_in}");
        }
    }
}

#[test]
fn shift_immediate_group_has_zero_specials_and_preserves_v() {
    for kind in 0..=2u16 {
        for amount in [0u16, 1, 15, 31] {
            for value in [0u32, 1, 0x8000_0000, 0x8000_0001, 0xFFFF_FFFF, 0x4000_0000] {
                let raw = shift_imm(kind, amount, 1, 0);
                let instr = decode_thumb(0, raw as u32).expect("shift immediate");
                let expected_kind = match kind {
                    0 => ShiftKind::Lsl,
                    1 => ShiftKind::Lsr,
                    _ => ShiftKind::Asr,
                };
                assert_eq!(
                    instr.op,
                    Op::Mov {
                        rd: 0,
                        src: Src::RegShift {
                            rm: 1,
                            shift: Shift::Imm {
                                kind: expected_kind,
                                amount: amount as u8
                            }
                        },
                        set_flags: true
                    }
                );
                let cpu = run(raw, 0x0800_0000, |cpu| {
                    cpu.set_reg(1, value);
                    cpu.set_nzcv(true, true, true, true);
                });
                let (result, carry) = spec_shift_imm(kind, amount as u32, value);
                let (n, z) = nz(result);
                assert_eq!(
                    cpu.reg(0),
                    result,
                    "kind={kind} amount={amount} value={value:#010X}"
                );
                assert_eq!(
                    cpu.flags(),
                    (n, z, carry.unwrap_or(true), true),
                    "kind={kind} amount={amount} value={value:#010X}"
                );
            }
        }
    }
}

#[test]
fn register_shift_amounts_use_the_low_eight_bits() {
    for amount in [0u32, 1, 31, 32, 33, 63, 64, 255, 256] {
        for kind in 2..=4u16 {
            let raw = alu(kind, 3, 0);
            let cpu = run(raw, 0x0800_0000, |cpu| {
                cpu.set_reg(0, 0x8000_0001);
                cpu.set_reg(3, amount);
                cpu.set_nzcv(false, false, true, true);
            });
            let (result, carry) = spec_shift(kind - 2, amount, 0x8000_0001);
            let (n, z) = nz(result);
            assert_eq!(cpu.reg(0), result, "kind={kind} amount={amount}");
            assert_eq!(
                cpu.flags(),
                (n, z, carry.unwrap_or(true), true),
                "kind={kind} amount={amount}"
            );
        }
    }
}

#[test]
fn high_register_operations_read_pc_plus_4_and_never_set_flags_for_add_mov() {
    let addr = 0x0800_0002;
    let cpu = run(hi(2, 0, 1, 7, 0), addr, |_| {});
    assert_eq!(cpu.reg(0), addr + 4, "MOV r0,PC");

    let cpu = run(hi(0, 0, 1, 7, 0), addr, |cpu| {
        cpu.set_reg(0, 0x10);
        cpu.set_nzcv(false, false, true, false);
    });
    assert_eq!(cpu.reg(0), 0x10 + addr + 4, "ADD r0,PC uses PC+4");
    assert_eq!(
        cpu.flags(),
        (false, false, true, false),
        "ADD does not set flags"
    );

    // Low/low MOV and CMP are accepted as this prototype's Design extension
    // (GBATEK THUMB.5 restricts these high-register fields); MOV never sets
    // flags here.
    let cpu = run(hi(2, 0, 0, 1, 0), addr, |cpu| {
        cpu.set_reg(1, 0x55);
        cpu.set_nzcv(true, false, true, true);
    });
    assert_eq!(cpu.reg(0), 0x55);
    assert_eq!(cpu.flags(), (true, false, true, true));

    let cpu = run(hi(1, 0, 0, 1, 0), addr, |cpu| {
        cpu.set_reg(0, 0x55);
        cpu.set_reg(1, 0x55);
        cpu.set_nzcv(false, false, false, true);
    });
    assert_eq!(cpu.flags(), (false, true, true, false), "CMP sets NZCV");

    // High ADD/MOV to PC are aligned and stay in Thumb state.
    let cpu = run(hi(2, 1, 0, 0, 7), addr, |cpu| {
        cpu.set_reg(0, 0x0800_0305);
    });
    assert_eq!(cpu.pc(), 0x0800_0304);
    assert!(cpu.thumb());

    let cpu = run(hi(0, 1, 0, 0, 7), addr, |cpu| {
        cpu.set_reg(0, 2);
    });
    assert_eq!(cpu.pc(), (addr + 4 + 2) & !1);
    assert!(cpu.thumb());
}

#[test]
fn low_low_add_is_reserved_and_blx_register_is_armv5() {
    let error = decode_thumb(0, 0x4400).expect_err("low/low ADD reserved");
    assert!(format!("{error}").contains("reserved"), "{error}");
    let error = decode_thumb(0, 0x4780).expect_err("BLX register");
    assert!(format!("{error}").contains("BLX"), "{error}");
}

#[test]
fn adr_pc_sp_and_stack_adjust_use_documented_bases() {
    let addr = 0x0800_0006;
    let base = (addr + 4) & !3;
    let cpu = run(0xA000, addr, |_| {});
    assert_eq!(cpu.reg(0), base, "ADR r0,#0");

    let cpu = run(0xA003, 0x0800_0002, |_| {});
    assert_eq!(cpu.reg(0), ((0x0800_0002 + 4) & !3) + 12, "ADR r0,#12");

    let cpu = run(0xA800 | 2, addr, |cpu| {
        cpu.set_reg(13, 0x0300_7F00);
        cpu.set_nzcv(true, true, true, true);
    });
    assert_eq!(cpu.reg(0), 0x0300_7F00 + 8, "ADD r0,SP,#8");
    assert_eq!(cpu.flags(), (true, true, true, true), "no flags");

    let cpu = run(0xB004, addr, |cpu| {
        cpu.set_reg(13, 0x0300_7F00);
    });
    assert_eq!(cpu.reg(13), 0x0300_7F10, "ADD SP,#16");
    let cpu = run(0xB084, addr, |cpu| {
        cpu.set_reg(13, 0x0300_7F10);
    });
    assert_eq!(cpu.reg(13), 0x0300_7F00, "SUB SP,#16");
}

#[test]
fn typed_ir_for_neg_and_high_register_forms_is_explicit() {
    assert_eq!(
        decode_thumb(0, alu(9, 3, 0) as u32).expect("NEG").op,
        Op::Alu {
            opcode: AluOpcode::Rsb,
            rd: Some(0),
            rn: 3,
            rhs: Src::Imm {
                value: 0,
                carry: None
            },
            set_flags: true
        }
    );
    assert_eq!(
        decode_thumb(0, hi(0, 0, 1, 2, 0) as u32)
            .expect("ADD r0,r10")
            .op,
        Op::Add {
            rd: 0,
            rn: 0,
            rhs: Src::Reg(10),
            set_flags: false
        }
    );
}
