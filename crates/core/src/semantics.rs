//! Documented operand/shifter semantics shared by the reference interpreter and
//! the native emitter. These are typed helpers, not a decoder and not an
//! interpreter: they never look at a raw instruction word.
//!
//! Sources: GBATEK `#second-operand-op2`, `#using-r15-pc`,
//! `#arm-opcodes-data-processing-alu` and the Thumb ALU/jump sections.

use crate::cpu::{Cpu, FLAG_C};
use crate::ir::{Shift, ShiftKind, Src};

/// PC value read as a data-processing operand. ARM reads `+8`, or `+12` when
/// the instruction uses a register-specified shift; Thumb reads `+4`.
pub fn operand_pc(addr: u32, thumb: bool, register_shift: bool) -> u32 {
    if thumb {
        addr.wrapping_add(4)
    } else {
        addr.wrapping_add(if register_shift { 12 } else { 8 })
    }
}

/// Read a general register or the documented pipeline PC value.
pub fn read_register(cpu: &Cpu, addr: u32, thumb: bool, index: u8, register_shift: bool) -> u32 {
    if index == 15 {
        operand_pc(addr, thumb, register_shift)
    } else {
        cpu.reg(index)
    }
}

/// ARM/Thumb PC write through MOV/ALU: the PC is force-aligned, but the
/// instruction does not change the ARM/Thumb state (only BX does).
pub fn aligned_pc(value: u32, thumb: bool) -> u32 {
    if thumb { value & !1 } else { value & !3 }
}

fn carry_in(cpu: &Cpu) -> bool {
    cpu.cpsr() & FLAG_C != 0
}

/// Shift by an encoded immediate. A zero amount has the documented special
/// meaning per operation: LSL#0 leaves C unchanged, LSR#0/ASR#0 are 32, and
/// ROR#0 is RRX. The returned `Option<bool>` is the shifter-carry update;
/// `None` means "leave C unchanged".
pub fn shift_immediate(
    kind: ShiftKind,
    amount: u8,
    value: u32,
    carry_in: bool,
) -> (u32, Option<bool>) {
    let amount = amount.min(31);
    match kind {
        ShiftKind::Lsl => {
            if amount == 0 {
                (value, None)
            } else {
                let carried = (value >> (32 - u32::from(amount))) & 1 != 0;
                (value << amount, Some(carried))
            }
        }
        ShiftKind::Lsr => {
            let count = if amount == 0 { 32 } else { u32::from(amount) };
            let carried = (value >> (count - 1)) & 1 != 0;
            let result = if count == 32 { 0 } else { value >> count };
            (result, Some(carried))
        }
        ShiftKind::Asr => {
            let count = if amount == 0 { 32 } else { u32::from(amount) };
            let result = ((value as i32) >> (count - 1)) >> 1;
            let carried = (value >> (count - 1)) & 1 != 0;
            (result as u32, Some(carried))
        }
        ShiftKind::Ror => {
            if amount == 0 {
                let result = (u32::from(carry_in) << 31) | (value >> 1);
                (result, Some(value & 1 != 0))
            } else {
                let carried = (value >> (u32::from(amount) - 1)) & 1 != 0;
                (value.rotate_right(u32::from(amount)), Some(carried))
            }
        }
    }
}

