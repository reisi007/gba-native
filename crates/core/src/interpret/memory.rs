//! Reference execution helpers for single, block and stack transfers.

use crate::bus::Bus;
use crate::cpu::Cpu;
use crate::error::Result;
use crate::ir::{IrInstr, MemOffset, MemWidth};
use crate::memory as addr;

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_mem_single(
    cpu: &mut Cpu,
    bus: &mut Bus,
    instr: IrInstr,
    load: bool,
    width: MemWidth,
    signed: bool,
    rd: u8,
    rn: u8,
    offset: MemOffset,
    add: bool,
    pre_index: bool,
    writeback: bool,
) -> Result<()> {
    let base = addr::base_value(cpu, instr.addr, instr.thumb, rn);
    let offset = addr::offset_value(cpu, offset);
    let address = addr::accessed(base, offset, add, pre_index);

    if load {
        let value = load_value(bus, width, signed, address)?;
        if rd == 15 {
            // ARMv4 word LDR PC keeps ARM state and force-aligns.
            cpu.set_pc(value & !3);
        } else {
            cpu.set_reg(rd, value);
            cpu.set_pc(instr.next_addr());
        }
        // Writeback is committed only after the checked access succeeded.
        if writeback {
            cpu.set_reg(rn, addr::written_back(base, offset, add));
        }
    } else {
        let value = addr::stored_value(cpu, instr.addr, instr.thumb, rd);
        store_value(bus, width, address, value)?;
        if writeback {
            cpu.set_reg(rn, addr::written_back(base, offset, add));
        }
        cpu.set_pc(instr.next_addr());
    }
    Ok(())
}

fn load_value(bus: &Bus, width: MemWidth, signed: bool, address: u32) -> Result<u32> {
    match (width, signed) {
        (MemWidth::Byte, false) => Ok(u32::from(bus.read8(address)?)),
        (MemWidth::Byte, true) => Ok(addr::signed_byte(bus.read8(address)?)),
        (MemWidth::Half, false) => Ok(addr::half_load(bus.read16(address & !1)?, address)),
        // GBATEK ARM7: LDRSH from an odd address is a signed byte at that exact
        // address, not a rotated halfword.
        (MemWidth::Half, true) => {
            if address & 1 != 0 {
                Ok(addr::signed_byte(bus.read8(address)?))
            } else {
                Ok(addr::signed_half(bus.read16(address & !1)?))
            }
        }
        (MemWidth::Word, false) => Ok(addr::word_load(bus.read32(address & !3)?, address)),
        (MemWidth::Word, true) => unreachable!("signed word load is not decoded"),
    }
}

