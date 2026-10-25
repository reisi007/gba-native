use crate::ir::{Cond, DecodeResult, IrInstr, Op, unsupported};

mod block;
mod data;
mod extra;
mod memory;

pub fn decode_arm(addr: u32, raw: u32) -> DecodeResult {
    let cond = Cond::from_bits(raw >> 28).ok_or_else(|| {
        unsupported(
            addr,
            false,
            raw,
            "condition field 1111b is reserved on ARMv4T (unconditional/newer space)",
        )
    })?;
    if raw & 0x0F00_0000 == 0x0F00_0000 {
        return Ok(IrInstr {
            addr,
            thumb: false,
            raw,
            cond,
            op: Op::Swi {
                comment: raw & 0x00FF_FFFF,
            },
        });
    }
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
        0b010 | 0b011 => memory::decode_arm_single_transfer(addr, raw, cond),
        0b100 => block::decode_arm_block(addr, raw, cond),
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
