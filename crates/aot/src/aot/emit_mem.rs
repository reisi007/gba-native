use std::fmt::Write as _;

use gba_native::ir::{IrInstr, MemOffset, MemWidth, Op, Shift};

/// Emit direct Rust statements for the memory-transfer families. These are
/// address/value statements plus checked `Bus` calls; no decoder, interpreter
/// step or whole-IR execute wrapper is emitted.
pub(super) fn instruction_body(instruction: &IrInstr) -> String {
    match instruction.op {
        Op::MemSingle {
            load,
            width,
            signed,
            rd,
            rn,
            offset,
            add,
            pre_index,
            writeback,
            user_access: _,
        } => mem_single_body(
            instruction,
            load,
            width,
            signed,
            rd,
            rn,
            offset,
            add,
            pre_index,
            writeback,
        ),
        Op::BlockTransfer {
            load,
            rn,
            regs,
            pre_index,
            add,
            writeback,
        } => block_body(instruction, load, rn, regs, pre_index, add, writeback),
        Op::Push { regs, lr } => push_body(instruction, regs, lr),
        Op::Pop { regs, pc } => pop_body(instruction, regs, pc),
        Op::ThumbBlock { load, rn, regs } => thumb_block_body(instruction, load, rn, regs),
        _ => unreachable!("emit_mem received a non-memory operation"),
    }
}

#[allow(clippy::too_many_arguments)]
fn mem_single_body(
    instruction: &IrInstr,
    load: bool,
    width: MemWidth,
    signed: bool,
    rd: u8,
    rn: u8,
    offset: MemOffset,
    add: bool,
    pre_index: bool,
    writeback: bool,
) -> String {
    let mut out = String::new();
    writeln!(out, "    let base = {};", base_expression(instruction, rn)).unwrap();
    match offset {
        MemOffset::Imm(value) => {
            writeln!(out, "    let offset = 0x{value:08X}u32;").unwrap();
        }
        MemOffset::RegShift { rm, shift } => {
            writeln!(out, "    let offset = {};", shift_expression(rm, shift)).unwrap();
        }
    }
    let address = if pre_index {
        if add {
            "base.wrapping_add(offset)"
        } else {
            "base.wrapping_sub(offset)"
        }
    } else {
        "base"
    };
    writeln!(out, "    let address = {address};").unwrap();
    if !load {
        writeln!(
            out,
            "    let source = {};",
            source_expression(instruction, rd)
        )
        .unwrap();
    }
    match (load, width, signed) {
        (true, MemWidth::Word, false) => {
            writeln!(out, "    let word = bus.read32(address & !3)?;").unwrap();
            writeln!(out, "    let value = word.rotate_right((address & 3) * 8);").unwrap();
        }
        (true, MemWidth::Byte, false) => {
            writeln!(out, "    let value = bus.read8(address)? as u32;").unwrap();
        }
        (true, MemWidth::Byte, true) => {
            writeln!(
                out,
                "    let value = (bus.read8(address)? as i8 as i32) as u32;"
            )
            .unwrap();
        }
        (true, MemWidth::Half, false) => {
            writeln!(out, "    let half = bus.read16(address & !1)? as u32;").unwrap();
            writeln!(
                out,
                "    let value = if address & 1 != 0 {{ half.rotate_right(8) }} else {{ half }};"
            )
            .unwrap();
        }
        (true, MemWidth::Half, true) => {
            writeln!(
                out,
                "    let value = if address & 1 != 0 {{ (bus.read8(address)? as i8 as i32) as u32 }} else {{ (bus.read16(address & !1)? as i16 as i32) as u32 }};"
            )
            .unwrap();
        }
        (false, MemWidth::Byte, _) => {
            writeln!(out, "    bus.write8(address, source as u8)?;").unwrap();
        }
        (false, MemWidth::Half, _) => {
            writeln!(out, "    bus.write16(address & !1, source as u16)?;").unwrap();
        }
        (false, MemWidth::Word, _) => {
            writeln!(out, "    bus.write32(address & !3, source)?;").unwrap();
        }
        (true, MemWidth::Word, true) => unreachable!("signed word load is not decoded"),
    }
    if load {
        if rd == 15 {
            // ARMv4 word LDR PC force-aligns and keeps ARM state.
            writeln!(out, "    cpu.set_reg(15, value & !3);").unwrap();
        } else {
            writeln!(out, "    cpu.set_reg({rd}, value);").unwrap();
        }
    }
    if writeback {
        let value = if add {
            "base.wrapping_add(offset)"
        } else {
            "base.wrapping_sub(offset)"
        };
        writeln!(out, "    cpu.set_reg({rn}, {value});").unwrap();
    }
    out
}

