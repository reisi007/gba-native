use std::collections::{HashSet, VecDeque};

use super::artifact::{AotArtifact, AotBlock, DiscoveryStats, Limits, decode_rom};
use crate::cart::Cartridge;
use crate::error::{Error, Result};
use crate::ir::{Cond, IrInstr, Op, Src};

pub fn discover(
    cart: &Cartridge,
    entry: u32,
    entry_thumb: bool,
    limits: Limits,
) -> Result<AotArtifact> {
    if cart.rom_offset(entry).is_none() {
        return Err(Error::Aot(format!(
            "entry 0x{entry:08X} is outside the immutable ROM image"
        )));
    }
    if (entry_thumb && !entry.is_multiple_of(2)) || (!entry_thumb && !entry.is_multiple_of(4)) {
        return Err(Error::Aot(format!(
            "entry 0x{entry:08X} is not aligned for its instruction state"
        )));
    }
    if limits.max_block_instructions == 0 {
        return Err(Error::Aot(
            "max_block_instructions must be at least 1; a zero cap cannot form a block".to_string(),
        ));
    }
    let mut queue = VecDeque::new();
    queue.push_back((entry, entry_thumb));
    let mut seen = HashSet::new();
    let mut blocks = Vec::new();
    let mut stats = DiscoveryStats::default();

    while let Some((block_addr, block_thumb)) = queue.pop_front() {
        if !seen.insert((block_addr, block_thumb)) {
            continue;
        }
        if blocks.len() >= limits.max_blocks || stats.instructions >= limits.max_instructions {
            stats.limit_reached = true;
            break;
        }
        if (block_thumb && block_addr % 2 != 0) || (!block_thumb && block_addr % 4 != 0) {
            stats.decode_stops += 1;
            continue;
        }
        if cart.rom_offset(block_addr).is_none() {
            stats.external_targets += 1;
            continue;
        }

        let mut instructions = Vec::new();
        let mut constants: [Option<u32>; 16] = [None; 16];
        let mut cursor = block_addr;

        loop {
            if stats.instructions >= limits.max_instructions {
                stats.limit_reached = true;
                break;
            }
            if instructions.len() >= limits.max_block_instructions {
                // Per-block cap: finish this block and resume at the cursor.
                // This is not a global budget stop; continuation stays queued
                // so reachable code is not silently discarded.
                stats.block_limit_splits += 1;
                enqueue(&mut queue, cart, &mut stats, cursor, block_thumb);
                break;
            }
            let instruction = match decode_rom(cart, cursor, block_thumb) {
                Ok(instruction) => instruction,
                Err(_) => {
                    stats.decode_stops += 1;
                    break;
                }
            };

            let resolved_bx = match instruction.op {
                Op::Bx { rm } => constants[rm as usize],
                _ => None,
            };
            if matches!(instruction.op, Op::Bx { .. }) {
                if resolved_bx.is_some() {
                    stats.resolved_bx += 1;
                } else {
                    stats.dynamic_bx += 1;
                }
            }

            instructions.push(instruction);
            stats.instructions += 1;
            update_constants(&mut constants, instruction, cart);

            match instruction.op {
                Op::B { target } => {
                    enqueue(&mut queue, cart, &mut stats, target, block_thumb);
                    if instruction.cond != Cond::Al {
                        enqueue(
                            &mut queue,
                            cart,
                            &mut stats,
                            instruction.next_addr(),
                            block_thumb,
                        );
                    }
                    break;
                }
                Op::Bx { .. } => {
                    if let Some(value) = resolved_bx {
                        enqueue(&mut queue, cart, &mut stats, value & !1, value & 1 != 0);
                    }
                    if instruction.cond != Cond::Al {
                        enqueue(
                            &mut queue,
                            cart,
                            &mut stats,
                            instruction.next_addr(),
                            block_thumb,
                        );
                    }
                    break;
                }
                _ => {
                    cursor = instruction.next_addr();
                }
            }
        }

        if !instructions.is_empty() {
            blocks.push(AotBlock {
                addr: block_addr,
                thumb: block_thumb,
                instructions,
            });
            stats.blocks = blocks.len();
        }
        if stats.limit_reached {
            break;
        }
    }

    Ok(AotArtifact {
        rom_sha256: cart.sha256(),
        rom_len: cart.len(),
        entry,
        entry_thumb,
        blocks,
        stats,
    })
}

fn enqueue(
    queue: &mut VecDeque<(u32, bool)>,
    cart: &Cartridge,
    stats: &mut DiscoveryStats,
    addr: u32,
    thumb: bool,
) {
    if cart.rom_offset(addr).is_some() {
        queue.push_back((addr, thumb));
    } else {
        stats.external_targets += 1;
    }
}

fn update_constants(constants: &mut [Option<u32>; 16], instruction: IrInstr, cart: &Cartridge) {
    // A CPSR control write can switch the active bank, so cached r8-r14 values
    // no longer describe the registers of the following instruction. r0-r7 and
    // PC are not banked.
    if instruction.writes_cpsr_control() {
        constants[8..=14].fill(None);
    }
    if instruction.cond != Cond::Al {
        if let Some(rd) = written_register(instruction) {
            constants[rd as usize] = None;
        }
        return;
    }
    match instruction.op {
        Op::Mov {
            rd,
            src: Src::Imm { value, .. },
            ..
        } => constants[rd as usize] = Some(value),
        Op::Mov { rd, .. } => constants[rd as usize] = None,
        Op::Add { rd, .. } | Op::Sub { rd, .. } => constants[rd as usize] = None,
        Op::LdrLiteral { rd, addr } => {
            constants[rd as usize] = cart
                .read_u32(addr & !3)
                .map(|word| word.rotate_right((addr & 3) * 8));
        }
        Op::LdrHalf { rd, .. } => constants[rd as usize] = None,
        Op::Mrs { rd, .. } => constants[rd as usize] = None,
        Op::Cmp { .. } | Op::B { .. } | Op::Bx { .. } | Op::StrHalf { .. } | Op::Msr { .. } => {}
    }
}

fn written_register(instruction: IrInstr) -> Option<u8> {
    match instruction.op {
        Op::Mov { rd, .. }
        | Op::Add { rd, .. }
        | Op::Sub { rd, .. }
        | Op::LdrLiteral { rd, .. }
        | Op::LdrHalf { rd, .. }
        | Op::Mrs { rd, .. } => Some(rd),
        Op::Cmp { .. } | Op::B { .. } | Op::Bx { .. } | Op::StrHalf { .. } | Op::Msr { .. } => None,
    }
}
