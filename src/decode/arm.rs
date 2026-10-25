use super::sign_extend;
use crate::ir::{Cond, DecodeResult, IrInstr, Op, Psr, PsrSrc, Src, unsupported};

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
    // halfword transfers. The low-nibble checks below only apply to that
    // register-form space: bit25 == 1 is a data-processing/PSR immediate, where
    // bits 11-0 are the rotated immediate and must never be read as an opcode.
    // Otherwise a CMP immediate such as 0xE35000B0 would be misclassified as
    // LDRH.
    let immediate = (raw >> 25) & 1;
    let low_nibble = (raw >> 4) & 0xF;
    if immediate == 0 && low_nibble == 0b1001 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM multiply/swap encodings are not implemented",
        ));
    }
    // Extra halfword transfers are recognised by their register-form low
    // nibble 1SH1. They must be classified before the PSR shape below: a
    // pre-indexed negative halfword transfer (P=1, U=0) also has bits 24-23 =
    // 10b and bit 20 = 0 for stores, which is exactly the coarse PSR predicate.
    // The two classes are mutually exclusive, not overlapping: valid MRS and
    // register-form MSR encodings have bits 11-4 zero, while this halfword
    // pattern keeps bit 7 set. Immediate-form MSR has bit 25 set and is
    // therefore already excluded by `immediate == 0` here. Multiply/swap was
    // rejected above because its low nibble 1001b has neither H nor S set.
    if immediate == 0 && low_nibble & 0b1001 == 0b1001 && low_nibble & 0b0110 != 0 {
        return decode_arm_halfword(addr, raw, cond);
    }
    // PSR transfers occupy the unused TEQ/TST/CMP/CMN-with-S=0 slots: bits
    // 27-23 are 00010b (or 00110b for immediate MSR) and bit 20 is zero.
    if (raw >> 23) & 0b11 == 0b10 && (raw >> 20) & 1 == 0 {
        return decode_arm_psr(addr, raw, cond);
    }
    decode_arm_data_processing(addr, raw, cond)
}

/// ARM PSR transfer. The source/destination/fixed fields are documented in
/// GBATEK `#arm-opcodes-psr-transfer-mrs-msr`. `f`/`s`/`x`/`c` select the four
/// documented field *bytes* (bits 31-24, 23-16, 15-8, 7-0); on the ARM7TDMI
/// only N/Z/C/V (bits 31-28 of the `f` byte) and the control byte have writable
/// state, so the byte selection is wider than the writable mask.
fn decode_arm_psr(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    let immediate = (raw >> 25) & 1;
    let psr = if (raw >> 22) & 1 == 1 {
        Psr::Spsr
    } else {
        Psr::Cpsr
    };
    let opcode = (raw >> 21) & 1;

    if opcode == 0 {
        // MRS{cond} Rd,Psr: register form only, fixed Rn field 1111, bits 11-0
        // unused and zero, Rd restricted to R0-R14.
        if immediate != 0 || (raw >> 16) & 0xF != 0xF || raw & 0x0FFF != 0 {
            return Err(unsupported(
                addr,
                false,
                raw,
                "malformed ARM MRS: it must be register-only with fixed Rn=1111 and zero low bits",
            ));
        }
        let rd = ((raw >> 12) & 0xF) as u8;
        if rd == 15 {
            return Err(unsupported(
                addr,
                false,
                raw,
                "ARM MRS to PC is not a valid encoding",
            ));
        }
        return Ok(IrInstr {
            addr,
            thumb: false,
            raw,
            cond,
            op: Op::Mrs { rd, psr },
        });
    }

    // MSR{cond} Psr{_fields},Op: fixed Rd field 1111; register form has bits
    // 11-4 unused and zero, immediate form uses bits 11-8 as imm8 ROR steps.
    if (raw >> 12) & 0xF != 0xF {
        return Err(unsupported(
            addr,
            false,
            raw,
            "malformed ARM MSR: bits 15-12 must be 1111b",
        ));
    }
    let fields = ((raw >> 16) & 0xF) as u8;
    // GBATEK documents that at least one field bit must be set; an empty mask
    // is unpredictable rather than a successful no-op. Reject it explicitly.
    if fields == 0 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM MSR with an empty field mask is unpredictable: at least one of f/s/x/c must be set",
        ));
    }
    let src = if immediate == 0 {
        if (raw >> 4) & 0xFF != 0 {
            return Err(unsupported(
                addr,
                false,
                raw,
                "malformed register-form ARM MSR: bits 11-4 must be zero",
            ));
        }
        let rm = (raw & 0xF) as u8;
        if rm == 15 {
            return Err(unsupported(
                addr,
                false,
                raw,
                "ARM MSR from PC is not implemented",
            ));
        }
        PsrSrc::Reg(rm)
    } else {
        let imm8 = raw & 0xFF;
        let rotation = ((raw >> 8) & 0xF) * 2;
        PsrSrc::Imm(imm8.rotate_right(rotation))
    };
    Ok(IrInstr {
        addr,
        thumb: false,
        raw,
        cond,
        op: Op::Msr { psr, fields, src },
    })
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
            "only ARM MOV/ADD/SUB/CMP are implemented (no logical/multiply family)",
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
