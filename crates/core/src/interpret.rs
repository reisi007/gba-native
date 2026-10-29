//! Reference interpreter for typed IR. This is both the semantic reference and
//! the runtime fallback for the native-AOT prototype.

use crate::bus::Bus;
use crate::cpu::{
    Cpu, IRQ_DISPATCH_CONTINUATION, IRQ_FRAME_BYTES, IRQ_FRAME_WORDS, Mode, adc_flags, add_flags,
    condition_holds, sbc_flags, sub_flags,
};
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

/// Execute one guest instruction, conditional no-op or modeled IRQ return step.
pub fn step(cpu: &mut Cpu, bus: &mut Bus) -> Result<()> {
    if cpu.mode() == Mode::Irq && !cpu.thumb() && cpu.pc() == IRQ_DISPATCH_CONTINUATION {
        return return_from_irq_dispatch(cpu, bus);
    }
    let addr = cpu.pc();
    let thumb = cpu.thumb();
    let instr = decode_at(bus, addr, thumb)?;
    execute(cpu, bus, instr)
}

/// Deliver a pending enabled IRQ before the next guest instruction when the
/// handler and six-word IRQ frame are valid. This models the documented BIOS
/// dispatcher behavior without fetching BIOS code. Entry consumes no guest
/// instruction budget. Shared by the reference runner and generated AOT code.
pub fn deliver_pending_irq(cpu: &mut Cpu, bus: &mut Bus) -> Result<bool> {
    if cpu.irq_masked() || cpu.mode() == Mode::Irq || !bus.irq_pending() {
        return Ok(false);
    }
    let Some(handler) = bus.irq_handler() else {
        return Ok(false);
    };
    if handler & 3 != 0 {
        return Err(crate::error::Error::IrqHandler {
            addr: handler,
            reason: "handler pointer is not word-aligned ARM code",
        });
    }
    bus.fetch32(handler)
        .map_err(|_| crate::error::Error::IrqHandler {
            addr: handler,
            reason: "handler pointer does not reference executable memory",
        })?;

    let sp = cpu.irq_stack_pointer();
    if sp & 3 != 0 {
        return Err(crate::error::Error::IrqStack {
            sp,
            reason: "stack pointer is not word-aligned",
        });
    }
    let frame_sp = sp
        .checked_sub(IRQ_FRAME_BYTES)
        .ok_or(crate::error::Error::IrqStack {
            sp,
            reason: "six-word frame underflows the address space",
        })?;
    if !Bus::is_work_ram_range(frame_sp, IRQ_FRAME_BYTES as usize) {
        return Err(crate::error::Error::IrqStack {
            sp,
            reason: "six-word frame must fit wholly in EWRAM or IWRAM",
        });
    }

    let frame = cpu.irq_frame_values();
    for (index, value) in frame.into_iter().enumerate() {
        bus.write32(frame_sp + index as u32 * 4, value)?;
    }
    cpu.enter_irq(handler, frame_sp);
    Ok(true)
}

fn return_from_irq_dispatch(cpu: &mut Cpu, bus: &mut Bus) -> Result<()> {
    let frame_sp = cpu.reg(13);
    if frame_sp & 3 != 0 {
        return Err(crate::error::Error::IrqStack {
            sp: frame_sp,
            reason: "dispatcher frame pointer is not word-aligned",
        });
    }
    if !Bus::is_work_ram_range(frame_sp, IRQ_FRAME_BYTES as usize) {
        return Err(crate::error::Error::IrqStack {
            sp: frame_sp,
            reason: "dispatcher frame must fit wholly in EWRAM or IWRAM",
        });
    }
    let restored_sp =
        frame_sp
            .checked_add(IRQ_FRAME_BYTES)
            .ok_or(crate::error::Error::IrqStack {
                sp: frame_sp,
                reason: "restored stack pointer overflows the address space",
            })?;
    let mut frame = [0; IRQ_FRAME_WORDS];
    for (index, value) in frame.iter_mut().enumerate() {
        *value = bus.read32(frame_sp + index as u32 * 4)?;
    }
    cpu.validate_exception_return()?;

    // The BIOS dispatcher restores the saved user registers and IRQ LR before
    // applying SUBS-PC/LR/#4 semantics. No guest BIOS instruction is fetched.
    for (index, value) in frame[..4].iter().copied().enumerate() {
        cpu.set_reg(index as u8, value);
    }
    cpu.set_reg(12, frame[4]);
    cpu.set_reg(14, frame[5]);
    cpu.set_reg(13, restored_sp);
    cpu.return_from_exception(frame[5].wrapping_sub(4))
}

