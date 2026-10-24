use super::sign_extend;
use crate::ir::{Cond, DecodeResult, IrInstr, Op, Src, unsupported};

pub fn decode_arm(addr: u32, raw: u32) -> DecodeResult {
    let cond = Cond::from_bits(raw >> 28).ok_or_else(|| {
        unsupported(
            addr,
            false,
            raw,
            "unconditional/newer ARM encoding space is not implemented",
        )
    })?;
    if raw & 0x0FFF_FFF0 == 0x012F_FF10 {
        let rm = (raw & 0xF) as u8;
        if rm == 15 {
            return Err(unsupported(
                addr,
                false,
                raw,
                "ARM BX PC is not implemented",
            ));
        }
        return Ok(IrInstr {
            addr,
            thumb: false,
            raw,
            cond,
            op: Op::Bx { rm },
        });
    }
    let group = (raw >> 25) & 0x7;
    match group {
        0b000 | 0b001 => decode_arm_data_or_extra(addr, raw, cond),
        0b010 | 0b011 => decode_arm_single_transfer(addr, raw, cond),
        0b100 => Err(unsupported(
            addr,
            false,
            raw,
            "ARM block transfers (LDM/STM) are not implemented",
        )),
        0b101 => decode_arm_branch(addr, raw, cond),
        0b110 => Err(unsupported(
            addr,
            false,
            raw,
            "ARM coprocessor transfers are not implemented",
        )),
        _ => Err(unsupported(
            addr,
            false,
            raw,
            "ARM coprocessor/SWI encoding space is not implemented",
        )),
    }
}

fn decode_arm_data_or_extra(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    // Bits 27-25 == 000 is shared by data-processing, multiply/swap and extra
    // halfword transfers. The extra/multiply classification below only applies
    // to that register-form space: bit25 == 1 is a data-processing immediate,
    // where bits 11-0 are the rotated immediate and must never be read as a
    // low opcode nibble. Otherwise a CMP immediate such as 0xE35000B0 would be
    // misclassified as LDRH.
    if (raw >> 25) & 1 == 0 {
        let low_nibble = (raw >> 4) & 0xF;
        if low_nibble == 0b1001 {
            return Err(unsupported(
                addr,
                false,
                raw,
                "ARM multiply/swap encodings are not implemented",
            ));
        }
        if low_nibble & 0b1001 == 0b1001 && low_nibble & 0b0110 != 0 {
            return decode_arm_halfword(addr, raw, cond);
        }
    }
    decode_arm_data_processing(addr, raw, cond)
}

fn decode_arm_halfword(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    let p = (raw >> 24) & 1;
    let u = (raw >> 23) & 1;
    let immediate = (raw >> 22) & 1;
    let w = (raw >> 21) & 1;
    let l = (raw >> 20) & 1;
    let rn = ((raw >> 16) & 0xF) as u8;
    let rd = ((raw >> 12) & 0xF) as u8;
    let s = (raw >> 6) & 1;
    let h = (raw >> 5) & 1;
    let offset_magnitude = (((raw >> 8) & 0xF) << 4) | (raw & 0xF);

    if p != 1 || immediate != 1 || w != 0 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "only pre-indexed immediate halfword transfers are implemented",
        ));
    }
    if s != 0 || h != 1 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "signed halfword/byte transfers are not implemented",
        ));
    }
    if rd == 15 || rn == 15 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "PC-relative halfword transfers are not implemented",
        ));
    }
    let offset = if u == 1 {
        offset_magnitude as i32
    } else {
        -(offset_magnitude as i32)
    };
    let op = if l == 1 {
        Op::LdrHalf { rd, rn, offset }
    } else {
        Op::StrHalf { rd, rn, offset }
    };
    Ok(IrInstr {
        addr,
        thumb: false,
        raw,
        cond,
        op,
    })
}

fn decode_arm_data_processing(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    let immediate = (raw >> 25) & 1;
    let opcode = (raw >> 21) & 0xF;
    let set_flags = (raw >> 20) & 1 == 1;
    let rn = ((raw >> 16) & 0xF) as u8;
    let rd = ((raw >> 12) & 0xF) as u8;

    if !matches!(opcode, 2 | 4 | 10 | 13) {
        return Err(unsupported(
            addr,
            false,
            raw,
            "only ARM MOV/ADD/SUB/CMP are implemented (no logical/multiply/PSR family)",
        ));
    }
    if opcode != 10 && rd == 15 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "writes to PC are not implemented as ALU operations",
        ));
    }
    if opcode == 10 && (!set_flags || rd != 0) {
        return Err(unsupported(
            addr,
            false,
            raw,
            "CMP requires S=1 and its fixed Rd=0 encoding",
        ));
    }
    if opcode == 13 && rn != 0 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "MOV requires its fixed Rn=0 encoding",
        ));
    }
    if opcode != 13 && rn == 15 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "PC register operands are not implemented",
        ));
    }

    let src = if immediate == 1 {
        let imm8 = raw & 0xFF;
        let rotation = ((raw >> 8) & 0xF) * 2;
        let value = imm8.rotate_right(rotation);
        let carry = if rotation == 0 {
            None
        } else {
            Some(value & 0x8000_0000 != 0)
        };
        Src::Imm { value, carry }
    } else {
        if (raw >> 4) & 0xFF != 0 {
            return Err(unsupported(
                addr,
                false,
                raw,
                "shifted register operands are not implemented",
            ));
        }
        let rm = (raw & 0xF) as u8;
        if rm == 15 {
            return Err(unsupported(
                addr,
                false,
                raw,
                "PC register operands are not implemented",
            ));
        }
        Src::Reg(rm)
    };

    let op = match opcode {
        13 => Op::Mov { rd, src, set_flags },
        4 => Op::Add {
            rd,
            rn,
            rhs: src,
            set_flags,
        },
        2 => Op::Sub {
            rd,
            rn,
            rhs: src,
            set_flags,
        },
        10 => Op::Cmp { rn, rhs: src },
        _ => unreachable!(),
    };
    Ok(IrInstr {
        addr,
        thumb: false,
        raw,
        cond,
        op,
    })
}

fn decode_arm_single_transfer(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    let register_offset = (raw >> 25) & 1;
    if register_offset == 1 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "register-offset single transfers are not implemented",
        ));
    }
    let p = (raw >> 24) & 1;
    let u = (raw >> 23) & 1;
    let b = (raw >> 22) & 1;
    let w = (raw >> 21) & 1;
    let l = (raw >> 20) & 1;
    let rn = (raw >> 16) & 0xF;
    let rd = ((raw >> 12) & 0xF) as u8;
    let offset = raw & 0xFFF;

    if p != 1 || w != 0 || b != 0 || l != 1 || rn != 15 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "only ARM LDR literal is implemented",
        ));
    }
    if rd == 15 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "LDR to PC is not implemented",
        ));
    }
    let base = addr.wrapping_add(8);
    let target = if u == 1 {
        base.wrapping_add(offset)
    } else {
        base.wrapping_sub(offset)
    };
    Ok(IrInstr {
        addr,
        thumb: false,
        raw,
        cond,
        op: Op::LdrLiteral { rd, addr: target },
    })
}

fn decode_arm_branch(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    if (raw >> 24) & 1 == 1 {
        return Err(unsupported(addr, false, raw, "ARM BL is not implemented"));
    }
    let offset = sign_extend(raw & 0x00FF_FFFF, 24) << 2;
    let target = addr.wrapping_add(8).wrapping_add(offset as u32) & !3;
    Ok(IrInstr {
        addr,
        thumb: false,
        raw,
        cond,
        op: Op::B { target },
    })
}
