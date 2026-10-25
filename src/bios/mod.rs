//! Independently implemented, documentation-driven BIOS HLE foundation.
//!
//! This module dispatches a small set of documented, timing-independent GBA
//! BIOS services directly from Rust. It is **not** SWI/SVC exception entry,
//! BIOS boot, a BIOS image reader, or playability support: the caller supplies
//! the service number and register state, and no instruction is consumed and
//! no PC is advanced here. A later SWI integration owns the instruction budget,
//! next-PC update and exception framing.
//!
//! All service implementations are derived from the public hardware interface
//! documented in GBATEK; see `docs/clean-room/hle-services.md`. No Nintendo
//! BIOS bytes are read, no emulator implementation is consulted, and no BIOS
//! side effects outside the documented service outputs are invented.

mod arithmetic;
mod memory;

use crate::bus::Bus;
use crate::cpu::Cpu;
use crate::error::{Error, Result};
use std::fmt;

/// GBA SWI 01h: reset selected memory regions and force blank.
pub const SERVICE_REGISTER_RAM_RESET: u8 = 0x01;
/// GBA SWI 06h: signed division, numerator in r0, denominator in r1.
pub const SERVICE_DIV: u8 = 0x06;
/// GBA SWI 07h: signed division, numerator in r1, denominator in r0.
pub const SERVICE_DIV_ARM: u8 = 0x07;
/// GBA SWI 08h: unsigned integer square root of r0.
pub const SERVICE_SQRT: u8 = 0x08;
/// GBA SWI 0Bh: halfword/word memory copy or fill.
pub const SERVICE_CPU_SET: u8 = 0x0B;
/// GBA SWI 0Ch: word memory copy or fill in 8-word blocks.
pub const SERVICE_CPU_FAST_SET: u8 = 0x0C;

/// Outcome of an unsupported or invalid BIOS service request.
///
/// Every non-success is explicit and named; an unsupported or waiting service
/// is never reported as a successful no-op. Register mutations of division
/// services happen only after the complete result is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BiosError {
    /// No HLE implementation exists for this service number. This includes the
    /// documented timing services (Halt `02h`, IntrWait `04h`,
    /// VBlankIntrWait `05h`), which cannot be modelled without IRQ/timing.
    UnsupportedService(u8),
    /// Signed division by zero. Returned before any register is changed.
    ZeroDivisor,
    /// `i32::MIN / -1` has no representable signed quotient. Returned before
    /// any register is changed instead of computing a wrapped value or
    /// panicking.
    DivisionOverflow,
    /// A source or destination address is not aligned for the selected unit.
    Misaligned { service: u8, addr: u32, align: u8 },
    /// The control word contains bits reserved by this service.
    ReservedControl { service: u8, control: u32 },
    /// Checked expansion of the unit count overflowed the 32-bit address space.
    RangeOverflow { service: u8, addr: u32, units: u32 },
    /// The expanded range crosses or leaves its documented bus region.
    RangeOutsideRegion { service: u8, addr: u32, end: u32 },
    /// The source range is not in a read-capable region.
    UnsupportedSource { service: u8, addr: u32 },
    /// The destination range is not in a write-capable memory region.
    UnsupportedDestination { service: u8, addr: u32 },
    /// `RegisterRamReset` requested a reset beyond the implemented memory bits.
    UnsupportedResetFlags { flags: u32 },
}

impl fmt::Display for BiosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BiosError::UnsupportedService(service) => {
                write!(f, "service 0x{service:02X} is not implemented")
            }
            BiosError::ZeroDivisor => {
                write!(f, "signed division by zero; no registers changed")
            }
            BiosError::DivisionOverflow => write!(
                f,
                "signed quotient i32::MIN / -1 is not representable; no registers changed"
            ),
            BiosError::Misaligned {
                service,
                addr,
                align,
            } => write!(
                f,
                "service 0x{service:02X}: address 0x{addr:08X} is not aligned to {align} bytes"
            ),
            BiosError::ReservedControl { service, control } => write!(
                f,
                "service 0x{service:02X}: control 0x{control:08X} sets reserved bits"
            ),
            BiosError::RangeOverflow {
                service,
                addr,
                units,
            } => write!(
                f,
                "service 0x{service:02X}: range at 0x{addr:08X} with {units} units overflows the address space"
            ),
            BiosError::RangeOutsideRegion { service, addr, end } => write!(
                f,
                "service 0x{service:02X}: range 0x{addr:08X}..=0x{end:08X} crosses or leaves a supported bus region"
            ),
            BiosError::UnsupportedSource { service, addr } => write!(
                f,
                "service 0x{service:02X}: source range at 0x{addr:08X} is not readable"
            ),
            BiosError::UnsupportedDestination { service, addr } => write!(
                f,
                "service 0x{service:02X}: destination range at 0x{addr:08X} is not writable memory"
            ),
            BiosError::UnsupportedResetFlags { flags } => write!(
                f,
                "RegisterRamReset flags 0x{flags:08X} request unsupported reset bits 5..31"
            ),
        }
    }
}

impl std::error::Error for BiosError {}

/// Dispatch one documented BIOS service.
///
/// This primitive interprets the service number exactly as supplied. It does
/// not consume an instruction, advance `PC`, change CPSR/mode/T/NZCV/SPSR, or
/// charge hardware cycles. Registers outside the documented service outputs are
/// preserved as a **Design** choice, not as a claim about a particular BIOS
/// implementation.
pub fn call(service: u8, cpu: &mut Cpu, bus: &mut Bus) -> Result<()> {
    match service {
        SERVICE_REGISTER_RAM_RESET => memory::register_ram_reset(cpu, bus),
        SERVICE_DIV => arithmetic::div(cpu),
        SERVICE_DIV_ARM => arithmetic::div_arm(cpu),
        SERVICE_SQRT => arithmetic::sqrt(cpu),
        SERVICE_CPU_SET => memory::cpu_set(cpu, bus),
        SERVICE_CPU_FAST_SET => memory::cpu_fast_set(cpu, bus),
        other => Err(Error::Bios(BiosError::UnsupportedService(other))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cart::Cartridge;

    fn bus() -> Bus {
        Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"))
    }

    #[test]
    fn unsupported_services_are_explicit_and_mutate_nothing() {
        let mut cpu = Cpu::direct_entry();
        let mut bus = bus();
        cpu.set_reg(0, 0x1234_5678);
        cpu.set_reg(1, 0x9ABC_DEF0);
        let before = (cpu.state_digest(), bus.state_digest());
        for service in [
            0x00u8, 0x02, 0x04, 0x05, 0x09, 0x10, 0x11, 0x12, 0x14, 0x15, 0xFF,
        ] {
            assert_eq!(
                call(service, &mut cpu, &mut bus),
                Err(Error::Bios(BiosError::UnsupportedService(service)))
            );
        }
        assert_eq!(before, (cpu.state_digest(), bus.state_digest()));
    }
}
