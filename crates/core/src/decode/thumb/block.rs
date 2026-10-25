use crate::ir::{Cond, DecodeResult, IrInstr, Op, unsupported};

fn make(addr: u32, raw: u32, op: Op) -> IrInstr {
    IrInstr {
        addr,
        thumb: true,
        raw,
        cond: Cond::Al,
        op,
    }
}

/// THUMB.14 `PUSH {Rlist}{,LR}` / `POP {Rlist}{,PC}`.
///
/// An empty list (no low register and no LR/PC bit) is an ARMv4 quirk, not a
/// successful NOP, and is rejected explicitly. The stack is full-descending and
/// registers are written in increasing memory order.
pub(super) fn decode_push_pop(addr: u32, raw: u32) -> DecodeResult {
    if raw & 0x0600 != 0x0400 {
        return Err(unsupported(
            addr,
            true,
            raw,
            "unimplemented Thumb 1011b encoding (not PUSH/POP or ADD/SUB SP)",
        ));
    }
    let load = raw & 0x0800 != 0;
    let extra = raw & 0x0100 != 0;
    let regs = (raw & 0x00FF) as u8;
    if regs == 0 && !extra {
        return Err(unsupported(
            addr,
            true,
            raw,
            "Thumb PUSH/POP with an empty register list is not implemented (ARMv4 quirk)",
        ));
    }
    let op = if load {
        Op::Pop { regs, pc: extra }
    } else {
        Op::Push { regs, lr: extra }
    };
    Ok(make(addr, raw, op))
}

/// THUMB.15 `LDMIA Rb!,{Rlist}` / `STMIA Rb!,{Rlist}` for R0-R7.
///
/// The ARMv4 base-in-list special is documented for Thumb too: STM stores the
/// old base when the base is the first list entry, otherwise the new base; LDM
/// has no additional writeback and the loaded base value wins. Empty lists are
/// rejected explicitly.
pub(super) fn decode_ldm_stm(addr: u32, raw: u32) -> DecodeResult {
    let load = raw & 0x0800 != 0;
    let rn = ((raw >> 8) & 0x7) as u8;
    let regs = (raw & 0xFF) as u8;
    if regs == 0 {
        return Err(unsupported(
            addr,
            true,
            raw,
            "Thumb LDMIA/STMIA with an empty register list is not implemented (ARMv4 quirk)",
        ));
    }
    Ok(make(addr, raw, Op::ThumbBlock { load, rn, regs }))
}
