//! Pure address/width helpers for the implemented memory-transfer subset.
//!
//! Sources: GBATEK `#arm-cpu-memory-alignments`,
//! `#arm-opcodes-memory-single-data-transfer-ldr-str-pld`,
//! `#arm-opcodes-memory-halfword-doubleword-and-signed-data-transfer`,
//! `#arm-opcodes-memory-block-data-transfer-ldm-stm`,
//! `#thumb-opcodes-memory-loadstore-ldrstr` and
//! `#thumb-opcodes-memory-multiple-loadstore-pushpop-and-ldmstm`.
//!
//! These helpers compute addresses and register values only. They never access
//! memory; the interpreter and the generated native code keep their own direct,
//! width-checked `Bus` calls so byte accesses are real byte accesses.

use crate::cpu::Cpu;
use crate::ir::{MemOffset, Shift};
use crate::semantics;

/// ARM R15 read as a base register. GBATEK documents the instruction address
/// plus 8 for single transfers and block transfers.
pub fn base_value(cpu: &Cpu, addr: u32, thumb: bool, rn: u8) -> u32 {
    if rn == 15 {
        addr.wrapping_add(if thumb { 4 } else { 8 })
    } else {
        cpu.reg(rn)
    }
}

/// R15 as a stored source register. GBATEK documents PC+12 for ARM stores.
/// No Thumb encoding in the implemented subset can store PC directly.
pub fn stored_value(cpu: &Cpu, addr: u32, thumb: bool, rd: u8) -> u32 {
    if rd == 15 {
        addr.wrapping_add(if thumb { 4 } else { 12 })
    } else {
        cpu.reg(rd)
    }
}

/// Evaluate a single-transfer offset. ARM shifted offsets only carry immediate
/// shifts (register-controlled bit 4 is rejected at decode); Thumb register
/// offsets are encoded as LSL #0.
pub fn offset_value(cpu: &Cpu, offset: MemOffset) -> u32 {
    match offset {
        MemOffset::Imm(value) => value,
        MemOffset::RegShift { rm, shift } => {
            // Decode never builds a register-controlled memory offset.
            let (kind, amount) = match shift {
                Shift::Imm { kind, amount } => (kind, amount),
                Shift::Reg { .. } => unreachable!("register-controlled memory offset"),
            };
            // Rm=15 is rejected at decode, so the read is a plain register.
            let carry = cpu.cpsr() & crate::cpu::FLAG_C != 0;
            semantics::shift_immediate(kind, amount, cpu.reg(rm), carry).0
        }
    }
}

/// Address actually accessed. Post-indexing accesses the unmodified base.
pub fn accessed(base: u32, offset: u32, add: bool, pre_index: bool) -> u32 {
    if !pre_index {
        base
    } else if add {
        base.wrapping_add(offset)
    } else {
        base.wrapping_sub(offset)
    }
}

/// New base value for a writeback transfer, committed only after the access
/// succeeded.
pub fn written_back(base: u32, offset: u32, add: bool) -> u32 {
    if add {
        base.wrapping_add(offset)
    } else {
        base.wrapping_sub(offset)
    }
}

/// GBATEK: an unaligned word load reads `addr AND NOT 3` and rotates right by
/// `(addr AND 3) * 8`.
pub fn word_load(word: u32, address: u32) -> u32 {
    word.rotate_right((address & 3) * 8)
}

/// GBATEK ARM7: an odd unaligned LDRH reads `addr AND NOT 1` and rotates the
/// halfword result right by 8.
pub fn half_load(half: u16, address: u32) -> u32 {
    let half = u32::from(half);
    if address & 1 != 0 {
        half.rotate_right(8)
    } else {
        half
    }
}

pub fn signed_byte(byte: u8) -> u32 {
    (byte as i8 as i32) as u32
}

pub fn signed_half(half: u16) -> u32 {
    (half as i16 as i32) as u32
}

/// Number of registers in an ARM block-transfer bitmap.
pub fn register_count(regs: u16) -> u32 {
    regs.count_ones()
}

/// First (lowest-numbered) register in a non-empty block list.
pub fn first_register(regs: u16) -> u8 {
    debug_assert_ne!(regs, 0);
    regs.trailing_zeros() as u8
}

/// First address of an ARM block transfer. Registers are always transferred in
/// ascending order from this lowest address, including the decrement modes.
pub fn block_start(base: u32, count: u32, pre_index: bool, add: bool) -> u32 {
    let bytes = count.wrapping_mul(4);
    match (pre_index, add) {
        (false, true) => base,
        (true, true) => base.wrapping_add(4),
        (false, false) => base.wrapping_sub(bytes).wrapping_add(4),
        (true, false) => base.wrapping_sub(bytes),
    }
}

pub fn block_written_back(base: u32, count: u32, add: bool) -> u32 {
    let bytes = count.wrapping_mul(4);
    if add {
        base.wrapping_add(bytes)
    } else {
        base.wrapping_sub(bytes)
    }
}

/// Transfer base for ARM/Thumb block and stack transfers. GBATEK documents that
/// the low address bits of a block/stack base are ignored, so the addressing
/// mode is evaluated against `base & !3`.
///
/// **Design policy:** the public document does not establish whether writeback
/// derives from the original or the aligned base. This prototype aligns only
/// the transfer address; `block_written_back` and the stack update always use
/// the original, unmodified base register.
pub fn aligned_block_base(base: u32) -> u32 {
    base & !3
}

/// Number of words moved by a Thumb PUSH/POP: the low-register list plus the
/// optional LR (PUSH) or PC (POP) word.
pub fn stack_count(regs: u8, extra: bool) -> u32 {
    regs.count_ones() + u32::from(extra)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_addresses_are_ascending_for_every_mode() {
        let base = 0x0200_0000;
        // Four words: IA/IB/DA/DB lowest addresses.
        assert_eq!(block_start(base, 4, false, true), base);
        assert_eq!(block_start(base, 4, true, true), base + 4);
        assert_eq!(block_start(base, 4, false, false), base - 12);
        assert_eq!(block_start(base, 4, true, false), base - 16);
        assert_eq!(block_written_back(base, 4, true), base + 16);
        assert_eq!(block_written_back(base, 4, false), base - 16);
    }

    #[test]
    fn block_transfer_address_ignores_low_base_bits_but_writeback_does_not() {
        assert_eq!(aligned_block_base(0x0200_0000), 0x0200_0000);
        assert_eq!(aligned_block_base(0x0200_0001), 0x0200_0000);
        assert_eq!(aligned_block_base(0x0200_0002), 0x0200_0000);
        assert_eq!(aligned_block_base(0x0200_0003), 0x0200_0000);
        // The Design policy keeps writeback on the original base.
        assert_eq!(block_written_back(0x0200_0002, 2, true), 0x0200_000A);
        assert_eq!(block_written_back(0x0200_0002, 2, false), 0x01FF_FFFA);
    }
}
