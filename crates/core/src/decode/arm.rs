use crate::ir::{Cond, DecodeResult, IrInstr, Op, unsupported};

mod data;
mod extra;

pub fn decode_arm(addr: u32, raw: u32) -> DecodeResult {
    let cond = Cond::from_bits(raw >> 28).ok_or_else(|| {
        unsupported(
            addr,
            false,
            raw,
            "condition field 1111b is reserved on ARMv4T (unconditional/newer space)",
        )
    })?;
    // BX/BXJ/BLX register occupy a fixed bits 27-8 pattern with the low byte as
    // opcode+Rn. Check it before the general opcode groups so BLX does not slip
    // into the register-shifted ALU space.
    if raw & 0x0FFF_FF00 == 0x012F_FF00 {
        let rm = (raw & 0xF) as u8;
        return match (raw >> 4) & 0xF {
            0b0001 => Ok(IrInstr {
                addr,
                thumb: false,
                raw,
                cond,
                op: Op::Bx { rm },
            }),
            0b0010 => Err(unsupported(
                addr,
                false,
                raw,
                "ARMv5 BXJ is not implemented",
            )),
            0b0011 => Err(unsupported(
                addr,
                false,
                raw,
                "ARMv5 BLX register is not implemented",
            )),
            _ => Err(unsupported(
                addr,
                false,
                raw,
                "reserved ARM branch-and-exchange encoding",
            )),
        };
    }
    match (raw >> 25) & 0x7 {
        0b000 | 0b001 => data::decode_arm_data_or_extra(addr, raw, cond),
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
    let link = (raw >> 24) & 1 == 1;
    let offset = super::sign_extend(raw & 0x00FF_FFFF, 24) << 2;
    let target = addr.wrapping_add(8).wrapping_add(offset as u32) & !3;
    let op = if link {
        Op::Bl { target }
    } else {
        Op::B { target }
    };
    Ok(IrInstr {
        addr,
        thumb: false,
        raw,
        cond,
        op,
    })
}
