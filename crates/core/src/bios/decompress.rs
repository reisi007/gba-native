//! SWI 11h/12h/14h/15h/16h/17h/18h: documented decompression HLE.
//!
//! **Explicit opt-in, not part of the six-service default.** The formats are
//! reimplemented from the public GBATEK descriptions in
//! `docs/clean-room/hle-services.md`; no BIOS byte, code or disassembly is read
//! or consulted.
//!
//! The HLE rebuilds the documented output bytes in a host buffer before the
//! first destination store, so a decode failure (invalid header/back-reference
//! or an exhausted readable source) leaves the destination untouched. A
//! structurally valid stream that simply describes different bytes cannot be
//! detected without a checksum. The final store is not transactional: a bus
//! failure part-way through it leaves the bytes already written.
//!
//! Write units follow the documented Wram/Vram split. Byte variants accept only
//! byte-writable EWRAM/IWRAM; halfword variants require a halfword-aligned
//! destination in EWRAM/IWRAM/palette/VRAM/OAM and an even decompressed size,
//! because no other write unit is documented for them. The LZ77 halfword
//! variant rejects `disp=0`: GBATEK states that the Vram function works only
//! with `disp=001h..FFFh`, not `disp=000h`.

use crate::bus::Bus;
use crate::cpu::Cpu;
use crate::error::{Error, Result};

/// GBA SWI 11h: LZ77, 8-bit destination stores.
pub(super) const SERVICE_LZ77_8: u8 = 0x11;
/// GBA SWI 12h: LZ77, 16-bit destination stores (VRAM variant).
pub(super) const SERVICE_LZ77_16: u8 = 0x12;
/// GBA SWI 14h: run-length, 8-bit destination stores.
pub(super) const SERVICE_RL_8: u8 = 0x14;
/// GBA SWI 15h: run-length, 16-bit destination stores (VRAM variant).
pub(super) const SERVICE_RL_16: u8 = 0x15;
/// GBA SWI 16h: 8-bit diff filter, 8-bit destination stores.
pub(super) const SERVICE_DIFF8_8: u8 = 0x16;
/// GBA SWI 17h: 8-bit diff filter, 16-bit destination stores.
pub(super) const SERVICE_DIFF8_16: u8 = 0x17;
/// GBA SWI 18h: 16-bit diff filter, 16-bit destination stores.
pub(super) const SERVICE_DIFF16_16: u8 = 0x18;

const HEADER_BYTES: u32 = 4;
const LZ77_TYPE: u8 = 1;
const RUN_LENGTH_TYPE: u8 = 3;
const DIFF_TYPE: u8 = 8;

/// Host-selected policy for the documented decompression SWIs.
///
/// The prototype's HLE contract is six services by default. This policy keeps
/// that boundary fail-closed: `Strict` reports SWI `10h..19h` decompression
/// numbers as [`crate::bios::BiosError::UnsupportedService`]. The explicit
/// `DocumentedFormats` opt-in enables only the formats and write-unit rules
/// implemented in this module.
///
/// This is host configuration, not guest state, and is excluded from
/// [`Bus::state_digest`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecompressionPolicy {
    /// Fail closed: the decompression SWIs remain explicit unsupported errors.
    Strict,
    /// **Experimental opt-in:** documented LZ77, run-length and diff formats.
    DocumentedFormats,
}

