use crate::ir::{
    Cond, DecodeResult, IrInstr, MemOffset, MemWidth, Op, Shift, ShiftKind, unsupported,
};

fn shift_kind(bits: u32) -> ShiftKind {
    match bits & 0x3 {
        0 => ShiftKind::Lsl,
        1 => ShiftKind::Lsr,
        2 => ShiftKind::Asr,
        _ => ShiftKind::Ror,
    }
}

/// ARM single data transfer: word/byte `LDR`/`STR` with immediate or shifted
/// register offset, pre/post indexing, add/subtract and pre-index writeback.
///
/// The exact previous literal `LDR` IR shape is preserved for the
/// `P=1,W=0,B=0,L=1,Rn=R15,Rd!=R15` immediate form so existing demo/discovery
/// constant propagation and old tests are unchanged. All other valid forms use
/// the typed `MemSingle` operation.
pub(super) fn decode_arm_single_transfer(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    let register_offset = (raw >> 25) & 1;
    let p = (raw >> 24) & 1;
    let u = (raw >> 23) & 1;
    let b = (raw >> 22) & 1;
    let bit21 = (raw >> 21) & 1;
    let l = (raw >> 20) & 1;
    let rn = ((raw >> 16) & 0xF) as u8;
    let rd = ((raw >> 12) & 0xF) as u8;

    // Preserve the original literal-word shape exactly.
    if register_offset == 0 && p == 1 && bit21 == 0 && b == 0 && l == 1 && rn == 15 && rd != 15 {
        let base = addr.wrapping_add(8);
        let target = if u == 1 {
            base.wrapping_add(raw & 0xFFF)
        } else {
            base.wrapping_sub(raw & 0xFFF)
        };
        return Ok(IrInstr {
            addr,
            thumb: false,
            raw,
            cond,
            op: Op::LdrLiteral { rd, addr: target },
        });
    }

    // P=0 makes the value of bit 21 the T (force user) bit and enables writeback
    // unconditionally. The flat checked GBA bus has no MMU/privilege split, so T
    // is recorded but does not change CPU mode.
    let (pre_index, writeback, user_access) = if p == 1 {
        (true, bit21 == 1, false)
    } else {
        (false, true, bit21 == 1)
    };

    let offset = if register_offset == 1 {
        // Bit 4 is the reserved register-shift indicator and must be zero.
        if raw & 0x10 != 0 {
            return Err(unsupported(
                addr,
                false,
                raw,
                "ARM single-transfer register offset has reserved bit 4 set",
            ));
        }
        let rm = (raw & 0xF) as u8;
        if rm == 15 {
            return Err(unsupported(
                addr,
                false,
                raw,
                "ARM single-transfer offset register is PC, which is not an implemented encoding",
            ));
        }
        MemOffset::RegShift {
            rm,
            shift: Shift::Imm {
                kind: shift_kind(raw >> 5),
                amount: ((raw >> 7) & 0x1F) as u8,
            },
        }
    } else {
        MemOffset::Imm(raw & 0xFFF)
    };

    // PC as a base with writeback would update R15 and is not required by the
    // initial slice. Reject before any side effect instead of guessing.
    if rn == 15 && writeback {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM single transfer with PC as base and writeback is not implemented",
        ));
    }
    // The load destination aliasing the written-back base is an unpredictable
    // field combination in the research notes; keep it an explicit error.
    if l == 1 && writeback && rd == rn {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM load with Rd=Rn and writeback is not implemented (unpredictable)",
        ));
    }
    // Only a word load may target R15. Narrow loads into PC are rejected rather
    // than inventing a result. A word LDR PC keeps ARM state on ARMv4.
    if l == 1 && rd == 15 && b == 1 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM byte load into PC is not implemented",
        ));
    }

    let width = if b == 1 {
        MemWidth::Byte
    } else {
        MemWidth::Word
    };
    Ok(IrInstr {
        addr,
        thumb: false,
        raw,
        cond,
        op: Op::MemSingle {
            load: l == 1,
            width,
            signed: false,
            rd,
            rn,
            offset,
            add: u == 1,
            pre_index,
            writeback,
            user_access,
        },
    })
}
