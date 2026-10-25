use super::extra::{decode_arm_halfword, decode_arm_psr};
use crate::ir::{AluOpcode, Cond, DecodeResult, IrInstr, Op, Shift, ShiftKind, Src, unsupported};

/// Classification inside `bits 27-25 == 000/001`, which is shared by
/// data-processing, multiply/swap and extra halfword transfers. The low-nibble
/// tests apply only to register-form data processing: with bit 25 set, bits
/// 11-0 are the rotated immediate and must never be read as an opcode.
pub(super) fn decode_arm_data_or_extra(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
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
    // Halfword/signed transfers keep bit 7 set in the 1SH1 pattern, while valid
    // MRS/MSR register encodings have bits 11-4 zero. Classifying these before
    // the PSR shape prevents a negative pre-indexed STRH from becoming a
    // malformed MRS.
    if immediate == 0 && low_nibble & 0b1001 == 0b1001 && low_nibble & 0b0110 != 0 {
        return decode_arm_halfword(addr, raw, cond);
    }
    // PSR transfers occupy the S=0 test-operation slots (TST/TEQ/CMP/CMN).
    if (raw >> 23) & 0b11 == 0b10 && (raw >> 20) & 1 == 0 {
        return decode_arm_psr(addr, raw, cond);
    }
    decode_arm_data_processing(addr, raw, cond)
}

fn shift_kind(bits: u32) -> ShiftKind {
    match bits & 0x3 {
        0 => ShiftKind::Lsl,
        1 => ShiftKind::Lsr,
        2 => ShiftKind::Asr,
        _ => ShiftKind::Ror,
    }
}

fn decode_operand(addr: u32, raw: u32) -> crate::error::Result<Src> {
    if (raw >> 25) & 1 == 1 {
        let imm8 = raw & 0xFF;
        let rotation = ((raw >> 8) & 0xF) * 2;
        let value = imm8.rotate_right(rotation);
        let carry = if rotation == 0 {
            None
        } else {
            Some(value & 0x8000_0000 != 0)
        };
        return Ok(Src::Imm { value, carry });
    }
    let rm = (raw & 0xF) as u8;
    let register_shift = (raw >> 4) & 1 == 1;
    if !register_shift {
        let amount = ((raw >> 7) & 0x1F) as u8;
        let kind = shift_kind(raw >> 5);
        if amount == 0 && kind == ShiftKind::Lsl {
            return Ok(Src::Reg(rm));
        }
        return Ok(Src::RegShift {
            rm,
            shift: Shift::Imm { kind, amount },
        });
    }
    let rs = ((raw >> 8) & 0xF) as u8;
    if rs == 15 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM register-specified shift by PC (Rs=15) is unpredictable",
        ));
    }
    if (raw >> 7) & 1 != 0 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "reserved ARM register-shift encoding bit 7 is set",
        ));
    }
    Ok(Src::RegShift {
        rm,
        shift: Shift::Reg {
            kind: shift_kind(raw >> 5),
            rm: rs,
        },
    })
}

pub(super) fn decode_arm_data_processing(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    let opcode = (raw >> 21) & 0xF;
    let set_flags = (raw >> 20) & 1 == 1;
    let rn = ((raw >> 16) & 0xF) as u8;
    let rd = ((raw >> 12) & 0xF) as u8;
    let is_test = matches!(opcode, 8..=11);
    let is_move = matches!(opcode, 13 | 15);

    if is_test {
        // CMP/CMN/TST/TEQ always set flags; their S=0 encodings belong to the
        // MRS/MSR space and were handled before this function.
        if !set_flags {
            return Err(unsupported(
                addr,
                false,
                raw,
                "ARM test operation requires S=1",
            ));
        }
        if rd != 0 {
            return Err(unsupported(
                addr,
                false,
                raw,
                "ARM test operation requires its fixed Rd=0 encoding; the ARMv2 P-variant is not implemented",
            ));
        }
    }
    if is_move && rn != 0 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM MOV/MVN requires its fixed Rn=0 encoding",
        ));
    }
    if rd == 15 && set_flags && !is_test {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM S=1 with Rd=PC requires an SPSR restore, which is not implemented",
        ));
    }

    let rhs = decode_operand(addr, raw)?;
    let op = match opcode {
        0 => alu(AluOpcode::And, rd, rn, rhs, set_flags),
        1 => alu(AluOpcode::Eor, rd, rn, rhs, set_flags),
        2 => Op::Sub {
            rd,
            rn,
            rhs,
            set_flags,
        },
        3 => alu(AluOpcode::Rsb, rd, rn, rhs, set_flags),
        4 => Op::Add {
            rd,
            rn,
            rhs,
            set_flags,
        },
        5 => alu(AluOpcode::Adc, rd, rn, rhs, set_flags),
        6 => alu(AluOpcode::Sbc, rd, rn, rhs, set_flags),
        7 => alu(AluOpcode::Rsc, rd, rn, rhs, set_flags),
        8 => alu_test(AluOpcode::Tst, rn, rhs),
        9 => alu_test(AluOpcode::Teq, rn, rhs),
        10 => Op::Cmp { rn, rhs },
        11 => alu_test(AluOpcode::Cmn, rn, rhs),
        12 => alu(AluOpcode::Orr, rd, rn, rhs, set_flags),
        13 => Op::Mov {
            rd,
            src: rhs,
            set_flags,
        },
        14 => alu(AluOpcode::Bic, rd, rn, rhs, set_flags),
        15 => alu(AluOpcode::Mvn, rd, rn, rhs, set_flags),
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

fn alu(opcode: AluOpcode, rd: u8, rn: u8, rhs: Src, set_flags: bool) -> Op {
    Op::Alu {
        opcode,
        rd: Some(rd),
        rn,
        rhs,
        set_flags,
    }
}

fn alu_test(opcode: AluOpcode, rn: u8, rhs: Src) -> Op {
    Op::Alu {
        opcode,
        rd: None,
        rn,
        rhs,
        set_flags: true,
    }
}