fn store_value(bus: &mut Bus, width: MemWidth, address: u32, value: u32) -> Result<()> {
    match width {
        MemWidth::Byte => bus.write8(address, value as u8),
        MemWidth::Half => bus.write16(address & !1, value as u16),
        MemWidth::Word => bus.write32(address & !3, value),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_block(
    cpu: &mut Cpu,
    bus: &mut Bus,
    instr: IrInstr,
    load: bool,
    rn: u8,
    regs: u16,
    pre_index: bool,
    add: bool,
    writeback: bool,
) -> Result<()> {
    let base = cpu.reg(rn);
    addr::require_aligned_block_base(base, instr.thumb)?;
    let count = addr::register_count(regs);
    let start = addr::block_start(base, count, pre_index, add);
    let new_base = addr::block_written_back(base, count, add);
    let first = addr::first_register(regs);
    let mut index = 0u32;

    if load {
        let mut pc_value = None;
        for reg in 0..16u8 {
            if regs & (1 << reg) == 0 {
                continue;
            }
            let address = start.wrapping_add(index * 4);
            let value = bus.read32(address)?;
            if reg == 15 {
                pc_value = Some(value);
            } else {
                cpu.set_reg(reg, value);
            }
            index += 1;
        }
        if let Some(value) = pc_value {
            // ARMv4 LDM PC keeps ARM state and force-aligns.
            cpu.set_pc(value & !3);
        } else {
            cpu.set_pc(instr.next_addr());
        }
        // ARMv4: a loaded base wins; there is no additional writeback.
        if writeback && regs & (1 << rn) == 0 {
            cpu.set_reg(rn, new_base);
        }
    } else {
        for reg in 0..16u8 {
            if regs & (1 << reg) == 0 {
                continue;
            }
            let address = start.wrapping_add(index * 4);
            let value = if reg == 15 {
                instr.addr.wrapping_add(12)
            } else if reg == rn && writeback && rn != first {
                // ARMv4: STM with the base in a writeback list stores the old
                // base when it is the first entry, otherwise the new base.
                new_base
            } else {
                cpu.reg(reg)
            };
            bus.write32(address, value)?;
            index += 1;
        }
        if writeback {
            cpu.set_reg(rn, new_base);
        }
        cpu.set_pc(instr.next_addr());
    }
    Ok(())
}

pub(super) fn execute_push(
    cpu: &mut Cpu,
    bus: &mut Bus,
    instr: IrInstr,
    regs: u8,
    lr: bool,
) -> Result<()> {
    let sp = cpu.reg(13);
    addr::require_aligned_block_base(sp, true)?;
    let count = addr::stack_count(regs, lr);
    let start = sp.wrapping_sub(count.wrapping_mul(4));
    let mut index = 0u32;
    for reg in 0..8u8 {
        if regs & (1 << reg) == 0 {
            continue;
        }
        bus.write32(start.wrapping_add(index * 4), cpu.reg(reg))?;
        index += 1;
    }
    if lr {
        bus.write32(start.wrapping_add(index * 4), cpu.reg(14))?;
    }
    cpu.set_reg(13, start);
    cpu.set_pc(instr.next_addr());
    Ok(())
}

pub(super) fn execute_pop(
    cpu: &mut Cpu,
    bus: &mut Bus,
    instr: IrInstr,
    regs: u8,
    pc: bool,
) -> Result<()> {
    let sp = cpu.reg(13);
    addr::require_aligned_block_base(sp, true)?;
    let count = addr::stack_count(regs, pc);
    let mut index = 0u32;
    for reg in 0..8u8 {
        if regs & (1 << reg) == 0 {
            continue;
        }
        let value = bus.read32(sp.wrapping_add(index * 4))?;
        cpu.set_reg(reg, value);
        index += 1;
    }
    if pc {
        let value = bus.read32(sp.wrapping_add(index * 4))?;
        // Thumb POP PC clears bit 0 and keeps Thumb state.
        cpu.set_pc(value & !1);
    } else {
        cpu.set_pc(instr.next_addr());
    }
    cpu.set_reg(13, sp.wrapping_add(count.wrapping_mul(4)));
    Ok(())
}

pub(super) fn execute_thumb_block(
    cpu: &mut Cpu,
    bus: &mut Bus,
    instr: IrInstr,
    load: bool,
    rn: u8,
    regs: u8,
) -> Result<()> {
    let base = cpu.reg(rn);
    addr::require_aligned_block_base(base, true)?;
    let count = regs.count_ones();
    let new_base = base.wrapping_add(count * 4);
    let first = regs.trailing_zeros() as u8;
    let mut index = 0u32;

    if load {
        for reg in 0..8u8 {
            if regs & (1 << reg) == 0 {
                continue;
            }
            let value = bus.read32(base.wrapping_add(index * 4))?;
            cpu.set_reg(reg, value);
            index += 1;
        }
        // Thumb LDM with the base in the list has no additional writeback; the
        // loaded value wins. Otherwise the documented increment is committed.
        if regs & (1 << rn) == 0 {
            cpu.set_reg(rn, new_base);
        }
    } else {
        for reg in 0..8u8 {
            if regs & (1 << reg) == 0 {
                continue;
            }
            let value = if reg == rn && rn != first {
                new_base
            } else {
                cpu.reg(reg)
            };
            bus.write32(base.wrapping_add(index * 4), value)?;
            index += 1;
        }
        cpu.set_reg(rn, new_base);
    }
    cpu.set_pc(instr.next_addr());
    Ok(())
}
