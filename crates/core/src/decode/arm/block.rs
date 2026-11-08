use crate::ir::{Cond, DecodeResult, IrInstr, Op, unsupported};

/// ARM block data transfer (`LDM`/`STM`, IA/IB/DA/DB, optional writeback).
///
/// Empty lists retain an ARMv4 marker while acting as a single PC transfer. The
/// S=1 empty-list form is explicitly rejected. PC as base is not an implemented
/// encoding (GBATEK: R0-R14). A normal PC load force-aligns and keeps ARM
/// state; `LDM^` with PC restores CPSR from the current SPSR. PC stores write
/// the instruction address + 12.
pub(super) fn decode_arm_block(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    let pre_index = (raw >> 24) & 1 == 1;
    let add = (raw >> 23) & 1 == 1;
    let user_bank = (raw >> 22) & 1 == 1;
    let writeback = (raw >> 21) & 1 == 1;
    let load = (raw >> 20) & 1 == 1;
    let rn = ((raw >> 16) & 0xF) as u8;
    let regs = (raw & 0xFFFF) as u16;

    if rn == 15 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM LDM/STM with PC as base register is not an implemented encoding",
        ));
    }
    let empty_list = regs == 0;
    if user_bank && empty_list {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM S=1 empty-list transfer is unspecified",
        ));
    }
    if user_bank && writeback && !(load && regs & (1 << 15) != 0) {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM S=1 user-bank transfer with writeback is unsupported by the documented rule",
        ));
    }
    let regs = if empty_list { 1 << 15 } else { regs };

    Ok(IrInstr {
        addr,
        thumb: false,
        raw,
        cond,
        op: Op::BlockTransfer {
            load,
            rn,
            regs,
            empty_list,
            pre_index,
            add,
            writeback,
            user_bank,
        },
    })
}
