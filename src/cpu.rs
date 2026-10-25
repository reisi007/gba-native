//! Portable CPU/register/flags state.
//!
//! Models the ARM7TDMI register banking needed by the implemented PSR-transfer
//! subset: r0-r7 are shared, r8-r12 are banked only for FIQ, r13/r14 are banked
//! per exception mode (USER and SYSTEM share one bank), and FIQ/IRQ/SVC/ABT/UND
//! each have a separate SPSR. Exceptions, vector entry, SWI and PSR restore via
//! ALU/PC are absent. Bits reserved on the ARM7TDMI are preserved, never
//! invented or silently zeroed by a field write.

use crate::error::{Error, Result};
use crate::ir::{Cond, PSR_FIELD_C, PSR_FIELD_F, PSR_FIELD_S, PSR_FIELD_X, Psr};
use crate::sha256;

pub const DIRECT_ENTRY_CPSR: u32 = 0x0000_00DF;
pub const MODE_MASK: u32 = 0x1F;
pub const MODE_SYSTEM: u32 = 0x1F;
pub const USER_STACK: u32 = 0x0300_7F00;

pub const FLAG_N: u32 = 1 << 31;
pub const FLAG_Z: u32 = 1 << 30;
pub const FLAG_C: u32 = 1 << 29;
pub const FLAG_V: u32 = 1 << 28;
pub const FLAG_I: u32 = 1 << 7;
pub const FLAG_F: u32 = 1 << 6;
pub const FLAG_T: u32 = 1 << 5;

/// ARM7TDMI condition-code byte. Bits 27-24 are reserved on this CPU.
pub const CPSR_FLAGS_MASK: u32 = 0xF000_0000;
/// Control byte I/F/T/M4-M0. Bits 15-8 are reserved on this CPU.
pub const CPSR_CONTROL_MASK: u32 = 0x0000_00FF;

/// The seven ARM7TDMI modes and their bank layout, from GBATEK
/// `#arm-cpu-register-set` and `#arm-cpu-flags--condition-field-cond`.
/// Old 26-bit compatibility values are rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    User,
    Fiq,
    Irq,
    Svc,
    Abt,
    Und,
    System,
}

impl Mode {
    pub const ALL: [Mode; 7] = [
        Mode::User,
        Mode::Fiq,
        Mode::Irq,
        Mode::Svc,
        Mode::Abt,
        Mode::Und,
        Mode::System,
    ];

    pub fn from_bits(bits: u32) -> Option<Self> {
        Some(match bits & MODE_MASK {
            0x10 => Mode::User,
            0x11 => Mode::Fiq,
            0x12 => Mode::Irq,
            0x13 => Mode::Svc,
            0x17 => Mode::Abt,
            0x1B => Mode::Und,
            0x1F => Mode::System,
            _ => return None,
        })
    }

    pub fn bits(self) -> u32 {
        match self {
            Mode::User => 0x10,
            Mode::Fiq => 0x11,
            Mode::Irq => 0x12,
            Mode::Svc => 0x13,
            Mode::Abt => 0x17,
            Mode::Und => 0x1B,
            Mode::System => 0x1F,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::User => "USER",
            Mode::Fiq => "FIQ",
            Mode::Irq => "IRQ",
            Mode::Svc => "SVC",
            Mode::Abt => "ABT",
            Mode::Und => "UND",
            Mode::System => "SYS",
        }
    }

    pub fn has_spsr(self) -> bool {
        !matches!(self, Mode::User | Mode::System)
    }

