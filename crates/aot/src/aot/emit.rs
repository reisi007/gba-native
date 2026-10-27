use std::fmt::Write as _;

use super::artifact::{AotArtifact, AotBlock};
use super::emit_driver::emit_driver;
use super::emit_ops::instruction_body;
use super::emit_program::emit_program_module;
use gba_native::cart::Cartridge;
use gba_native::error::Result;
use gba_native::ir::{Cond, IrInstr, Op};

/// Emit a self-contained native harness: the target-neutral program section
/// plus the native driver that binds an external ROM and reports the result.
/// Known blocks become direct statements; only unknown targets call the
/// reference interpreter.
pub fn emit_native_module(cart: &Cartridge, artifact: &AotArtifact, budget: u64) -> Result<String> {
    let mut out = emit_program_module(cart, artifact)?;
    emit_driver(&mut out, artifact, budget)?;
    Ok(out)
}

pub(super) fn emit_block(out: &mut String, block: &AotBlock) -> Result<()> {
    let name = block_name(block.addr, block.thumb);
    writeln!(
        out,
        "fn {name}(cpu: &mut Cpu, bus: &mut Bus, budget: &mut u64) -> Result<Next> {{"
    )
    .unwrap();
    for instruction in &block.instructions {
        writeln!(
            out,
            "    // 0x{:08X} {} 0x{:0width$X}",
            instruction.addr,
            if instruction.thumb { "thumb" } else { "arm" },
            instruction.raw,
            width = if instruction.thumb { 4 } else { 8 }
        )
        .unwrap();
        emit_instruction(out, instruction)?;
    }
    writeln!(out, "    Ok(Next::Continue)").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    Ok(())
}

fn emit_instruction(out: &mut String, instruction: &IrInstr) -> Result<()> {
    match instruction.op {
        Op::B { target } => emit_direct_branch(out, instruction, target, false),
        Op::Bl { target } => emit_direct_branch(out, instruction, target, true),
        Op::Bx { rm } => emit_bx(out, instruction, rm),
        Op::BlLow { offset } => {
            writeln!(
                out,
                "    let target = cpu.reg(14).wrapping_add(0x{offset:08X}u32);"
            )
            .unwrap();
            writeln!(
                out,
                "    cpu.set_reg(14, 0x{:08X}u32);",
                instruction.next_addr() | 1
            )
            .unwrap();
            writeln!(out, "    cpu.set_reg(15, target & !1);").unwrap();
            emit_tick(out, "    ");
            writeln!(out, "    return Ok(Next::Continue);").unwrap();
        }
        _ if instruction.op.writes_pc() => {
            let body = instruction_body(instruction);
            if instruction.cond == Cond::Al {
                out.push_str(&body);
            } else {
                writeln!(out, "    if {} {{", condition_expression(instruction.cond)).unwrap();
                out.push_str(&indent(&body, 4));
                writeln!(out, "    }} else {{").unwrap();
                writeln!(
                    out,
                    "        cpu.set_reg(15, 0x{:08X});",
                    instruction.next_addr()
                )
                .unwrap();
                writeln!(out, "    }}").unwrap();
            }
            emit_tick(out, "    ");
            writeln!(out, "    return Ok(Next::Continue);").unwrap();
        }
        _ => {
            let body = instruction_body(instruction);
            if instruction.cond == Cond::Al {
                out.push_str(&body);
            } else {
                writeln!(out, "    if {} {{", condition_expression(instruction.cond)).unwrap();
                out.push_str(&indent(&body, 4));
                writeln!(out, "    }}").unwrap();
            }
            writeln!(
                out,
                "    cpu.set_reg(15, 0x{:08X});",
                instruction.next_addr()
            )
            .unwrap();
            emit_tick(out, "    ");
        }
    }
    Ok(())
}

fn emit_direct_branch(out: &mut String, instruction: &IrInstr, target: u32, link: bool) {
    if instruction.cond == Cond::Al {
        if link {
            writeln!(
                out,
                "    cpu.set_reg(14, 0x{:08X});",
                instruction.next_addr()
            )
            .unwrap();
        }
        writeln!(out, "    cpu.set_reg(15, 0x{target:08X});").unwrap();
    } else {
        writeln!(out, "    if {} {{", condition_expression(instruction.cond)).unwrap();
        if link {
            writeln!(
                out,
                "        cpu.set_reg(14, 0x{:08X});",
                instruction.next_addr()
            )
            .unwrap();
        }
        writeln!(out, "        cpu.set_reg(15, 0x{target:08X});").unwrap();
        writeln!(out, "    }} else {{").unwrap();
        writeln!(
            out,
            "        cpu.set_reg(15, 0x{:08X});",
            instruction.next_addr()
        )
        .unwrap();
        writeln!(out, "    }}").unwrap();
    }
    emit_tick(out, "    ");
    writeln!(out, "    return Ok(Next::Continue);").unwrap();
}

fn emit_bx(out: &mut String, instruction: &IrInstr, rm: u8) {
    let body_indent = if instruction.cond == Cond::Al {
        "    "
    } else {
        "        "
    };
    if instruction.cond != Cond::Al {
        writeln!(out, "    if {} {{", condition_expression(instruction.cond)).unwrap();
    }
    if rm == 15 {
        let value = if instruction.thumb {
            format!("0x{:08X}u32", instruction.addr.wrapping_add(4) & !2)
        } else {
            format!("0x{:08X}u32", instruction.addr.wrapping_add(8))
        };
        writeln!(out, "{body_indent}let target = {value};").unwrap();
    } else {
        writeln!(out, "{body_indent}let target = cpu.reg({rm});").unwrap();
    }
    writeln!(out, "{body_indent}cpu.set_thumb(target & 1 != 0);").unwrap();
    writeln!(out, "{body_indent}cpu.set_reg(15, target & !1);").unwrap();
    if instruction.cond != Cond::Al {
        writeln!(out, "    }} else {{").unwrap();
        writeln!(
            out,
            "        cpu.set_reg(15, 0x{:08X});",
            instruction.next_addr()
        )
        .unwrap();
        writeln!(out, "    }}").unwrap();
    }
    emit_tick(out, "    ");
    writeln!(out, "    return Ok(Next::Continue);").unwrap();
}

fn condition_expression(cond: Cond) -> String {
    format!(
        "gba_native::cpu::condition_holds(cpu.cpsr(), Cond::{})",
        cond.name()
    )
}

fn emit_tick(out: &mut String, indent: &str) {
    writeln!(out, "{indent}*budget -= 1;").unwrap();
    writeln!(out, "{indent}if *budget == 0 {{").unwrap();
    writeln!(out, "{indent}    return Ok(Next::Halt);").unwrap();
    writeln!(out, "{indent}}}").unwrap();
}

fn indent(text: &str, spaces: usize) -> String {
    let prefix = " ".repeat(spaces);
    text.lines()
        .map(|line| format!("{prefix}{line}\n"))
        .collect()
}

pub(super) fn block_name(addr: u32, thumb: bool) -> String {
    format!("block_{addr:08x}_{}", if thumb { "thumb" } else { "arm" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indentation_is_stable() {
        assert_eq!(indent("a\nb\n", 2), "  a\n  b\n");
    }
}
