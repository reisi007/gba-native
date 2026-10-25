use crate::ir::{Cond, DecodeResult, IrInstr, Op, unsupported};

/// ARM block data transfer (`LDM`/`STM`, IA/IB/DA/DB, optional writeback).
///
/// The S=1 user-bank mode and the ARMv4 empty-list quirk are outside this
/// bounded slice and are rejected explicitly, never treated as a successful
/// NOP. PC as base is not an implemented encoding (GBATEK: R0-R14). A PC entry
/// in the transfer list is supported: loads force-align and keep ARM state,
/// stores write the documented instruction address + 12.
pub(super) fn decode_arm_block(addr: u32, raw: u32, cond: Cond) -> DecodeResult {
    let pre_index = (raw >> 24) & 1 == 1;
    let add = (raw >> 23) & 1 == 1;
    let user_bank = (raw >> 22) & 1 == 1;
    let writeback = (raw >> 21) & 1 == 1;
    let load = (raw >> 20) & 1 == 1;
    let rn = ((raw >> 16) & 0xF) as u8;
    let regs = (raw & 0xFFFF) as u16;

    if user_bank {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM LDM/STM S=1 user-bank/SPSR transfer is not implemented",
        ));
    }
    if rn == 15 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM LDM/STM with PC as base register is not an implemented encoding",
        ));
    }
    if regs == 0 {
        return Err(unsupported(
            addr,
            false,
            raw,
            "ARM LDM/STM with an empty register list is not a NOP; the ARMv4 quirk is not implemented",
        ));
    }

    Ok(IrInstr {
        addr,
        thumb: false,
        raw,
        cond,
        op: Op::BlockTransfer {
            load,
            rn,
            regs,
            pre_index,
            add,
            writeback,
        },
    })
}
