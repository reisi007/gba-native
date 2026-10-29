//! SWI 0Fh: documented `ObjAffineSet`, explicit opt-in.
//!
//! The interface (source struct, destination parameter order, angle and offset
//! semantics) is derived from the public GBATEK section
//! `#swi-0fh-gba---objaffineset`. The 256-entry Q12 sine table is generated
//! from the mathematical sine at the documented 8-bit angle resolution; it is
//! not copied from a BIOS image. The HLE therefore does not claim bit-identical
//! results to a particular BIOS sine table.
//!
//! Destination layout: one calculation writes PA at `dest`, PB at
//! `dest + offset`, PC at `dest + 2*offset` and PD at `dest + 3*offset`; the
//! next calculation starts `4*offset` bytes later. `offset=2` therefore stores
//! the four parameters contiguously, and `offset=8` addresses one parameter per
//! 8-byte OAM slot, matching the documented OAM structure.
//!
//! The service is host-selected configuration, not guest state: the selected
//! policy is excluded from [`crate::bus::Bus::state_digest`].

use super::target::target_of;
use crate::bios::BiosError;
use crate::bus::Bus;
use crate::cpu::Cpu;
use crate::error::{Error, Result};

mod table;

/// GBA SWI 0Fh: `ObjAffineSet`.
pub(super) const SERVICE_OBJ_AFFINE_SET: u8 = 0x0F;
const SOURCE_ENTRY_BYTES: u32 = 6;
const PARAMETERS_PER_ENTRY: u32 = 4;
const Q12_SHIFT: u32 = 12;

/// Host-selected policy for the documented `ObjAffineSet` service.
///
/// `Strict` keeps SWI `0Fh` an explicit unsupported-service error, preserving
/// the six-service default contract. `Documented` enables only the documented
/// affine calculation and destination layout implemented in this module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AffinePolicy {
    /// Fail closed: SWI `0Fh` remains an explicit unsupported-service error.
    Strict,
    /// **Experimental opt-in:** documented `ObjAffineSet` calculation.
    Documented,
}

impl AffinePolicy {
    /// Stable label for CLI output and snapshot reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Documented => "documented",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Plan {
    source: u32,
    destination: u32,
    count: u32,
    offset: u32,
}

pub(super) fn validate_call(cpu: &Cpu, bus: &Bus) -> Result<()> {
    plan(cpu, bus).map(|_| ())
}

pub(super) fn call(cpu: &mut Cpu, bus: &mut Bus) -> Result<()> {
    let plan = plan(cpu, bus)?;
    execute(&plan, bus)
}

fn plan(cpu: &Cpu, bus: &Bus) -> Result<Plan> {
    if bus.affine_policy() == AffinePolicy::Strict {
        return Err(Error::Bios(BiosError::UnsupportedService(
            SERVICE_OBJ_AFFINE_SET,
        )));
    }
    let source = cpu.reg(0);
    let destination = cpu.reg(1);
    let count = cpu.reg(2);
    let offset = cpu.reg(3);
    require_alignment(source, 2)?;
    require_alignment(destination, 2)?;
    if offset < 2 || !offset.is_multiple_of(2) {
        return Err(Error::Bios(BiosError::UnsupportedParameter {
            service: SERVICE_OBJ_AFFINE_SET,
            value: offset,
            reason: "offset must be an even byte count of at least 2",
        }));
    }
    if count == 0 {
        return Ok(Plan {
            source,
            destination,
            count,
            offset,
        });
    }
    // Source entries are six bytes each; reuse the bounded HLE range rules.
    super::memory::validate_source(
        bus,
        SERVICE_OBJ_AFFINE_SET,
        source,
        count,
        SOURCE_ENTRY_BYTES,
    )?;
    let span = count
        .checked_mul(PARAMETERS_PER_ENTRY)
        .and_then(|value| value.checked_sub(1))
        .and_then(|value| value.checked_mul(offset))
        .and_then(|value| value.checked_add(2))
        .ok_or(Error::Bios(BiosError::RangeOverflow {
            service: SERVICE_OBJ_AFFINE_SET,
            addr: destination,
            units: count,
        }))?;
    let end = destination
        .checked_add(span - 1)
        .ok_or(Error::Bios(BiosError::RangeOverflow {
            service: SERVICE_OBJ_AFFINE_SET,
            addr: destination,
            units: count,
        }))?;
    let target = target_of(destination).ok_or(Error::Bios(BiosError::UnsupportedDestination {
        service: SERVICE_OBJ_AFFINE_SET,
        addr: destination,
    }))?;
    if target_of(end) != Some(target) {
        return Err(Error::Bios(BiosError::RangeOutsideRegion {
            service: SERVICE_OBJ_AFFINE_SET,
            addr: destination,
            end,
        }));
    }
    Ok(Plan {
        source,
        destination,
        count,
        offset,
    })
}

fn require_alignment(addr: u32, align: u32) -> Result<()> {
    if addr.is_multiple_of(align) {
        Ok(())
    } else {
        Err(Error::Bios(BiosError::Misaligned {
            service: SERVICE_OBJ_AFFINE_SET,
            addr,
            align: align as u8,
        }))
    }
}

fn execute(plan: &Plan, bus: &mut Bus) -> Result<()> {
    let mut destination = plan.destination;
    for calculation in 0..plan.count {
        let source = plan.source + calculation * SOURCE_ENTRY_BYTES;
        let scale_x = i32::from(bus.read16(source)? as i16);
        let scale_y = i32::from(bus.read16(source + 2)? as i16);
        let angle = bus.read16(source + 4)?;
        let index = usize::from((angle >> 8) as u8);
        let sin = i32::from(table::SINE_Q12[index]);
        let cos = i32::from(table::SINE_Q12[(index + 64) & 0xFF]);
        // The products fit in i32: |s16| * 4096 <= 2^27. The final 16-bit
        // store keeps the low halfword, like a hardware STRH.
        let parameters = [
            (scale_x * cos) >> Q12_SHIFT,
            (scale_x * -sin) >> Q12_SHIFT,
            (scale_y * sin) >> Q12_SHIFT,
            (scale_y * cos) >> Q12_SHIFT,
        ];
        for (index, value) in parameters.iter().enumerate() {
            bus.write16(destination + index as u32 * plan.offset, *value as u16)?;
        }
        destination += PARAMETERS_PER_ENTRY * plan.offset;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
