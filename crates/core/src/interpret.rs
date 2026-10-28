//! Reference interpreter for typed IR. This is both the semantic reference and
//! the runtime fallback for the native-AOT prototype.

use crate::bus::Bus;
use crate::cpu::{Cpu, adc_flags, add_flags, condition_holds, sbc_flags, sub_flags};
use crate::decode::{decode_arm, decode_thumb};
use crate::error::Result;
use crate::ir::{AluOpcode, IrInstr, Op, PsrSrc, Src};
use crate::semantics;

mod memory;

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

fn read_rn(cpu: &Cpu, instr: IrInstr, rn: u8, rhs: Src) -> u32 {
    semantics::read_register(cpu, instr.addr, instr.thumb, rn, rhs.uses_register_shift())
}

fn eval_rhs(cpu: &Cpu, instr: IrInstr, rhs: Src) -> (u32, Option<bool>) {
    semantics::operand(cpu, instr.addr, instr.thumb, rhs, rhs.uses_register_shift())
}

/// Write `Rd` and either fall through or redirect the PC. PC writes through
/// MOV/ALU are force-aligned and do not change ARM/Thumb state.
fn write_rd(cpu: &mut Cpu, instr: IrInstr, rd: u8, value: u32) {
    if rd == 15 {
        cpu.set_pc(semantics::aligned_pc(value, instr.thumb));
    } else {
        cpu.set_reg(rd, value);
        cpu.set_pc(instr.next_addr());
    }
}

