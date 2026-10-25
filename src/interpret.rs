//! Reference interpreter for typed IR. This is both the semantic reference and
//! the runtime fallback for the native-AOT prototype.

use crate::bus::Bus;
use crate::cpu::{Cpu, add_flags, condition_holds, sub_flags};
use crate::decode::{decode_arm, decode_thumb};
use crate::error::Result;
use crate::ir::{IrInstr, Op, PsrSrc, Src};

pub fn decode_at(bus: &Bus, addr: u32, thumb: bool) -> Result<IrInstr> {
    if thumb {
        decode_thumb(addr, bus.fetch16(addr)? as u32)
    } else {
        decode_arm(addr, bus.fetch32(addr)?)
    }
}

/// Execute exactly one instruction (or one conditional no-op).
pub fn step(cpu: &mut Cpu, bus: &mut Bus) -> Result<()> {
    let addr = cpu.pc();
    let thumb = cpu.thumb();
    let instr = decode_at(bus, addr, thumb)?;
    execute(cpu, bus, instr)
}

pub fn execute(cpu: &mut Cpu, bus: &mut Bus, instr: IrInstr) -> Result<()> {
    if !condition_holds(cpu.cpsr(), instr.cond) {
        cpu.set_pc(instr.next_addr());
        return Ok(());
    }

    match instr.op {
        Op::Mov { rd, src, set_flags } => {
            let value = source_value(cpu, src);
            cpu.set_reg(rd, value);
            if set_flags {
                cpu.set_logic_flags(value, source_carry(src));
            }
            cpu.set_pc(instr.next_addr());
        }
        Op::Add {
            rd,
            rn,
            rhs,
            set_flags,
        } => {
            let (result, carry, overflow) = add_flags(cpu.reg(rn), source_value(cpu, rhs));
            cpu.set_reg(rd, result);
            if set_flags {
                cpu.set_arith_flags(result, carry, overflow);
            }
            cpu.set_pc(instr.next_addr());
        }
        Op::Sub {
            rd,
            rn,
            rhs,
            set_flags,
        } => {
            let (result, carry, overflow) = sub_flags(cpu.reg(rn), source_value(cpu, rhs));
            cpu.set_reg(rd, result);
            if set_flags {
                cpu.set_arith_flags(result, carry, overflow);
            }
            cpu.set_pc(instr.next_addr());
        }
        Op::Cmp { rn, rhs } => {
            let (result, carry, overflow) = sub_flags(cpu.reg(rn), source_value(cpu, rhs));
            cpu.set_arith_flags(result, carry, overflow);
            cpu.set_pc(instr.next_addr());
        }
        Op::B { target } => {
            cpu.set_pc(target);
        }
        Op::Bx { rm } => {
            let value = cpu.reg(rm);
            cpu.set_thumb(value & 1 != 0);
            cpu.set_pc(value & !1);
        }
        Op::LdrLiteral { rd, addr } => {
            let word = bus.read32(addr & !3)?;
            let value = word.rotate_right((addr & 3) * 8);
            cpu.set_reg(rd, value);
            cpu.set_pc(instr.next_addr());
        }
        Op::LdrHalf { rd, rn, offset } => {
            let effective = cpu.reg(rn).wrapping_add(offset as u32);
            let half = bus.read16(effective & !1)? as u32;
            let value = if effective & 1 != 0 {
                half.rotate_right(8)
            } else {
                half
            };
            cpu.set_reg(rd, value);
            cpu.set_pc(instr.next_addr());
        }
        Op::StrHalf { rd, rn, offset } => {
            let effective = cpu.reg(rn).wrapping_add(offset as u32);
            bus.write16(effective & !1, cpu.reg(rd) as u16)?;
            cpu.set_pc(instr.next_addr());
        }
        Op::Mrs { rd, psr } => {
            let value = cpu.read_psr(psr)?;
            cpu.set_reg(rd, value);
            cpu.set_pc(instr.next_addr());
        }
        Op::Msr { psr, fields, src } => {
            cpu.write_psr(psr, fields, psr_source_value(cpu, src))?;
            cpu.set_pc(instr.next_addr());
        }
    }
    Ok(())
}

fn psr_source_value(cpu: &Cpu, src: PsrSrc) -> u32 {
    match src {
        PsrSrc::Reg(index) => cpu.reg(index),
        PsrSrc::Imm(value) => value,
    }
}

pub fn source_value(cpu: &Cpu, src: Src) -> u32 {
    match src {
        Src::Reg(index) => cpu.reg(index),
        Src::Imm { value, .. } => value,
    }
}

fn source_carry(src: Src) -> Option<bool> {
    match src {
        Src::Reg(_) => None,
        Src::Imm { carry, .. } => carry,
    }
}

/// Execute at most `budget` instructions. Exhausting the budget is not evidence
/// that a game booted or produced a frame.
pub fn run_budget(cpu: &mut Cpu, bus: &mut Bus, budget: u64) -> Result<u64> {
    let mut used = 0;
    while used < budget {
        step(cpu, bus)?;
        used += 1;
    }
    Ok(used)
}