/// Shift by a register's low eight bits. Amount zero has the documented
/// no-shift meaning for LSL/LSR/ASR/ROR (C unchanged); 32 and larger follow the
/// ARM7TDMI rules. Consumption of only the low byte is deliberate.
pub fn shift_register(
    kind: ShiftKind,
    amount: u32,
    value: u32,
    _carry_in: bool,
) -> (u32, Option<bool>) {
    let amount = amount & 0xFF;
    match kind {
        ShiftKind::Lsl => match amount {
            0 => (value, None),
            1..=31 => (value << amount, Some((value >> (32 - amount)) & 1 != 0)),
            32 => (0, Some(value & 1 != 0)),
            _ => (0, Some(false)),
        },
        ShiftKind::Lsr => match amount {
            0 => (value, None),
            1..=31 => (value >> amount, Some((value >> (amount - 1)) & 1 != 0)),
            32 => (0, Some(value >> 31 != 0)),
            _ => (0, Some(false)),
        },
        ShiftKind::Asr => match amount {
            0 => (value, None),
            1..=31 => (
                (((value as i32) >> (amount - 1)) >> 1) as u32,
                Some((value >> (amount - 1)) & 1 != 0),
            ),
            _ => (((value as i32) >> 31) as u32, Some(value >> 31 != 0)),
        },
        ShiftKind::Ror => {
            if amount == 0 {
                (value, None)
            } else {
                let rotate = amount % 32;
                if rotate == 0 {
                    (value, Some(value >> 31 != 0))
                } else {
                    (
                        value.rotate_right(rotate),
                        Some((value >> (rotate - 1)) & 1 != 0),
                    )
                }
            }
        }
    }
}

/// Evaluate a shifted register operand using the current register value and the
/// current carry flag. The shifter carry is computed from the old C where the
/// operation depends on it (RRX), before any flag update by the caller.
pub fn shifted_register(
    cpu: &Cpu,
    addr: u32,
    thumb: bool,
    rm: u8,
    shift: Shift,
) -> (u32, Option<bool>) {
    let register_shift = shift.is_register_controlled();
    let value = read_register(cpu, addr, thumb, rm, register_shift);
    let carry = carry_in(cpu);
    match shift {
        Shift::Imm { kind, amount } => shift_immediate(kind, amount, value, carry),
        Shift::Reg { kind, rm: rs } => shift_register(kind, cpu.reg(rs) & 0xFF, value, carry),
    }
}

/// Evaluate any typed data-processing operand.
pub fn operand(
    cpu: &Cpu,
    addr: u32,
    thumb: bool,
    src: Src,
    register_shift: bool,
) -> (u32, Option<bool>) {
    match src {
        Src::Reg(rm) => (read_register(cpu, addr, thumb, rm, register_shift), None),
        Src::Imm { value, carry } => (value, carry),
        Src::RegShift { rm, shift } => shifted_register(cpu, addr, thumb, rm, shift),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_register_shift_boundaries() {
        let value = 0x8000_0001u32;
        assert_eq!(
            shift_register(ShiftKind::Lsl, 0, value, false),
            (value, None)
        );
        assert_eq!(
            shift_register(ShiftKind::Lsl, 32, value, false),
            (0, Some(true))
        );
        assert_eq!(
            shift_register(ShiftKind::Lsl, 33, value, false),
            (0, Some(false))
        );
        assert_eq!(
            shift_register(ShiftKind::Lsr, 32, value, false),
            (0, Some(true))
        );
        assert_eq!(
            shift_register(ShiftKind::Asr, 64, value, false),
            (0xFFFF_FFFF, Some(true))
        );
        assert_eq!(
            shift_register(ShiftKind::Ror, 32, value, false),
            (value, Some(true))
        );
        assert_eq!(
            shift_register(ShiftKind::Ror, 33, value, false),
            (0xC000_0000, Some(true))
        );
        assert_eq!(
            shift_register(ShiftKind::Ror, 256, value, false),
            (value, None),
            "the shift amount is only the low eight bits"
        );
    }

    #[test]
    fn immediate_specials_and_rrx() {
        let value = 0x8000_0001u32;
        assert_eq!(
            shift_immediate(ShiftKind::Lsl, 0, value, true),
            (value, None)
        );
        assert_eq!(
            shift_immediate(ShiftKind::Lsr, 0, value, false),
            (0, Some(true))
        );
        assert_eq!(
            shift_immediate(ShiftKind::Asr, 0, value, false),
            (0xFFFF_FFFF, Some(true))
        );
        assert_eq!(
            shift_immediate(ShiftKind::Ror, 0, value, true),
            (0xC000_0000, Some(true))
        );
        assert_eq!(
            shift_immediate(ShiftKind::Ror, 0, value, false),
            (0x4000_0000, Some(true))
        );
    }
}