    /// Bank slot: USER/SYSTEM share slot 0, then one slot per exception mode.
    fn bank(self) -> usize {
        match self {
            Mode::User | Mode::System => 0,
            Mode::Fiq => 1,
            Mode::Irq => 2,
            Mode::Svc => 3,
            Mode::Abt => 4,
            Mode::Und => 5,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cpu {
    low: [u32; 8],
    high: [u32; 5],
    fiq_high: [u32; 5],
    sp: [u32; 6],
    lr: [u32; 6],
    pc: u32,
    cpsr: u32,
    spsr: [Option<u32>; 6],
}

impl Cpu {
    /// Design: direct-entry state, not the result of executing a BIOS reset.
    pub fn direct_entry() -> Self {
        let mut cpu = Self {
            low: [0; 8],
            high: [0; 5],
            fiq_high: [0; 5],
            sp: [0; 6],
            lr: [0; 6],
            pc: 0x0800_0000,
            cpsr: DIRECT_ENTRY_CPSR,
            // Design: direct-entry exception-mode SPSRs start at zero. USER and
            // SYSTEM have no SPSR at all (slot 0 remains None).
            spsr: [None, Some(0), Some(0), Some(0), Some(0), Some(0)],
        };
        cpu.sp[0] = USER_STACK;
        cpu
    }

    pub fn reg(&self, index: u8) -> u32 {
        match index & 0xF {
            0..=7 => self.low[(index & 0xF) as usize],
            8..=12 => {
                let index = (index & 0xF) as usize - 8;
                if self.mode() == Mode::Fiq {
                    self.fiq_high[index]
                } else {
                    self.high[index]
                }
            }
            13 => self.sp[self.mode().bank()],
            14 => self.lr[self.mode().bank()],
            _ => self.pc,
        }
    }

    pub fn set_reg(&mut self, index: u8, value: u32) {
        match index & 0xF {
            0..=7 => self.low[(index & 0xF) as usize] = value,
            8..=12 => {
                let index = (index & 0xF) as usize - 8;
                if self.mode() == Mode::Fiq {
                    self.fiq_high[index] = value;
                } else {
                    self.high[index] = value;
                }
            }
            13 => self.sp[self.mode().bank()] = value,
            14 => self.lr[self.mode().bank()] = value,
            _ => self.pc = value,
        }
    }

    pub fn pc(&self) -> u32 {
        self.pc
    }

    pub fn set_pc(&mut self, value: u32) {
        self.pc = value;
    }

    pub fn mode(&self) -> Mode {
        Mode::from_bits(self.cpsr).expect("CPSR mode invariant is validated on every write")
    }

    /// The single source of truth for ARM/Thumb state is CPSR bit 5.
    pub fn thumb(&self) -> bool {
        self.cpsr & FLAG_T != 0
    }

    pub fn set_thumb(&mut self, thumb: bool) {
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

    /// Read the selected PSR. USER/SYSTEM have no SPSR; that access is
    /// unpredictable on hardware, so it is an explicit error here instead of a
    /// fabricated value.
    pub fn read_psr(&self, psr: Psr) -> Result<u32> {
        match psr {
            Psr::Cpsr => Ok(self.cpsr),
            Psr::Spsr => self
                .spsr
                .get(self.mode().bank())
                .copied()
                .flatten()
                .ok_or(Error::Psr(
                    "SPSR does not exist in USER/SYSTEM mode; access is unpredictable",
                )),
        }
    }

    /// Apply an MSR field write without mutating on failure. The caller supplies
    /// the expanded immediate or register value. An empty field mask is
    /// unpredictable on the ARM7TDMI (at least one field bit must be set) and is
    /// rejected instead of being reported as a successful no-op.
    pub fn write_psr(&mut self, psr: Psr, fields: u8, value: u32) -> Result<()> {
        if fields & (PSR_FIELD_F | PSR_FIELD_S | PSR_FIELD_X | PSR_FIELD_C) == 0 {
            return Err(Error::Psr(
                "MSR with an empty field mask is unpredictable; at least one field bit is required",
            ));
        }
        match psr {
            Psr::Cpsr => self.write_cpsr(fields, value),
            Psr::Spsr => {
                let bank = self.mode().bank();
                let current = self.spsr[bank].ok_or(Error::Psr(
                    "SPSR does not exist in USER/SYSTEM mode; access is unpredictable",
                ))?;
                if fields & PSR_FIELD_C != 0 {
                    validate_control(current, value)?;
                }
                let next = merge_psr(current, value, fields);
                self.spsr[bank] = Some(next);
                Ok(())
            }
        }
    }

    fn write_cpsr(&mut self, fields: u8, value: u32) -> Result<()> {
        // Documented USER-mode restriction: only the condition-code byte may
        // change; control bits are ignored rather than inventing a failure.
        let fields = if self.mode() == Mode::User {
            fields & !PSR_FIELD_C
        } else {
            if fields & PSR_FIELD_C != 0 {
                validate_control(self.cpsr, value)?;
            }
            fields
        };
        self.cpsr = merge_psr(self.cpsr, value, fields);
        Ok(())
    }

    /// Canonical digest of all architectural CPU state, including inactive
    /// banks and SPSRs. Used to compare native and reference execution beyond
    /// the visible register snapshot.
    pub fn state_digest(&self) -> [u8; 32] {
        let mut bytes = Vec::with_capacity(4 * 32);
        for word in self
            .low
            .iter()
            .chain(self.high.iter())
            .chain(self.fiq_high.iter())
            .chain(self.sp.iter())
            .chain(self.lr.iter())
            .chain([self.pc].iter())
            .chain([self.cpsr].iter())
        {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for spsr in &self.spsr {
            bytes.push(u8::from(spsr.is_some()));
            bytes.extend_from_slice(&spsr.unwrap_or(0).to_le_bytes());
        }
        sha256::digest(&bytes)
    }
}

/// Merge the selected documented PSR field bytes while preserving reserved
/// bits. `s` and `x` address reserved ARM7TDMI bytes and therefore write
/// nothing; `f` only reaches N/Z/C/V and `c` only the control byte.
fn merge_psr(current: u32, value: u32, fields: u8) -> u32 {
    let mut next = current;
    if fields & PSR_FIELD_F != 0 {
        next = (next & !CPSR_FLAGS_MASK) | (value & CPSR_FLAGS_MASK);
    }
    if fields & PSR_FIELD_C != 0 {
        next = (next & !CPSR_CONTROL_MASK) | (value & CPSR_CONTROL_MASK);
    }
    next
}

/// Validate a control-byte write before any state changes. Only the seven
/// ARM7TDMI modes are accepted and the T bit may not be changed by MSR.
fn validate_control(current: u32, value: u32) -> Result<()> {
    if Mode::from_bits(value & MODE_MASK).is_none() {
        return Err(Error::Psr(
            "MSR writes a mode value that is not a valid ARM7TDMI mode",
        ));
    }
    if value & FLAG_T != current & FLAG_T {
        return Err(Error::Psr(
            "MSR cannot change the T bit; ARM/Thumb switching uses BX",
        ));
    }
    Ok(())
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

    #[test]
    fn direct_entry_mode_and_stack_are_preserved() {
        let cpu = Cpu::direct_entry();
        assert_eq!(cpu.cpsr(), DIRECT_ENTRY_CPSR);
        assert_eq!(cpu.mode(), Mode::System);
        assert_eq!(cpu.reg(13), USER_STACK);
        assert_eq!(cpu.reg(15), 0x0800_0000);
        assert!(!cpu.thumb());
        assert!(Mode::ALL.iter().any(|mode| mode.name() == "IRQ"));
    }

    #[test]
    fn state_digest_tracks_inactive_banks() {
        let mut a = Cpu::direct_entry();
        let before = a.state_digest();
        a.write_psr(Psr::Cpsr, PSR_FIELD_C, Mode::Fiq.bits())
            .expect("switch to FIQ");
        a.set_reg(13, 0x1111_1111);
        let fiq_state = a.state_digest();
        assert_ne!(before, fiq_state);
        a.write_psr(Psr::Cpsr, PSR_FIELD_C, Mode::System.bits())
            .expect("switch back");
        assert_eq!(a.reg(13), USER_STACK);
        a.write_psr(Psr::Cpsr, PSR_FIELD_C, Mode::Fiq.bits())
            .expect("switch back to FIQ");
        assert_eq!(a.reg(13), 0x1111_1111);
        assert_eq!(a.state_digest(), fiq_state, "the FIQ bank is preserved");
    }
}