fn block_body(
    instruction: &IrInstr,
    load: bool,
    rn: u8,
    regs: u16,
    pre_index: bool,
    add: bool,
    writeback: bool,
) -> String {
    let mut out = String::new();
    let count = regs.count_ones();
    let first = regs.trailing_zeros() as u8;
    writeln!(out, "    let base = cpu.reg({rn});").unwrap();
    writeln!(
        out,
        "    gba_native::memory::require_aligned_block_base(base, {})?;",
        instruction.thumb
    )
    .unwrap();
    writeln!(
        out,
        "    let start = gba_native::memory::block_start(base, {count}u32, {pre_index}, {add});"
    )
    .unwrap();
    writeln!(
        out,
        "    let new_base = gba_native::memory::block_written_back(base, {count}u32, {add});"
    )
    .unwrap();

    let mut index = 0u32;
    if load {
        for reg in 0..16u8 {
            if regs & (1 << reg) == 0 {
                continue;
            }
            let offset = index * 4;
            if reg == 15 {
                writeln!(
                    out,
                    "    let pc_value = bus.read32(start.wrapping_add(0x{offset:X}u32))?;"
                )
                .unwrap();
            } else {
                writeln!(
                    out,
                    "    let value_{reg} = bus.read32(start.wrapping_add(0x{offset:X}u32))?;"
                )
                .unwrap();
                writeln!(out, "    cpu.set_reg({reg}, value_{reg});").unwrap();
            }
            index += 1;
        }
        if regs & (1 << 15) != 0 {
            // ARMv4 LDM PC force-aligns and keeps ARM state.
            writeln!(out, "    cpu.set_reg(15, pc_value & !3);").unwrap();
        }
        if writeback && regs & (1 << rn) == 0 {
            writeln!(out, "    cpu.set_reg({rn}, new_base);").unwrap();
        }
    } else {
        for reg in 0..16u8 {
            if regs & (1 << reg) == 0 {
                continue;
            }
            let offset = index * 4;
            let value = if reg == 15 {
                format!("0x{:08X}u32", instruction.addr.wrapping_add(12))
            } else if reg == rn && writeback && rn != first {
                "new_base".to_string()
            } else {
                format!("cpu.reg({reg})")
            };
            writeln!(
                out,
                "    bus.write32(start.wrapping_add(0x{offset:X}u32), {value})?;"
            )
            .unwrap();
            index += 1;
        }
        if writeback {
            writeln!(out, "    cpu.set_reg({rn}, new_base);").unwrap();
        }
    }
    out
}

fn push_body(instruction: &IrInstr, regs: u8, lr: bool) -> String {
    let mut out = String::new();
    let count = regs.count_ones() + u32::from(lr);
    let bytes = count * 4;
    writeln!(out, "    let sp = cpu.reg(13);").unwrap();
    writeln!(
        out,
        "    gba_native::memory::require_aligned_block_base(sp, true)?;"
    )
    .unwrap();
    writeln!(out, "    let start = sp.wrapping_sub(0x{bytes:X}u32);").unwrap();
    let mut index = 0u32;
    for reg in 0..8u8 {
        if regs & (1 << reg) == 0 {
            continue;
        }
        let offset = index * 4;
        writeln!(
            out,
            "    bus.write32(start.wrapping_add(0x{offset:X}u32), cpu.reg({reg}))?;"
        )
        .unwrap();
        index += 1;
    }
    if lr {
        let offset = index * 4;
        writeln!(
            out,
            "    bus.write32(start.wrapping_add(0x{offset:X}u32), cpu.reg(14))?;"
        )
        .unwrap();
    }
    writeln!(out, "    cpu.set_reg(13, start);").unwrap();
    let _ = instruction;
    out
}

