use crate::ir::{AluOpcode, DecodeResult, IrInstr, Op, Shift, ShiftKind, Src, unsupported};

fn shift_kind(bits: u32) -> ShiftKind {
    match bits & 0x3 {
        0 => ShiftKind::Lsl,
        1 => ShiftKind::Lsr,
        2 => ShiftKind::Asr,
        _ => ShiftKind::Ror,
    }
}

fn make(addr: u32, raw: u32, op: Op) -> IrInstr {
    IrInstr {
        addr,
        thumb: true,
        raw,
        cond: crate::ir::Cond::Al,
        op,
    }
}

/// THUMB.1 move-shifted-register: LSL/LSR/ASR by an immediate, always setting
/// N/Z/C (V preserved, and C preserved for LSL#0).
pub(super) fn decode_move_shifted(addr: u32, raw: u32) -> DecodeResult {
    let kind = shift_kind(raw >> 11);
    let amount = ((raw >> 6) & 0x1F) as u8;
    let rs = ((raw >> 3) & 0x7) as u8;
    let rd = (raw & 0x7) as u8;
    Ok(make(
        addr,
        raw,
        Op::Mov {
            rd,
            src: Src::RegShift {
                rm: rs,
                shift: Shift::Imm { kind, amount },
            },
            set_flags: true,
        },
    ))
}

fn alu(opcode: AluOpcode, rd: Option<u8>, rn: u8, rhs: Src) -> Op {
    Op::Alu {
        opcode,
        rd,
        rn,
        rhs,
        set_flags: true,
    }
}

/// THUMB.4 register ALU operations. Every form sets flags as documented; MUL is
/// deliberately still an explicit unsupported encoding.
pub(super) fn decode_register_alu(addr: u32, raw: u32) -> DecodeResult {
    let kind = (raw >> 6) & 0xF;
    let rs = ((raw >> 3) & 0x7) as u8;
    let rd = (raw & 0x7) as u8;
    let op = match kind {
        0x0 => alu(AluOpcode::And, Some(rd), rd, Src::Reg(rs)),
        0x1 => alu(AluOpcode::Eor, Some(rd), rd, Src::Reg(rs)),
        0x2..=0x4 | 0x7 => {
            let kind = match kind {
                0x2 => ShiftKind::Lsl,
                0x3 => ShiftKind::Lsr,
                0x4 => ShiftKind::Asr,
                _ => ShiftKind::Ror,
            };
            Op::Mov {
                rd,
                src: Src::RegShift {
                    rm: rd,
                    shift: Shift::Reg { kind, rm: rs },
                },
                set_flags: true,
            }
        }
        0x5 => alu(AluOpcode::Adc, Some(rd), rd, Src::Reg(rs)),
        0x6 => alu(AluOpcode::Sbc, Some(rd), rd, Src::Reg(rs)),
        0x8 => alu(AluOpcode::Tst, None, rd, Src::Reg(rs)),
        0x9 => alu(
            AluOpcode::Rsb,
            Some(rd),
            rs,
            Src::Imm {
                value: 0,
                carry: None,
            },
        ),
        0xA => Op::Cmp {
            rn: rd,
            rhs: Src::Reg(rs),
        },
        0xB => alu(AluOpcode::Cmn, None, rd, Src::Reg(rs)),
        0xC => alu(AluOpcode::Orr, Some(rd), rd, Src::Reg(rs)),
        0xD => {
            return Err(unsupported(
                addr,
                true,
                raw,
                "Thumb MUL is not implemented; the other fifteen ALU opcodes are",
            ));
        }
        0xE => alu(AluOpcode::Bic, Some(rd), rd, Src::Reg(rs)),
        0xF => alu(AluOpcode::Mvn, Some(rd), rd, Src::Reg(rs)),
        _ => unreachable!(),
    };
    Ok(make(addr, raw, op))
}

/// THUMB.5 hi-register operations and BX. Valid low/low MOV and CMP aliases are
/// accepted; low/low ADD remains the documented reserved encoding. High-register
/// ADD/MOV never update flags, CMP always does, and PC reads as address+4.
pub(super) fn decode_hi_register(addr: u32, raw: u32) -> DecodeResult {
    let kind = (raw >> 8) & 0x3;
    let rd = (((raw >> 7) & 1) << 3 | (raw & 0x7)) as u8;
    let rs = (((raw >> 6) & 1) << 3 | ((raw >> 3) & 0x7)) as u8;
    match kind {
        0x0 => {
            if raw & 0x00C0 == 0 {
                return Err(unsupported(
                    addr,
                    true,
                    raw,
                    "low/low Thumb high-register ADD is the reserved encoding",
                ));
            }
            Ok(make(
                addr,
                raw,
                Op::Add {
                    rd,
                    rn: rd,
                    rhs: Src::Reg(rs),
                    set_flags: false,
                },
            ))
        }
        0x1 => Ok(make(
            addr,
            raw,
            Op::Cmp {
                rn: rd,
                rhs: Src::Reg(rs),
            },
        )),
        0x2 => Ok(make(
            addr,
            raw,
            Op::Mov {
                rd,
                src: Src::Reg(rs),
                set_flags: false,
            },
        )),
        _ => {
            if raw & 0x0080 != 0 {
                return Err(unsupported(
                    addr,
                    true,
                    raw,
                    "Thumb BLX register (ARMv5) is not implemented",
                ));
            }
            Ok(make(addr, raw, Op::Bx { rm: rs }))
        }
    }
}