fn read_rn(cpu: &Cpu, instr: IrInstr, rn: u8, rhs: Src) -> u32 {
    semantics::read_register(cpu, instr.addr, instr.thumb, rn, rhs.uses_register_shift())
}

fn eval_rhs(cpu: &Cpu, instr: IrInstr, rhs: Src) -> (u32, Option<bool>) {
    semantics::operand(cpu, instr.addr, instr.thumb, rhs, rhs.uses_register_shift())
}

/// Write `Rd` and report whether the ordinary `S=1` flag update still applies.
///
/// A non-PC `Rd` writes the register and falls through. `S=0` PC writes are
/// force-aligned and keep the ARM/Thumb state. `S=1` with `Rd=R15` is the
/// documented exception return: the computed value becomes the PC in the state
/// restored from the current mode's SPSR, so the arithmetic N/Z/C/V result must
/// not be applied afterwards. The restore is validated before any mutation, so
/// a missing or invalid SPSR leaves the CPU untouched.
fn write_rd(cpu: &mut Cpu, instr: IrInstr, rd: u8, set_flags: bool, value: u32) -> Result<bool> {
    if rd != 15 {
        cpu.set_reg(rd, value);
        cpu.set_pc(instr.next_addr());
        return Ok(true);
    }
    if set_flags {
        cpu.return_from_exception(value)?;
        return Ok(false);
    }
    cpu.set_pc(semantics::aligned_pc(value, instr.thumb));
    Ok(true)
}

pub fn execute(cpu: &mut Cpu, bus: &mut Bus, instr: IrInstr) -> Result<()> {
    if !condition_holds(cpu.cpsr(), instr.cond) {
        cpu.set_pc(instr.next_addr());
        return Ok(());
    }

    match instr.op {
        Op::Mov { rd, src, set_flags } => {
            let (value, shifter_carry) = eval_rhs(cpu, instr, src);
            if write_rd(cpu, instr, rd, set_flags, value)? && set_flags {
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
            if write_rd(cpu, instr, rd, set_flags, result)? && set_flags {
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
            if write_rd(cpu, instr, rd, set_flags, result)? && set_flags {
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
                    let ordinary = match rd {
                        Some(rd) => write_rd(cpu, instr, rd, set_flags, result)?,
                        None => {
                            cpu.set_pc(instr.next_addr());
                            true
                        }
                    };
                    if set_flags && ordinary {
                        cpu.set_logic_flags(result, shifter_carry);
                    }
                }
                _ => {
                    let (result, carry, overflow) =
                        arithmetic_result(cpu, opcode, rn_value, rhs_value);
                    let ordinary = match rd {
                        Some(rd) => write_rd(cpu, instr, rd, set_flags, result)?,
                        None => {
                            cpu.set_pc(instr.next_addr());
                            true
                        }
                    };
                    if set_flags && ordinary {
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
            // MUL with Rd=R15 is not a valid encoding, so this can never be the
            // `S=1` exception-return form.
            write_rd(cpu, instr, rd, false, value)?;
            if set_flags {
                // N/Z only: C is "destroyed" on ARMv4 (preserved here as a
                // documented Design choice) and V is unaffected.
                cpu.set_logic_flags(value, None);
            }
        }
        Op::MultiplyLong {
            signed,
            accumulate,
            set_flags,
            rd_hi,
            rd_lo,
            rs,
            rm,
        } => {
            let mut value = if signed {
                let rhs = i64::from(cpu.reg(rm) as i32);
                let lhs = i64::from(cpu.reg(rs) as i32);
                rhs.wrapping_mul(lhs) as u64
            } else {
                u64::from(cpu.reg(rm)).wrapping_mul(u64::from(cpu.reg(rs)))
            };
            if accumulate {
                let accumulator = (u64::from(cpu.reg(rd_hi)) << 32) | u64::from(cpu.reg(rd_lo));
                value = value.wrapping_add(accumulator);
            }
            let low = value as u32;
            let high = (value >> 32) as u32;
            cpu.set_reg(rd_lo, low);
            cpu.set_reg(rd_hi, high);
            if set_flags {
                let (_, _, carry, overflow) = cpu.flags();
                cpu.set_nzcv(high & 0x8000_0000 != 0, value == 0, carry, overflow);
            }
            cpu.set_pc(instr.next_addr());
        }
        Op::Swap { rd, rm, rn, byte } => {
            memory::execute_swap(cpu, bus, instr, rd, rm, rn, byte)?;
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
        deliver_pending_irq(cpu, bus)?;
        step(cpu, bus)?;
        bus.tick_frame();
        used += 1;
    }
    Ok(used)
}