fn pop_body(instruction: &IrInstr, regs: u8, pc: bool) -> String {
    let mut out = String::new();
    let count = regs.count_ones() + u32::from(pc);
    let bytes = count * 4;
    writeln!(out, "    let sp = cpu.reg(13);").unwrap();
    writeln!(
        out,
        "    gba_native::memory::require_aligned_block_base(sp, true)?;"
    )
    .unwrap();
    let mut index = 0u32;
    for reg in 0..8u8 {
        if regs & (1 << reg) == 0 {
            continue;
        }
        let offset = index * 4;
        writeln!(
            out,
            "    let value_{reg} = bus.read32(sp.wrapping_add(0x{offset:X}u32))?;"
        )
        .unwrap();
        writeln!(out, "    cpu.set_reg({reg}, value_{reg});").unwrap();
        index += 1;
    }
    if pc {
        let offset = index * 4;
        writeln!(
            out,
            "    let pc_value = bus.read32(sp.wrapping_add(0x{offset:X}u32))?;"
        )
        .unwrap();
        // Thumb POP PC force-aligns and keeps Thumb state.
        writeln!(out, "    cpu.set_reg(15, pc_value & !1);").unwrap();
    }
    writeln!(out, "    cpu.set_reg(13, sp.wrapping_add(0x{bytes:X}u32));").unwrap();
    let _ = instruction;
    out
}

fn thumb_block_body(instruction: &IrInstr, load: bool, rn: u8, regs: u8) -> String {
    let mut out = String::new();
    let count = regs.count_ones();
    let bytes = count * 4;
    let first = regs.trailing_zeros() as u8;
    writeln!(out, "    let base = cpu.reg({rn});").unwrap();
    writeln!(
        out,
        "    gba_native::memory::require_aligned_block_base(base, true)?;"
    )
    .unwrap();
    writeln!(out, "    let new_base = base.wrapping_add(0x{bytes:X}u32);").unwrap();
    let mut index = 0u32;
    for reg in 0..8u8 {
        if regs & (1 << reg) == 0 {
            continue;
        }
        let offset = index * 4;
        if load {
            writeln!(
                out,
                "    let value_{reg} = bus.read32(base.wrapping_add(0x{offset:X}u32))?;"
            )
            .unwrap();
            writeln!(out, "    cpu.set_reg({reg}, value_{reg});").unwrap();
        } else {
            let value = if reg == rn && rn != first {
                "new_base".to_string()
            } else {
                format!("cpu.reg({reg})")
            };
            writeln!(
                out,
                "    bus.write32(base.wrapping_add(0x{offset:X}u32), {value})?;"
            )
            .unwrap();
        }
        index += 1;
    }
    if load {
        if regs & (1 << rn) == 0 {
            writeln!(out, "    cpu.set_reg({rn}, new_base);").unwrap();
        }
    } else {
        writeln!(out, "    cpu.set_reg({rn}, new_base);").unwrap();
    }
    let _ = instruction;
    out
}

fn base_expression(instruction: &IrInstr, rn: u8) -> String {
    if rn == 15 {
        format!(
            "0x{:08X}u32",
            instruction
                .addr
                .wrapping_add(if instruction.thumb { 4 } else { 8 })
        )
    } else {
        format!("cpu.reg({rn})")
    }
}

fn source_expression(instruction: &IrInstr, rd: u8) -> String {
    if rd == 15 {
        format!(
            "0x{:08X}u32",
            instruction
                .addr
                .wrapping_add(if instruction.thumb { 4 } else { 12 })
        )
    } else {
        format!("cpu.reg({rd})")
    }
}

fn shift_expression(rm: u8, shift: Shift) -> String {
    match shift {
        Shift::Imm { kind, amount } => format!(
            "gba_native::semantics::shift_immediate(gba_native::ir::ShiftKind::{}, {amount}, cpu.reg({rm}), cpu.cpsr() & gba_native::cpu::FLAG_C != 0).0",
            kind.rust_name()
        ),
        Shift::Reg { .. } => unreachable!("register-controlled memory offset"),
    }
}
