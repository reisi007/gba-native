use crate::ir::{Cond, DecodeResult, IrInstr, MemOffset, MemWidth, Op, Shift, ShiftKind};

fn make(addr: u32, raw: u32, op: Op) -> IrInstr {
    IrInstr {
        addr,
        thumb: true,
        raw,
        cond: Cond::Al,
        op,
    }
}

fn unshifted(rm: u8) -> MemOffset {
    MemOffset::RegShift {
        rm,
        shift: Shift::Imm {
            kind: ShiftKind::Lsl,
            amount: 0,
        },
    }
}

/// THUMB.7 (`bit 9 = 0`): word/byte register-offset transfers.
/// THUMB.8 (`bit 9 = 1`): halfword and sign-extended register-offset transfers.
pub(super) fn decode_register_offset(addr: u32, raw: u32) -> DecodeResult {
    let opcode = (raw >> 10) & 0x3;
    let ro = ((raw >> 6) & 0x7) as u8;
    let rb = ((raw >> 3) & 0x7) as u8;
    let rd = (raw & 0x7) as u8;
    let offset = unshifted(ro);

    let (load, width, signed) = if raw & 0x0200 == 0 {
        match opcode {
            0 => (false, MemWidth::Word, false),
            1 => (false, MemWidth::Byte, false),
            2 => (true, MemWidth::Word, false),
            3 => (true, MemWidth::Byte, false),
            _ => unreachable!(),
        }
    } else {
        match opcode {
            0 => (false, MemWidth::Half, false),
            1 => (true, MemWidth::Byte, true),
            2 => (true, MemWidth::Half, false),
            3 => (true, MemWidth::Half, true),
            _ => unreachable!(),
        }
    };

    Ok(make(
        addr,
        raw,
        Op::MemSingle {
            load,
            width,
            signed,
            rd,
            rn: rb,
            offset,
            add: true,
            pre_index: true,
            writeback: false,
            user_access: false,
        },
    ))
}

/// THUMB.9: word/byte immediate-offset transfers. The 5-bit immediate is a byte
/// offset for byte transfers and scaled by four for word transfers.
pub(super) fn decode_immediate(addr: u32, raw: u32) -> DecodeResult {
    let opcode = (raw >> 11) & 0x3;
    let imm5 = (raw >> 6) & 0x1F;
    let rb = ((raw >> 3) & 0x7) as u8;
    let rd = (raw & 0x7) as u8;
    let (load, width) = match opcode {
        0 => (false, MemWidth::Word),
        1 => (true, MemWidth::Word),
        2 => (false, MemWidth::Byte),
        3 => (true, MemWidth::Byte),
        _ => unreachable!(),
    };
    let offset = if width == MemWidth::Word {
        imm5 * 4
    } else {
        imm5
    };
    Ok(make(
        addr,
        raw,
        Op::MemSingle {
            load,
            width,
            signed: false,
            rd,
            rn: rb,
            offset: MemOffset::Imm(offset),
            add: true,
            pre_index: true,
            writeback: false,
            user_access: false,
        },
    ))
}

/// THUMB.11: word transfer relative to SP. No writeback and no flags.
pub(super) fn decode_sp_relative(addr: u32, raw: u32) -> DecodeResult {
    let load = raw & 0x0800 != 0;
    let rd = ((raw >> 8) & 0x7) as u8;
    let offset = (raw & 0xFF) * 4;
    Ok(make(
        addr,
        raw,
        Op::MemSingle {
            load,
            width: MemWidth::Word,
            signed: false,
            rd,
            rn: 13,
            offset: MemOffset::Imm(offset),
            add: true,
            pre_index: true,
            writeback: false,
            user_access: false,
        },
    ))
}
