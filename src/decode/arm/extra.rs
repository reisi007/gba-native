use crate::ir::{Cond, DecodeResult, IrInstr, Op, Psr, PsrSrc, unsupported};

/// ARM PSR transfer. The source/destination/fixed fields are documented in
/// GBATEK `#arm-opcodes-psr-transfer-mrs-msr`. `f`/`s`/`x`/`c` select the four
/// documented field *bytes* (bits 31-24, 23-16, 15-8, 7-0); on the ARM7TDMI
/// only N/Z/C/V (bits 31-28 of the `f` byte) and the control byte have writable
/// state, so the byte selection is wider than the writable mask.
pub(super) fn decode_arm_psr(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
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

pub(super) fn decode_arm_halfword(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
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
