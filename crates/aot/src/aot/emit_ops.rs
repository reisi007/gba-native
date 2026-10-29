use std::fmt::Write as _;

use super::emit_mem;
use gba_native::ir::{AluOpcode, IrInstr, Op, Shift, Src};

#[derive(Clone, Copy)]
struct Site {
    addr: u32,
    thumb: bool,
}

/// Emit the direct Rust statements for one non-branch instruction. Every known
/// operation becomes explicit register/helper calls; there is no decoder or
/// interpreter call inside a translated block.
pub(super) fn instruction_body(instruction: &IrInstr) -> String {
    let mut out = String::new();
    let site = Site {
        addr: instruction.addr,
        thumb: instruction.thumb,
    };
    match instruction.op {
        Op::Mrs { rd, psr } => {
            writeln!(
                out,
                "    let value = cpu.read_psr(gba_native::ir::Psr::{})?;",
                psr.rust_name()
            )
            .unwrap();
            writeln!(out, "    cpu.set_reg({rd}, value);").unwrap();
        }
        Op::Msr { psr, fields, src } => {
            writeln!(
                out,
                "    cpu.write_psr(gba_native::ir::Psr::{}, 0x{fields:02X}, {})?;",
                psr.rust_name(),
                psr_source_expression(src)
            )
            .unwrap();
        }
        Op::Swi { comment } => {
            writeln!(
                out,
                "    gba_native::bios::execute_swi(0x{comment:08X}u32, {}, 0x{:08X}u32, cpu, bus)?;",
                site.thumb,
                site.addr
            )
            .unwrap();
        }
        Op::Mov { rd, src, set_flags } => {
            writeln!(
                out,
                "    let (value, shifter_carry) = {};",
                operand_call(site, src)
            )
            .unwrap();
            if write_result(&mut out, Some(rd), "value", site.thumb, set_flags) && set_flags {
                writeln!(out, "    cpu.set_logic_flags(value, shifter_carry);").unwrap();
            }
        }
        Op::Add {
            rd,
            rn,
            rhs,
            set_flags,
        } => emit_add_sub(&mut out, site, rd, rn, rhs, set_flags, true),
        Op::Sub {
            rd,
            rn,
            rhs,
            set_flags,
        } => emit_add_sub(&mut out, site, rd, rn, rhs, set_flags, false),
        Op::Cmp { rn, rhs } => {
            emit_operand_bindings(&mut out, site, rn, rhs);
            writeln!(
                out,
                "    let (result, carry, overflow) = gba_native::cpu::sub_flags(rn_value, rhs);"
            )
            .unwrap();
            writeln!(out, "    cpu.set_arith_flags(result, carry, overflow);").unwrap();
        }
        Op::Alu {
            opcode,
            rd,
            rn,
            rhs,
            set_flags,
        } => emit_alu(&mut out, site, opcode, rd, rn, rhs, set_flags),
        Op::Mul {
            rd,
            rm,
            rs,
            accumulate,
            set_flags,
        } => {
            let mut expression = format!("cpu.reg({rm}).wrapping_mul(cpu.reg({rs}))");
            if let Some(rn) = accumulate {
                expression = format!("({expression}).wrapping_add(cpu.reg({rn}))");
            }
            writeln!(out, "    let value = {expression};").unwrap();
            // MUL with Rd=R15 is not a valid encoding, so this is never the
            // `S=1` exception-return form.
            write_result(&mut out, Some(rd), "value", site.thumb, false);
            if set_flags {
                writeln!(out, "    cpu.set_logic_flags(value, None);").unwrap();
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
            if signed {
                writeln!(
                    out,
                    "    let mut value = (cpu.reg({rm}) as i32 as i64).wrapping_mul(cpu.reg({rs}) as i32 as i64) as u64;"
                )
                .unwrap();
            } else {
                writeln!(
                    out,
                    "    let mut value = (cpu.reg({rm}) as u64).wrapping_mul(cpu.reg({rs}) as u64);"
                )
                .unwrap();
            }
            if accumulate {
                writeln!(
                    out,
                    "    value = value.wrapping_add(((cpu.reg({rd_hi}) as u64) << 32) | cpu.reg({rd_lo}) as u64);"
                )
                .unwrap();
            }
            writeln!(out, "    let low = value as u32;").unwrap();
            writeln!(out, "    let high = (value >> 32) as u32;").unwrap();
            writeln!(out, "    cpu.set_reg({rd_lo}, low);").unwrap();
            writeln!(out, "    cpu.set_reg({rd_hi}, high);").unwrap();
            if set_flags {
                writeln!(out, "    let (_, _, carry, overflow) = cpu.flags();").unwrap();
                writeln!(
                    out,
                    "    cpu.set_nzcv(high & 0x8000_0000 != 0, low == 0 && high == 0, carry, overflow);"
                )
                .unwrap();
            }
        }
        Op::Swap { rd, rm, rn, byte } => {
            writeln!(out, "    let address = cpu.reg({rn});").unwrap();
            writeln!(out, "    let source = cpu.reg({rm});").unwrap();
            if byte {
                writeln!(out, "    let value = u32::from(bus.read8(address)?);").unwrap();
                writeln!(out, "    bus.write8(address, source as u8)?;").unwrap();
            } else {
                writeln!(out, "    let word = bus.read32(address & !3)?;").unwrap();
                writeln!(out, "    let value = word.rotate_right((address & 3) * 8);").unwrap();
                writeln!(out, "    bus.write32(address & !3, source)?;").unwrap();
            }
            writeln!(out, "    cpu.set_reg({rd}, value);").unwrap();
        }
        Op::BlHigh { lr } => {
            writeln!(out, "    cpu.set_reg(14, 0x{lr:08X}u32);").unwrap();
        }
        Op::LdrLiteral { rd, addr } => {
            writeln!(out, "    let address = 0x{addr:08X}u32;").unwrap();
            writeln!(out, "    let word = bus.read32(address & !3)?;").unwrap();
            writeln!(
                out,
                "    cpu.set_reg({rd}, word.rotate_right((address & 3) * 8));"
            )
            .unwrap();
        }
        Op::LdrHalf { rd, rn, offset } => {
            writeln!(
                out,
                "    let address = cpu.reg({rn}).wrapping_add(0x{:08X}u32);",
                offset as u32
            )
            .unwrap();
            writeln!(out, "    let half = bus.read16(address & !1)? as u32;").unwrap();
            writeln!(
                out,
                "    cpu.set_reg({rd}, if address & 1 != 0 {{ half.rotate_right(8) }} else {{ half }});"
            )
            .unwrap();
        }
        Op::StrHalf { rd, rn, offset } => {
            writeln!(
                out,
                "    let address = cpu.reg({rn}).wrapping_add(0x{:08X}u32);",
                offset as u32
            )
            .unwrap();
            writeln!(out, "    bus.write16(address & !1, cpu.reg({rd}) as u16)?;").unwrap();
        }
        Op::MemSingle { .. }
        | Op::BlockTransfer { .. }
        | Op::Push { .. }
        | Op::Pop { .. }
        | Op::ThumbBlock { .. } => out.push_str(&emit_mem::instruction_body(instruction)),
        Op::B { .. } | Op::Bl { .. } | Op::BlLow { .. } | Op::Bx { .. } => unreachable!(),
    }
    out
}

fn emit_operand_bindings(out: &mut String, site: Site, rn: u8, rhs: Src) {
    writeln!(
        out,
        "    let (rhs, shifter_carry) = {};",
        operand_call(site, rhs)
    )
    .unwrap();
    writeln!(
        out,
        "    let rn_value = {};",
        register_call(site, rn, rhs.uses_register_shift())
    )
    .unwrap();
}

fn emit_add_sub(
    out: &mut String,
    site: Site,
    rd: u8,
    rn: u8,
    rhs: Src,
    set_flags: bool,
    add: bool,
) {
    emit_operand_bindings(out, site, rn, rhs);
    let helper = if add {
        "gba_native::cpu::add_flags"
    } else {
        "gba_native::cpu::sub_flags"
    };
    writeln!(
        out,
        "    let (result, carry, overflow) = {helper}(rn_value, rhs);"
    )
    .unwrap();
    if write_result(out, Some(rd), "result", site.thumb, set_flags) && set_flags {
        writeln!(out, "    cpu.set_arith_flags(result, carry, overflow);").unwrap();
    }
}

fn emit_alu(
    out: &mut String,
    site: Site,
    opcode: AluOpcode,
    rd: Option<u8>,
    rn: u8,
    rhs: Src,
    set_flags: bool,
) {
    if opcode.is_logical() {
        emit_operand_bindings(out, site, rn, rhs);
        let expression = match opcode {
            AluOpcode::And | AluOpcode::Tst => "rn_value & rhs",
            AluOpcode::Eor | AluOpcode::Teq => "rn_value ^ rhs",
            AluOpcode::Orr => "rn_value | rhs",
            AluOpcode::Bic => "rn_value & !rhs",
            AluOpcode::Mvn => "!rhs",
            _ => unreachable!(),
        };
        writeln!(out, "    let result = {expression};").unwrap();
        if write_result(out, rd, "result", site.thumb, set_flags) && set_flags {
            writeln!(out, "    cpu.set_logic_flags(result, shifter_carry);").unwrap();
        }
        return;
    }
    emit_operand_bindings(out, site, rn, rhs);
    let expression = match opcode {
        AluOpcode::Rsb => "gba_native::cpu::sub_flags(rhs, rn_value)".to_string(),
        AluOpcode::Adc => "gba_native::cpu::adc_flags(rn_value, rhs, carry_in)".to_string(),
        AluOpcode::Sbc => "gba_native::cpu::sbc_flags(rn_value, rhs, carry_in)".to_string(),
        AluOpcode::Rsc => "gba_native::cpu::sbc_flags(rhs, rn_value, carry_in)".to_string(),
        AluOpcode::Cmn => "gba_native::cpu::add_flags(rn_value, rhs)".to_string(),
        _ => unreachable!(),
    };
    if matches!(opcode, AluOpcode::Adc | AluOpcode::Sbc | AluOpcode::Rsc) {
        writeln!(
            out,
            "    let carry_in = cpu.cpsr() & gba_native::cpu::FLAG_C != 0;"
        )
        .unwrap();
    }
    writeln!(out, "    let (result, carry, overflow) = {expression};").unwrap();
    if write_result(out, rd, "result", site.thumb, set_flags) && set_flags {
        writeln!(out, "    cpu.set_arith_flags(result, carry, overflow);").unwrap();
    }
}

/// Emit the `Rd` write and report whether the ordinary `S=1` flag update still
/// applies. `S=1` with `Rd=R15` is the documented exception return: the computed
/// value becomes the PC in the state restored from the current mode's SPSR and
/// the arithmetic N/Z/C/V result is discarded, because the restored CPSR
/// replaces it. The helper validates the SPSR before mutating any state.
fn write_result(
    out: &mut String,
    rd: Option<u8>,
    value: &str,
    thumb: bool,
    set_flags: bool,
) -> bool {
    match rd {
        Some(15) if set_flags => {
            writeln!(out, "    cpu.return_from_exception({value})?;").unwrap();
            false
        }
        Some(15) => {
            writeln!(
                out,
                "    cpu.set_reg(15, gba_native::semantics::aligned_pc({value}, {thumb}));"
            )
            .unwrap();
            true
        }
        Some(rd) => {
            writeln!(out, "    cpu.set_reg({rd}, {value});").unwrap();
            true
        }
        None => true,
    }
}

fn operand_call(site: Site, src: Src) -> String {
    let Site { addr, thumb } = site;
    format!(
        "gba_native::semantics::operand(cpu, 0x{addr:08X}, {thumb}, {}, {})",
        src_expression(src),
        src.uses_register_shift()
    )
}

fn register_call(site: Site, rn: u8, register_shift: bool) -> String {
    let Site { addr, thumb } = site;
    format!(
        "gba_native::semantics::read_register(cpu, 0x{addr:08X}, {thumb}, {rn}, {register_shift})"
    )
}

fn src_expression(src: Src) -> String {
    match src {
        Src::Reg(index) => format!("gba_native::ir::Src::Reg({index})"),
        Src::Imm { value, carry } => format!(
            "gba_native::ir::Src::Imm {{ value: 0x{value:08X}u32, carry: {} }}",
            option_bool(carry)
        ),
        Src::RegShift { rm, shift } => format!(
            "gba_native::ir::Src::RegShift {{ rm: {rm}, shift: {} }}",
            shift_expression(shift)
        ),
    }
}

fn shift_expression(shift: Shift) -> String {
    match shift {
        Shift::Imm { kind, amount } => format!(
            "gba_native::ir::Shift::Imm {{ kind: gba_native::ir::ShiftKind::{}, amount: {amount} }}",
            kind.rust_name()
        ),
        Shift::Reg { kind, rm } => format!(
            "gba_native::ir::Shift::Reg {{ kind: gba_native::ir::ShiftKind::{}, rm: {rm} }}",
            kind.rust_name()
        ),
    }
}

fn option_bool(value: Option<bool>) -> &'static str {
    match value {
        None => "None",
        Some(true) => "Some(true)",
        Some(false) => "Some(false)",
    }
}

fn psr_source_expression(src: gba_native::ir::PsrSrc) -> String {
    use gba_native::ir::PsrSrc;
    match src {
        PsrSrc::Reg(index) => format!("cpu.reg({index})"),
        PsrSrc::Imm(value) => format!("0x{value:08X}u32"),
    }
}