impl DecompressionPolicy {
    /// Stable label for CLI output and snapshot reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::DocumentedFormats => "documented-formats",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Format {
    Lz77,
    RunLength,
    Diff8,
    Diff16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WriteUnit {
    Byte,
    Halfword,
}

#[derive(Clone, Copy, Debug)]
struct Plan {
    service: u8,
    source: u32,
    destination: u32,
    size: u32,
    format: Format,
    unit: WriteUnit,
}

/// Validates the service policy, header and destination range without touching
/// the destination. Called before the SWI exception frame is created.
pub(super) fn validate_call(service: u8, cpu: &Cpu, bus: &Bus) -> Result<()> {
    plan(service, cpu, bus).map(|_| ())
}

/// Executes one decompression service after the SWI frame has been entered.
pub(super) fn call(service: u8, cpu: &mut Cpu, bus: &mut Bus) -> Result<()> {
    let plan = plan(service, cpu, bus)?;
    let output = decode(&plan, bus)?;
    store(bus, plan.destination, &output, plan.unit)
}

fn plan(service: u8, cpu: &Cpu, bus: &Bus) -> Result<Plan> {
    if bus.decompression_policy() == DecompressionPolicy::Strict {
        return Err(Error::Bios(crate::bios::BiosError::UnsupportedService(
            service,
        )));
    }
    let (format, unit) = match service {
        SERVICE_LZ77_8 => (Format::Lz77, WriteUnit::Byte),
        SERVICE_LZ77_16 => (Format::Lz77, WriteUnit::Halfword),
        SERVICE_RL_8 => (Format::RunLength, WriteUnit::Byte),
        SERVICE_RL_16 => (Format::RunLength, WriteUnit::Halfword),
        SERVICE_DIFF8_8 => (Format::Diff8, WriteUnit::Byte),
        SERVICE_DIFF8_16 => (Format::Diff8, WriteUnit::Halfword),
        SERVICE_DIFF16_16 => (Format::Diff16, WriteUnit::Halfword),
        other => {
            return Err(Error::Bios(crate::bios::BiosError::UnsupportedService(
                other,
            )));
        }
    };
    let source = cpu.reg(0);
    let destination = cpu.reg(1);
    require_alignment(service, source, 4)?;
    let header = bus.read32(source)?;
    let (expected_type, expected_nibble): (u8, u8) = match format {
        Format::Lz77 => (LZ77_TYPE, 0),
        Format::RunLength => (RUN_LENGTH_TYPE, 0),
        Format::Diff8 => (DIFF_TYPE, 1),
        Format::Diff16 => (DIFF_TYPE, 2),
    };
    if (header >> 4) & 0xF != u32::from(expected_type) || header & 0xF != u32::from(expected_nibble)
    {
        return Err(Error::Bios(crate::bios::BiosError::DecompressHeader {
            service,
            header,
            expected_type,
            expected_nibble,
        }));
    }
    let size = header >> 8;
    validate_destination(service, destination, size, unit)?;
    let source = source.checked_add(HEADER_BYTES).ok_or(Error::Bios(
        crate::bios::BiosError::RangeOverflow {
            service,
            addr: source,
            units: 1,
        },
    ))?;
    Ok(Plan {
        service,
        source,
        destination,
        size,
        format,
        unit,
    })
}

fn require_alignment(service: u8, addr: u32, align: u32) -> Result<()> {
    if addr.is_multiple_of(align) {
        Ok(())
    } else {
        Err(Error::Bios(crate::bios::BiosError::Misaligned {
            service,
            addr,
            align: align as u8,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Target {
    Ewram,
    Iwram,
    Palette,
    Vram,
    Oam,
}

fn validate_destination(service: u8, destination: u32, size: u32, unit: WriteUnit) -> Result<()> {
    if unit == WriteUnit::Halfword {
        require_alignment(service, destination, 2)?;
        if !size.is_multiple_of(2) {
            return Err(Error::Bios(crate::bios::BiosError::DecompressUnitSize {
                service,
                size,
            }));
        }
    }
    if size == 0 {
        // A zero-length request still names a destination; do not turn an
        // unsupported address into a vacuous success.
        let target = target_of(destination).ok_or(Error::Bios(
            crate::bios::BiosError::UnsupportedDestination {
                service,
                addr: destination,
            },
        ))?;
        if unit == WriteUnit::Byte && !matches!(target, Target::Ewram | Target::Iwram) {
            return Err(Error::Bios(
                crate::bios::BiosError::UnsupportedDestination {
                    service,
                    addr: destination,
                },
            ));
        }
        return Ok(());
    }
    let end = destination.checked_add(size - 1).ok_or(Error::Bios(
        crate::bios::BiosError::RangeOverflow {
            service,
            addr: destination,
            units: size,
        },
    ))?;
    let target = target_of(destination).ok_or(Error::Bios(
        crate::bios::BiosError::UnsupportedDestination {
            service,
            addr: destination,
        },
    ))?;
    if target_of(end) != Some(target) {
        return Err(Error::Bios(crate::bios::BiosError::RangeOutsideRegion {
            service,
            addr: destination,
            end,
        }));
    }
    if unit == WriteUnit::Byte && !matches!(target, Target::Ewram | Target::Iwram) {
        // Palette, VRAM and OAM are 16-bit devices; GBATEK documents the
        // 8-bit-write decompressors for RAM and calls out VRAM as unsupported.
        return Err(Error::Bios(
            crate::bios::BiosError::UnsupportedDestination {
                service,
                addr: destination,
            },
        ));
    }
    Ok(())
}

fn target_of(addr: u32) -> Option<Target> {
    // EWRAM and IWRAM are mirrored across their full 16 MiB CPU windows.
    if (0x0200_0000..0x0300_0000).contains(&addr) {
        return Some(Target::Ewram);
    }
    if (0x0300_0000..0x0400_0000).contains(&addr) {
        return Some(Target::Iwram);
    }
    if (0x0500_0000..0x0500_0400).contains(&addr) {
        return Some(Target::Palette);
    }
    if (0x0600_0000..0x0601_8000).contains(&addr) {
        return Some(Target::Vram);
    }
    if (0x0700_0000..0x0700_0400).contains(&addr) {
        return Some(Target::Oam);
    }
    None
}

struct Source<'a> {
    bus: &'a Bus,
    service: u8,
    addr: u32,
}

impl Source<'_> {
    fn read8(&mut self) -> Result<u8> {
        let value = self.bus.read8(self.addr)?;
        self.addr =
            self.addr
                .checked_add(1)
                .ok_or(Error::Bios(crate::bios::BiosError::RangeOverflow {
                    service: self.service,
                    addr: self.addr,
                    units: 1,
                }))?;
        Ok(value)
    }
}

fn decode(plan: &Plan, bus: &Bus) -> Result<Vec<u8>> {
    let mut source = Source {
        bus,
        service: plan.service,
        addr: plan.source,
    };
    match plan.format {
        Format::Lz77 => lz77(&mut source, plan),
        Format::RunLength => run_length(&mut source, plan.size),
        Format::Diff8 => diff8(&mut source, plan.size),
        Format::Diff16 => diff16(&mut source, plan.size),
    }
}

fn lz77(source: &mut Source<'_>, plan: &Plan) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(plan.size as usize);
    while (output.len() as u32) < plan.size {
        let flags = source.read8()?;
        for bit in (0..8).rev() {
            if (output.len() as u32) >= plan.size {
                break;
            }
            if flags & (1 << bit) == 0 {
                output.push(source.read8()?);
                continue;
            }
            let first = source.read8()?;
            let second = source.read8()?;
            let displacement = (u32::from(first & 0x0F) << 8) | u32::from(second);
            let length = usize::from(first >> 4) + 3;
            if displacement == 0 && plan.service == SERVICE_LZ77_16 {
                return Err(Error::Bios(
                    crate::bios::BiosError::DecompressZeroDisplacement {
                        service: plan.service,
                    },
                ));
            }
            if displacement >= output.len() as u32 {
                return Err(Error::Bios(
                    crate::bios::BiosError::DecompressBackReference {
                        service: plan.service,
                        output: output.len() as u32,
                        displacement,
                    },
                ));
            }
            let start = output.len() - displacement as usize - 1;
            for index in (start..).take(length) {
                if (output.len() as u32) >= plan.size {
                    break;
                }
                let byte = output[index];
                output.push(byte);
            }
        }
    }
    Ok(output)
}

fn run_length(source: &mut Source<'_>, size: u32) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(size as usize);
    while (output.len() as u32) < size {
        let flag = source.read8()?;
        let compressed = flag & 0x80 != 0;
        let length = if compressed {
            usize::from(flag & 0x7F) + 3
        } else {
            usize::from(flag & 0x7F) + 1
        };
        if compressed {
            let value = source.read8()?;
            for _ in 0..length {
                if (output.len() as u32) >= size {
                    break;
                }
                output.push(value);
            }
        } else {
            for _ in 0..length {
                if (output.len() as u32) >= size {
                    break;
                }
                output.push(source.read8()?);
            }
        }
    }
    Ok(output)
}

fn diff8(source: &mut Source<'_>, size: u32) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(size as usize);
    let mut previous = 0u8;
    for _ in 0..size {
        let value = previous.wrapping_add(source.read8()?);
        output.push(value);
        previous = value;
    }
    Ok(output)
}

fn diff16(source: &mut Source<'_>, size: u32) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(size as usize);
    let mut previous = 0u16;
    for _ in 0..size / 2 {
        let low = source.read8()?;
        let high = source.read8()?;
        let value = previous.wrapping_add(u16::from_le_bytes([low, high]));
        output.extend_from_slice(&value.to_le_bytes());
        previous = value;
    }
    Ok(output)
}

fn store(bus: &mut Bus, destination: u32, bytes: &[u8], unit: WriteUnit) -> Result<()> {
    match unit {
        WriteUnit::Byte => {
            for (offset, byte) in bytes.iter().enumerate() {
                bus.write8(destination + offset as u32, *byte)?;
            }
        }
        WriteUnit::Halfword => {
            for index in 0..bytes.len() / 2 {
                bus.write16(
                    destination + index as u32 * 2,
                    u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]]),
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
