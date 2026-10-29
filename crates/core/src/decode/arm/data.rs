use super::extra::{decode_arm_halfword, decode_arm_psr};
use crate::ir::{AluOpcode, Cond, DecodeResult, IrInstr, Op, Shift, ShiftKind, Src, unsupported};

/// Classification inside `bits 27-25 == 000/001`, which is shared by
/// data-processing, multiply/swap and extra halfword transfers. The low-nibble
/// tests apply only to register-form data processing: with bit 25 set, bits
/// 11-0 are the rotated immediate and must never be read as an opcode.
pub(super) fn decode_arm_data_or_extra(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    let immediate = (raw >> 25) & 1;
    let low_nibble = (raw >> 4) & 0xF;
    if immediate == 0 && raw & 0x0FB0_00F0 == 0x0100_0090 {
        return decode_arm_swap(addr, raw, cond);
    }
    if immediate == 0 && low_nibble == 0b1001 {
        return decode_arm_multiply(addr, raw, cond);
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

/// ARMv4T `SWP`/`SWPB`. DDI0100E §4.1.51–52 makes `Rd=Rm` legal, but
/// `R15` operands and `Rn` aliases with either `Rd` or `Rm` UNPREDICTABLE.
fn decode_arm_swap(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    let rn = ((raw >> 16) & 0xF) as u8;
    let rd = ((raw >> 12) & 0xF) as u8;
    let rm = (raw & 0xF) as u8;
    if raw & 0x0000_0F00 != 0 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM SWP/SWPB requires its fixed SBZ bits [11:8] to be zero",
        ));
    }
    if rn == 15 || rd == 15 || rm == 15 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM SWP/SWPB with R15 is unpredictable",
        ));
    }
    if rn == rd || rn == rm {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM SWP/SWPB with Rn aliased to Rd or Rm is unpredictable",
        ));
    }
    Ok(IrInstr {
        addr,
        thumb: false,
        raw,
        cond,
        op: Op::Swap {
            rd,
            rm,
            rn,
            byte: raw & (1 << 22) != 0,
        },
    })
}

/// ARM `MUL{cond}{S}` (bits 24-21 = 0000) and `MLA{cond}{S}` (0001). Every
/// other opcode in this encoding space (UMAAL, the long multiplies and the
/// halfword multiplies) stays an explicit unsupported encoding. Documented
/// restrictions from GBATEK `#arm-opcodes-multiply-and-multiply-accumulate-mul-mla`:
/// R15 may not appear and MUL's unused `Rn` field must be zero. The documented
/// unpredictable `MUL`/`MLA Rd=Rm` aliases use an explicit prototype design
/// extension.
fn decode_arm_multiply(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    let opcode = (raw >> 21) & 0xF;
    let set_flags = (raw >> 20) & 1 == 1;
    let rd = ((raw >> 16) & 0xF) as u8;
    let rn = ((raw >> 12) & 0xF) as u8;
    let rs = ((raw >> 8) & 0xF) as u8;
    let rm = (raw & 0xF) as u8;
    if !matches!(opcode, 0b0000 | 0b0001) {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM multiply/swap, multiply-long or halfword-multiply encodings are not implemented",
        ));
    }
    if rd == 15 || rn == 15 || rs == 15 || rm == 15 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM multiply with R15 as an operand is not a valid encoding",
        ));
    }
    if opcode == 0b0000 && rn != 0 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM MUL has a nonzero unused Rn field",
        ));
    }
    Ok(IrInstr {
        addr,
        thumb: false,
        raw,
        cond,
        op: Op::Mul {
            rd,
            rm,
            rs,
            accumulate: if opcode == 0b0001 { Some(rn) } else { None },
            set_flags,
        },
    })
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
    // `S=1` with `Rd=R15` is a documented exception return (GBATEK
    // `#arm-opcodes-data-processing-alu`): the CPSR is restored from the current
    // mode's SPSR and the computed result becomes the PC. Test operations have
    // a fixed Rd=0 and never reach this form, so it stays an ordinary typed
    // data-processing instruction here; whether an SPSR exists is a runtime
    // question (`Cpu::return_from_exception`).
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
