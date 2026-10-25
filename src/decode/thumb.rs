use super::sign_extend;
use crate::ir::{Cond, DecodeResult, IrInstr, Op, Src, unsupported};

mod alu;

pub fn decode_thumb(addr: u32, raw: u32) -> DecodeResult {
    let raw = raw & 0xFFFF;
    let cond = Cond::Al;
    let make = |op| IrInstr {
        addr,
        thumb: true,
        raw,
        cond,
        op,
    };
    let unsupported_thumb = |reason| unsupported(addr, true, raw, reason);

    if raw & 0xF800 == 0x1800 {
        let kind = (raw >> 9) & 0x3;
        let rs = ((raw >> 3) & 0x7) as u8;
        let rd = (raw & 0x7) as u8;
        let rhs = if kind >= 2 {
            Src::Imm {
                value: (raw >> 6) & 0x7,
                carry: None,
            }
        } else {
            Src::Reg(((raw >> 6) & 0x7) as u8)
        };
        let op = if kind & 1 == 0 {
            Op::Add {
                rd,
                rn: rs,
                rhs,
                set_flags: true,
            }
        } else {
            Op::Sub {
                rd,
                rn: rs,
                rhs,
                set_flags: true,
            }
        };
        return Ok(make(op));
    }

    if raw & 0xE000 == 0x0000 {
        return alu::decode_move_shifted(addr, raw);
    }

    if raw & 0xE000 == 0x2000 {
        let kind = (raw >> 11) & 0x3;
        let rd = ((raw >> 8) & 0x7) as u8;
        let imm = raw & 0xFF;
        let src = Src::Imm {
            value: imm,
            carry: None,
        };
        let op = match kind {
            0 => Op::Mov {
                rd,
                src,
                set_flags: true,
            },
            1 => Op::Cmp { rn: rd, rhs: src },
            2 => Op::Add {
                rd,
                rn: rd,
                rhs: src,
                set_flags: true,
            },
            3 => Op::Sub {
                rd,
                rn: rd,
                rhs: src,
                set_flags: true,
            },
            _ => unreachable!(),
        };
        return Ok(make(op));
    }

    if raw & 0xFC00 == 0x4000 {
        return alu::decode_register_alu(addr, raw);
    }

    if raw & 0xFC00 == 0x4400 {
        return alu::decode_hi_register(addr, raw);
    }

    if raw & 0xF800 == 0x4800 {
        let rd = ((raw >> 8) & 0x7) as u8;
        let base = addr.wrapping_add(4) & !3;
        let target = base.wrapping_add((raw & 0xFF) * 4);
        return Ok(make(Op::LdrLiteral { rd, addr: target }));
    }

    if raw & 0xF000 == 0x5000 {
        return Err(unsupported_thumb(
            "Thumb register-offset word transfers are not implemented",
        ));
    }
    if raw & 0xE000 == 0x6000 {
        return Err(unsupported_thumb(
            "Thumb immediate-offset word transfers are not implemented",
        ));
    }

    if raw & 0xF000 == 0x8000 {
        let load = (raw >> 11) & 1 == 1;
        let offset = (((raw >> 6) & 0x1F) * 2) as i32;
        let rb = ((raw >> 3) & 0x7) as u8;
        let rd = (raw & 0x7) as u8;
        let op = if load {
            Op::LdrHalf { rd, rn: rb, offset }
        } else {
            Op::StrHalf { rd, rn: rb, offset }
        };
        return Ok(make(op));
    }

    if raw & 0xF000 == 0x9000 {
        return Err(unsupported_thumb(
            "Thumb SP-relative transfers are not implemented",
        ));
    }

    if raw & 0xF000 == 0xA000 {
        let rd = ((raw >> 8) & 0x7) as u8;
        let offset = (raw & 0xFF) * 4;
        if raw & 0x0800 == 0 {
            // ADR: address generation uses the word-aligned PC+4 value.
            let value = (addr.wrapping_add(4) & !3).wrapping_add(offset);
            return Ok(make(Op::Mov {
                rd,
                src: Src::Imm { value, carry: None },
                set_flags: false,
            }));
        }
        return Ok(make(Op::Add {
            rd,
            rn: 13,
            rhs: Src::Imm {
                value: offset,
                carry: None,
            },
            set_flags: false,
        }));
    }

    if raw & 0xFF00 == 0xB000 {
        let subtract = raw & 0x0080 != 0;
        let offset = (raw & 0x7F) * 4;
        let rhs = Src::Imm {
            value: offset,
            carry: None,
        };
        let op = if subtract {
            Op::Sub {
                rd: 13,
                rn: 13,
                rhs,
                set_flags: false,
            }
        } else {
            Op::Add {
                rd: 13,
                rn: 13,
                rhs,
                set_flags: false,
            }
        };
        return Ok(make(op));
    }

    if raw & 0xF000 == 0xB000 {
        return Err(unsupported_thumb("Thumb PUSH/POP are not implemented"));
    }
    if raw & 0xF000 == 0xC000 {
        return Err(unsupported_thumb("Thumb LDM/STM are not implemented"));
    }

    if raw & 0xF000 == 0xD000 {
        let branch_cond = (raw >> 8) & 0xF;
        if branch_cond == 0xE || branch_cond == 0xF {
            return Err(unsupported_thumb(
                "Thumb SWI/undefined conditional-branch encoding is not implemented",
            ));
        }
        let cond = Cond::from_bits(branch_cond).unwrap_or(Cond::Al);
        let offset = sign_extend(raw & 0xFF, 8) << 1;
        let target = addr.wrapping_add(4).wrapping_add(offset as u32) & !1;
        return Ok(IrInstr {
            addr,
            thumb: true,
            raw,
            cond,
            op: Op::B { target },
        });
    }

    if raw & 0xF800 == 0xE000 {
        let offset = sign_extend(raw & 0x7FF, 11) << 1;
        let target = addr.wrapping_add(4).wrapping_add(offset as u32) & !1;
        return Ok(make(Op::B { target }));
    }

    if raw & 0xF800 == 0xE800 {
        return Err(unsupported_thumb(
            "Thumb BLX second half (ARMv5) is not implemented",
        ));
    }

    if raw & 0xF800 == 0xF000 {
        let high = sign_extend(raw & 0x7FF, 11) << 12;
        let lr = addr.wrapping_add(4).wrapping_add(high as u32);
        return Ok(make(Op::BlHigh { lr }));
    }

    if raw & 0xF800 == 0xF800 {
        return Ok(make(Op::BlLow {
            offset: (raw & 0x7FF) << 1,
        }));
    }

    Err(unsupported_thumb(
        "unimplemented Thumb instruction encoding",
    ))
}
