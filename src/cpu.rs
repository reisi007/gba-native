//! Portable CPU/register/flags state.
//!
//! Only the documented direct-entry subset is modeled: system mode, ARM/Thumb
//! state and NZCV. Exceptions, banking, PSR transfers and BIOS SWIs are absent.

use crate::ir::Cond;

pub const DIRECT_ENTRY_CPSR: u32 = 0x0000_00DF;
pub const MODE_SYSTEM: u32 = 0x1F;
pub const USER_STACK: u32 = 0x0300_7F00;

pub const FLAG_N: u32 = 1 << 31;
pub const FLAG_Z: u32 = 1 << 30;
pub const FLAG_C: u32 = 1 << 29;
pub const FLAG_V: u32 = 1 << 28;
pub const FLAG_T: u32 = 1 << 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cpu {
    regs: [u32; 16],
    cpsr: u32,
    thumb: bool,
}

impl Cpu {
    /// Design: direct-entry state, not the result of executing a BIOS reset.
    pub fn direct_entry() -> Self {
        let mut regs = [0u32; 16];
        regs[13] = USER_STACK;
        regs[15] = 0x0800_0000;
        Self {
            regs,
            cpsr: DIRECT_ENTRY_CPSR,
            thumb: false,
        }
    }

    pub fn reg(&self, index: u8) -> u32 {
        self.regs[(index & 0xF) as usize]
    }

    pub fn set_reg(&mut self, index: u8, value: u32) {
        self.regs[(index & 0xF) as usize] = value;
    }

    pub fn pc(&self) -> u32 {
        self.regs[15]
    }

    pub fn set_pc(&mut self, value: u32) {
        self.regs[15] = value;
    }

    pub fn thumb(&self) -> bool {
        self.thumb
    }

    pub fn set_thumb(&mut self, thumb: bool) {
        self.thumb = thumb;
        if thumb {
            self.cpsr |= FLAG_T;
        } else {
            self.cpsr &= !FLAG_T;
        }
    }

    pub fn cpsr(&self) -> u32 {
        self.cpsr
    }

    pub fn flags(&self) -> (bool, bool, bool, bool) {
        (
            self.cpsr & FLAG_N != 0,
            self.cpsr & FLAG_Z != 0,
            self.cpsr & FLAG_C != 0,
            self.cpsr & FLAG_V != 0,
        )
    }

    pub fn set_nzcv(&mut self, n: bool, z: bool, c: bool, v: bool) {
        self.set_flag(FLAG_N, n);
        self.set_flag(FLAG_Z, z);
        self.set_flag(FLAG_C, c);
        self.set_flag(FLAG_V, v);
    }

    /// Logical operations update N and Z, optionally C (shifter carry), and
    /// preserve V.
    pub fn set_logic_flags(&mut self, result: u32, carry: Option<bool>) {
        self.set_flag(FLAG_N, result & 0x8000_0000 != 0);
        self.set_flag(FLAG_Z, result == 0);
        if let Some(carry) = carry {
            self.set_flag(FLAG_C, carry);
        }
    }

    /// ADD/SUB/CMP flag update from `add_flags`/`sub_flags`.
    pub fn set_arith_flags(&mut self, result: u32, carry_or_no_borrow: bool, overflow: bool) {
        self.set_flag(FLAG_N, result & 0x8000_0000 != 0);
        self.set_flag(FLAG_Z, result == 0);
        self.set_flag(FLAG_C, carry_or_no_borrow);
        self.set_flag(FLAG_V, overflow);
    }

    fn set_flag(&mut self, flag: u32, value: bool) {
        if value {
            self.cpsr |= flag;
        } else {
            self.cpsr &= !flag;
        }
    }
}

pub fn condition_holds(cpsr: u32, cond: Cond) -> bool {
    let n = cpsr & FLAG_N != 0;
    let z = cpsr & FLAG_Z != 0;
    let c = cpsr & FLAG_C != 0;
    let v = cpsr & FLAG_V != 0;
    match cond {
        Cond::Eq => z,
        Cond::Ne => !z,
        Cond::Cs => c,
        Cond::Cc => !c,
        Cond::Mi => n,
        Cond::Pl => !n,
        Cond::Vs => v,
        Cond::Vc => !v,
        Cond::Hi => c && !z,
        Cond::Ls => !c || z,
        Cond::Ge => n == v,
        Cond::Lt => n != v,
        Cond::Gt => !z && n == v,
        Cond::Le => z || n != v,
        Cond::Al => true,
    }
}

/// Returns `(result, unsigned carry, signed overflow)`.
pub fn add_flags(a: u32, b: u32) -> (u32, bool, bool) {
    let result = a.wrapping_add(b);
    let carry = result < a;
    let overflow = ((a ^ result) & (b ^ result) & 0x8000_0000) != 0;
    (result, carry, overflow)
}

/// Returns `(result, no-borrow carry, signed overflow)`.
pub fn sub_flags(a: u32, b: u32) -> (u32, bool, bool) {
    let result = a.wrapping_sub(b);
    let no_borrow = a >= b;
    let overflow = ((a ^ b) & (a ^ result) & 0x8000_0000) != 0;
    (result, no_borrow, overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_flags() {
        assert_eq!(add_flags(0xFFFF_FFFF, 1), (0, true, false));
        assert_eq!(add_flags(0x7FFF_FFFF, 1), (0x8000_0000, false, true));
        assert_eq!(sub_flags(1, 2), (0xFFFF_FFFF, false, false));
        assert_eq!(sub_flags(0x8000_0000, 1), (0x7FFF_FFFF, true, true));
    }

    #[test]
    fn conditions_use_nzcv() {
        let mut cpu = Cpu::direct_entry();
        cpu.set_nzcv(false, true, true, false);
        assert!(condition_holds(cpu.cpsr(), Cond::Eq));
        assert!(condition_holds(cpu.cpsr(), Cond::Cs));
        assert!(!condition_holds(cpu.cpsr(), Cond::Hi));
        assert!(!condition_holds(cpu.cpsr(), Cond::Lt));
        cpu.set_nzcv(false, false, true, false);
        assert!(condition_holds(cpu.cpsr(), Cond::Hi));
        cpu.set_nzcv(true, false, false, false);
        assert!(condition_holds(cpu.cpsr(), Cond::Mi));
        assert!(!condition_holds(cpu.cpsr(), Cond::Ge));
        assert!(condition_holds(cpu.cpsr(), Cond::Lt));
        cpu.set_nzcv(true, false, false, true);
        assert!(condition_holds(cpu.cpsr(), Cond::Ge));
    }
}