pub fn execute(cpu: &mut Cpu, bus: &mut Bus, instr: IrInstr) -> Result<()> {
    if !condition_holds(cpu.cpsr(), instr.cond) {
        cpu.set_pc(instr.next_addr());
        return Ok(());
    }

    match instr.op {
        Op::Mov { rd, src, set_flags } => {
            let (value, shifter_carry) = eval_rhs(cpu, instr, src);
            write_rd(cpu, instr, rd, value);
            if set_flags {
                cpu.set_logic_flags(value, shifter_carry);
            }
        }
        Op::Add {
            rd,
            rn,
            rhs,
            set_flags,
        } => {
            let (rhs_value, _) = eval_rhs(cpu, instr, rhs);
            let (result, carry, overflow) = add_flags(read_rn(cpu, instr, rn, rhs), rhs_value);
            write_rd(cpu, instr, rd, result);
            if set_flags {
                cpu.set_arith_flags(result, carry, overflow);
            }
        }
        Op::Sub {
            rd,
            rn,
            rhs,
            set_flags,
        } => {
            let (rhs_value, _) = eval_rhs(cpu, instr, rhs);
            let (result, carry, overflow) = sub_flags(read_rn(cpu, instr, rn, rhs), rhs_value);
            write_rd(cpu, instr, rd, result);
            if set_flags {
                cpu.set_arith_flags(result, carry, overflow);
            }
        }
        Op::Cmp { rn, rhs } => {
            let (rhs_value, _) = eval_rhs(cpu, instr, rhs);
            let (result, carry, overflow) = sub_flags(read_rn(cpu, instr, rn, rhs), rhs_value);
            cpu.set_arith_flags(result, carry, overflow);
            cpu.set_pc(instr.next_addr());
        }
        Op::Alu {
            opcode,
            rd,
            rn,
            rhs,
            set_flags,
        } => {
            let (rhs_value, shifter_carry) = eval_rhs(cpu, instr, rhs);
            let rn_value = read_rn(cpu, instr, rn, rhs);
            match opcode {
                AluOpcode::And
                | AluOpcode::Eor
                | AluOpcode::Orr
                | AluOpcode::Bic
                | AluOpcode::Mvn
                | AluOpcode::Tst
                | AluOpcode::Teq => {
                    let result = logical_result(opcode, rn_value, rhs_value);
                    if let Some(rd) = rd {
                        write_rd(cpu, instr, rd, result);
                    } else {
                        cpu.set_pc(instr.next_addr());
                    }
                    if set_flags {
                        cpu.set_logic_flags(result, shifter_carry);
                    }
                }
                _ => {
                    let (result, carry, overflow) =
                        arithmetic_result(cpu, opcode, rn_value, rhs_value);
                    if let Some(rd) = rd {
                        write_rd(cpu, instr, rd, result);
                    } else {
                        cpu.set_pc(instr.next_addr());
                    }
                    if set_flags {
                        cpu.set_arith_flags(result, carry, overflow);
                    }
                }
            }
        }
        Op::Mul {
            rd,
            rm,
            rs,
            accumulate,
            set_flags,
        } => {
            let mut value = cpu.reg(rm).wrapping_mul(cpu.reg(rs));
            if let Some(rn) = accumulate {
                value = value.wrapping_add(cpu.reg(rn));
            }
            write_rd(cpu, instr, rd, value);
            if set_flags {
                // N/Z only: C is "destroyed" on ARMv4 (preserved here as a
                // documented Design choice) and V is unaffected.
                cpu.set_logic_flags(value, None);
            }
        }
        Op::B { target } => cpu.set_pc(target),
        Op::Bl { target } => {
            // ARM BL writes PC+4, which is the next instruction address.
            cpu.set_reg(14, instr.next_addr());
            cpu.set_pc(target);
        }
        Op::BlHigh { lr } => {
            cpu.set_reg(14, lr);
            cpu.set_pc(instr.next_addr());
        }
        Op::BlLow { offset } => {
            let target = cpu.reg(14).wrapping_add(offset);
            cpu.set_reg(14, instr.next_addr() | 1);
            cpu.set_pc(target & !1);
        }
        Op::Bx { rm } => {
            let value = if rm == 15 {
                if instr.thumb {
                    (instr.addr.wrapping_add(4)) & !2
                } else {
                    semantics::operand_pc(instr.addr, false, false)
                }
            } else {
                cpu.reg(rm)
            };
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
        } => memory::execute_mem_single(
            cpu, bus, instr, load, width, signed, rd, rn, offset, add, pre_index, writeback,
        )?,
        Op::BlockTransfer {
            load,
            rn,
            regs,
            pre_index,
            add,
            writeback,
        } => memory::execute_block(cpu, bus, instr, load, rn, regs, pre_index, add, writeback)?,
        Op::Push { regs, lr } => memory::execute_push(cpu, bus, instr, regs, lr)?,
        Op::Pop { regs, pc } => memory::execute_pop(cpu, bus, instr, regs, pc)?,
        Op::ThumbBlock { load, rn, regs } => {
            memory::execute_thumb_block(cpu, bus, instr, load, rn, regs)?
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
        Op::Swi { comment } => {
            crate::bios::execute_swi(comment, instr.thumb, instr.addr, cpu, bus)?;
        }
    }
    Ok(())
}

fn logical_result(opcode: AluOpcode, rn: u32, rhs: u32) -> u32 {
    match opcode {
        AluOpcode::And | AluOpcode::Tst => rn & rhs,
        AluOpcode::Eor | AluOpcode::Teq => rn ^ rhs,
        AluOpcode::Orr => rn | rhs,
        AluOpcode::Bic => rn & !rhs,
        AluOpcode::Mvn => !rhs,
        _ => unreachable!("arithmetic opcode in logical_result"),
    }
}

fn arithmetic_result(cpu: &Cpu, opcode: AluOpcode, rn: u32, rhs: u32) -> (u32, bool, bool) {
    let carry_in = cpu.cpsr() & crate::cpu::FLAG_C != 0;
    match opcode {
        AluOpcode::Rsb => sub_flags(rhs, rn),
        AluOpcode::Adc => adc_flags(rn, rhs, carry_in),
        AluOpcode::Sbc => sbc_flags(rn, rhs, carry_in),
        AluOpcode::Rsc => sbc_flags(rhs, rn, carry_in),
        AluOpcode::Cmn => add_flags(rn, rhs),
        _ => unreachable!("logical opcode in arithmetic_result"),
    }
}

fn psr_source_value(cpu: &Cpu, src: PsrSrc) -> u32 {
    match src {
        PsrSrc::Reg(index) => cpu.reg(index),
        PsrSrc::Imm(value) => value,
    }
}

/// Execute at most `budget` instructions. Exhausting the budget is not evidence
/// that a game booted or produced a frame.
pub fn run_budget(cpu: &mut Cpu, bus: &mut Bus, budget: u64) -> Result<u64> {
    let mut used = 0;
    while used < budget {
        // Deliver a pending interrupt between instructions. Without a handler
        // pointer at 0x03007FFC there is nowhere to dispatch, so the request
        // stays latched instead of being dropped.
        if !cpu.irq_masked()
            && bus.irq_pending()
            && let Some(handler) = bus.irq_handler()
        {
            cpu.enter_irq(handler);
        }
        step(cpu, bus)?;
        bus.tick_frame();
        used += 1;
    }
    Ok(used)
}
