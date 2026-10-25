//! SWI 01h/0Bh/0Ch: documented memory copy/fill and memory reset services.
//!
//! Ranges are validated before the first bus access: a control word with
//! reserved bits, a misaligned unit, an address-space overflow, a range that
//! crosses a region boundary, a BIOS source, or a non-writable destination is
//! rejected without mutating memory. A bus error raised after the first access
//! is not rolled back; this is a fail-closed non-transactional transfer, not an
//! atomic promise.

use crate::bios::BiosError;
use crate::bus::{
    Bus, DISPCNT_ADDR, EWRAM_BASE, EWRAM_LEN, IWRAM_BASE, IWRAM_LEN, OAM_BASE, OAM_LEN,
    PALETTE_BASE, PALETTE_LEN, VRAM_BASE, VRAM_LEN,
};
use crate::cpu::Cpu;
use crate::error::{Error, Result};

const BIOS_END: u32 = 0x0000_4000;
const ROM_WINDOW_LEN: u32 = 0x0200_0000;
const COUNT_MASK: u32 = 0x001F_FFFF;
const FILL_BIT: u32 = 1 << 24;
const WORD_BIT: u32 = 1 << 26;
/// CpuSet: count bits 0..20, fill bit 24, word bit 26.
const CPU_SET_MASK: u32 = 0x051F_FFFF;
/// CpuFastSet: count bits 0..20, fill bit 24; bit 26 is reserved here.
const CPU_FAST_SET_MASK: u32 = 0x011F_FFFF;
const RESET_MEMORY_MASK: u32 = 0x1F;
const IWRAM_PRESERVED_LEN: usize = 512;
const DISPCNT_FORCED_BLANK: u16 = 0x0080;

pub(super) fn validate_call(service: u8, cpu: &Cpu, bus: &Bus) -> Result<()> {
    match service {
        super::SERVICE_REGISTER_RAM_RESET => validate_reset_flags(cpu.reg(0)),
        super::SERVICE_CPU_SET => validate_cpu_set(cpu, bus),
        super::SERVICE_CPU_FAST_SET => validate_cpu_fast_set(cpu, bus),
        _ => unreachable!("memory validator only receives memory services"),
    }
}

/// SWI 0Bh `CpuSet`: halfword or word copy/fill.
pub(super) fn cpu_set(cpu: &mut Cpu, bus: &mut Bus) -> Result<()> {
    let source = cpu.reg(0);
    let destination = cpu.reg(1);
    let control = cpu.reg(2);
    if control & !CPU_SET_MASK != 0 {
        return Err(Error::Bios(BiosError::ReservedControl {
            service: super::SERVICE_CPU_SET,
            control,
        }));
    }
    let width = if control & WORD_BIT != 0 { 4 } else { 2 };
    require_alignment(super::SERVICE_CPU_SET, source, width)?;
    require_alignment(super::SERVICE_CPU_SET, destination, width)?;
    let units = control & COUNT_MASK;
    if units == 0 {
        return Ok(());
    }
    let fill = control & FILL_BIT != 0;
    validate_destination(super::SERVICE_CPU_SET, destination, units, width)?;
    // A fill reads only the first source unit; a copy reads the whole range.
    let source_units = if fill { 1 } else { units };
    validate_source(bus, super::SERVICE_CPU_SET, source, source_units, width)?;
    copy_or_fill(bus, source, destination, units, width, fill)
}

fn validate_cpu_set(cpu: &Cpu, bus: &Bus) -> Result<()> {
    let source = cpu.reg(0);
    let destination = cpu.reg(1);
    let control = cpu.reg(2);
    if control & !CPU_SET_MASK != 0 {
        return Err(Error::Bios(BiosError::ReservedControl {
            service: super::SERVICE_CPU_SET,
            control,
        }));
    }
    let width = if control & WORD_BIT != 0 { 4 } else { 2 };
    require_alignment(super::SERVICE_CPU_SET, source, width)?;
    require_alignment(super::SERVICE_CPU_SET, destination, width)?;
    let units = control & COUNT_MASK;
    if units == 0 {
        return Ok(());
    }
    let fill = control & FILL_BIT != 0;
    validate_destination(super::SERVICE_CPU_SET, destination, units, width)?;
    validate_source(
        bus,
        super::SERVICE_CPU_SET,
        source,
        if fill { 1 } else { units },
        width,
    )
}

/// SWI 0Ch `CpuFastSet`: 32-bit copy/fill, rounded up to 8-word blocks.
pub(super) fn cpu_fast_set(cpu: &mut Cpu, bus: &mut Bus) -> Result<()> {
    let source = cpu.reg(0);
    let destination = cpu.reg(1);
    let control = cpu.reg(2);
    if control & !CPU_FAST_SET_MASK != 0 {
        return Err(Error::Bios(BiosError::ReservedControl {
            service: super::SERVICE_CPU_FAST_SET,
            control,
        }));
    }
    require_alignment(super::SERVICE_CPU_FAST_SET, source, 4)?;
    require_alignment(super::SERVICE_CPU_FAST_SET, destination, 4)?;
    let count = control & COUNT_MASK;
    // GBA behavior documented by GBATEK: the word count is force-rounded up to
    // a multiple of 8 words. Count zero remains a no-access request.
    let units = if count == 0 { 0 } else { count.div_ceil(8) * 8 };
    if units == 0 {
        return Ok(());
    }
    let fill = control & FILL_BIT != 0;
    validate_destination(super::SERVICE_CPU_FAST_SET, destination, units, 4)?;
    let source_units = if fill { 1 } else { units };
    validate_source(bus, super::SERVICE_CPU_FAST_SET, source, source_units, 4)?;
    copy_or_fill(bus, source, destination, units, 4, fill)
}

fn validate_cpu_fast_set(cpu: &Cpu, bus: &Bus) -> Result<()> {
    let source = cpu.reg(0);
    let destination = cpu.reg(1);
    let control = cpu.reg(2);
    if control & !CPU_FAST_SET_MASK != 0 {
        return Err(Error::Bios(BiosError::ReservedControl {
            service: super::SERVICE_CPU_FAST_SET,
            control,
        }));
    }
    require_alignment(super::SERVICE_CPU_FAST_SET, source, 4)?;
    require_alignment(super::SERVICE_CPU_FAST_SET, destination, 4)?;
    let count = control & COUNT_MASK;
    let units = if count == 0 { 0 } else { count.div_ceil(8) * 8 };
    if units == 0 {
        return Ok(());
    }
    let fill = control & FILL_BIT != 0;
    validate_destination(super::SERVICE_CPU_FAST_SET, destination, units, 4)?;
    validate_source(
        bus,
        super::SERVICE_CPU_FAST_SET,
        source,
        if fill { 1 } else { units },
        4,
    )
}

/// SWI 01h `RegisterRamReset`: clear the selected memory regions, preserve the
/// top 512 bytes of IWRAM, and always force blank via DISPCNT=0080h.
pub(super) fn register_ram_reset(cpu: &mut Cpu, bus: &mut Bus) -> Result<()> {
    let flags = cpu.reg(0);
    validate_reset_flags(flags)?;
    if flags & 0x01 != 0 {
        fill_words(bus, EWRAM_BASE, EWRAM_LEN, 0)?;
    }
    if flags & 0x02 != 0 {
        // GBATEK: clear on-chip WRAM excluding the last 200h bytes.
        fill_words(bus, IWRAM_BASE, IWRAM_LEN - IWRAM_PRESERVED_LEN, 0)?;
    }
    if flags & 0x04 != 0 {
        fill_halfwords(bus, PALETTE_BASE, PALETTE_LEN)?;
    }
    if flags & 0x08 != 0 {
        fill_halfwords(bus, VRAM_BASE, VRAM_LEN)?;
    }
    if flags & 0x10 != 0 {
        // Zero-filling OAM does not by itself disable objects; DISPCNT bit 6
        // (OBJ enable) and the OAM attribute state are separate.
        fill_halfwords(bus, OAM_BASE, OAM_LEN)?;
    }
    // Always forced blank, even with flags==0, per the documented service.
    bus.write16(DISPCNT_ADDR, DISPCNT_FORCED_BLANK)?;
    Ok(())
}

fn validate_reset_flags(flags: u32) -> Result<()> {
    if flags & !RESET_MEMORY_MASK != 0 {
        // Bits 5..7 are serial/sound/other-register resets. They are not
        // implemented and must fail before any partial memory mutation.
        return Err(Error::Bios(BiosError::UnsupportedResetFlags { flags }));
    }
    Ok(())
}

fn copy_or_fill(
    bus: &mut Bus,
    source: u32,
    destination: u32,
    units: u32,
    width: u32,
    fill: bool,
) -> Result<()> {
    if fill {
        let value = read_unit(bus, source, width)?;
        for unit in 0..units {
            write_unit(bus, destination + unit * width, width, value)?;
        }
        return Ok(());
    }
    // Ascending LDMIA/STMIA-style copy. Forward overlap deliberately smears
    // rather than taking a snapshot like memmove.
    for unit in 0..units {
        let offset = unit * width;
        let value = read_unit(bus, source + offset, width)?;
        write_unit(bus, destination + offset, width, value)?;
    }
    Ok(())
}

fn read_unit(bus: &Bus, addr: u32, width: u32) -> Result<u32> {
    if width == 4 {
        bus.read32(addr)
    } else {
        bus.read16(addr).map(u32::from)
    }
}

fn write_unit(bus: &mut Bus, addr: u32, width: u32, value: u32) -> Result<()> {
    if width == 4 {
        bus.write32(addr, value)
    } else {
        bus.write16(addr, value as u16)
    }
}

fn require_alignment(service: u8, addr: u32, align: u32) -> Result<()> {
    if addr.is_multiple_of(align) {
        Ok(())
    } else {
        Err(Error::Bios(BiosError::Misaligned {
            service,
            addr,
            align: align as u8,
        }))
    }
}

fn validate_source(bus: &Bus, service: u8, addr: u32, units: u32, width: u32) -> Result<()> {
    let (start, end) = expanded_range(service, addr, units, width)?;
    if start < BIOS_END {
        // Fail closed instead of reading a non-existent BIOS or silently
        // rejecting the whole request as if it had succeeded.
        return Err(Error::BiosAccess { addr: start });
    }
    let region = region_of(start).ok_or(Error::Bios(BiosError::UnsupportedSource {
        service,
        addr: start,
    }))?;
    if region_of(end) != Some(region) {
        return Err(Error::Bios(BiosError::RangeOutsideRegion {
            service,
            addr: start,
            end,
        }));
    }
    if let Region::Rom(base) = region {
        let last_offset = usize::try_from(end - base).unwrap_or(usize::MAX);
        if last_offset >= bus.cart().len() {
            return Err(Error::Bios(BiosError::RangeOutsideRegion {
                service,
                addr: start,
                end,
            }));
        }
    }
    Ok(())
}

fn validate_destination(service: u8, addr: u32, units: u32, width: u32) -> Result<()> {
    let (start, end) = expanded_range(service, addr, units, width)?;
    let region = region_of(start).ok_or(Error::Bios(BiosError::UnsupportedDestination {
        service,
        addr: start,
    }))?;
    if !region.is_writable() {
        return Err(Error::Bios(BiosError::UnsupportedDestination {
            service,
            addr: start,
        }));
    }
    if region_of(end) != Some(region) {
        return Err(Error::Bios(BiosError::RangeOutsideRegion {
            service,
            addr: start,
            end,
        }));
    }
    Ok(())
}

/// Expand `units * width` bytes and return the inclusive last address using
/// checked arithmetic. Count is at most 21 bits, so no unbounded allocation or
/// loop size can result.
fn expanded_range(service: u8, addr: u32, units: u32, width: u32) -> Result<(u32, u32)> {
    if units == 0 {
        return Ok((addr, addr));
    }
    let length = units
        .checked_mul(width)
        .ok_or(Error::Bios(BiosError::RangeOverflow {
            service,
            addr,
            units,
        }))?;
    let end = addr
        .checked_add(length - 1)
        .ok_or(Error::Bios(BiosError::RangeOverflow {
            service,
            addr,
            units,
        }))?;
    Ok((addr, end))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Region {
    Ewram,
    Iwram,
    Palette,
    Vram,
    Oam,
    Rom(u32),
}

impl Region {
    fn is_writable(self) -> bool {
        !matches!(self, Region::Rom(_))
    }
}

fn region_of(addr: u32) -> Option<Region> {
    if offset_in(addr, EWRAM_BASE, EWRAM_LEN) {
        return Some(Region::Ewram);
    }
    if offset_in(addr, IWRAM_BASE, IWRAM_LEN) {
        return Some(Region::Iwram);
    }
    if offset_in(addr, PALETTE_BASE, PALETTE_LEN) {
        return Some(Region::Palette);
    }
    if offset_in(addr, VRAM_BASE, VRAM_LEN) {
        return Some(Region::Vram);
    }
    if offset_in(addr, OAM_BASE, OAM_LEN) {
        return Some(Region::Oam);
    }
    for base in [0x0800_0000, 0x0A00_0000, 0x0C00_0000] {
        if (base..base + ROM_WINDOW_LEN).contains(&addr) {
            return Some(Region::Rom(base));
        }
    }
    None
}

fn offset_in(addr: u32, base: u32, len: usize) -> bool {
    addr.checked_sub(base)
        .is_some_and(|offset| (offset as usize) < len)
}

fn fill_words(bus: &mut Bus, base: u32, len: usize, value: u32) -> Result<()> {
    let mut offset = 0;
    while offset < len {
        bus.write32(base + offset as u32, value)?;
        offset += 4;
    }
    Ok(())
}

fn fill_halfwords(bus: &mut Bus, base: u32, len: usize) -> Result<()> {
    let mut offset = 0;
    while offset < len {
        bus.write16(base + offset as u32, 0)?;
        offset += 2;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cart::Cartridge;

    fn bus() -> Bus {
        Bus::new(Cartridge::from_bytes(vec![0u8; 0x400]).expect("cart"))
    }

    #[test]
    fn region_classification_has_no_mirrors() {
        assert!(matches!(region_of(EWRAM_BASE), Some(Region::Ewram)));
        assert_eq!(region_of(EWRAM_BASE + EWRAM_LEN as u32), None);
        assert_eq!(region_of(0x0000_0000), None);
        assert_eq!(region_of(0x0400_0000), None);
        assert!(matches!(
            region_of(0x0C00_0000),
            Some(Region::Rom(0x0C00_0000))
        ));
        assert!(matches!(region_of(0x0DFF_FFFF), Some(Region::Rom(_))));
        assert_eq!(region_of(0x0E00_0000), None);
    }

    #[test]
    fn expanded_range_reports_overflow() {
        assert_eq!(
            expanded_range(0x0B, 0x0200_0000, 4, 2).unwrap(),
            (0x0200_0000, 0x0200_0007)
        );
        assert!(matches!(
            expanded_range(0x0B, 0xFFFF_FFFE, 4, 2),
            Err(Error::Bios(BiosError::RangeOverflow { .. }))
        ));
    }

    #[test]
    fn zero_count_skips_range_validation() {
        assert_eq!(expanded_range(0x0B, 0, 0, 4).unwrap(), (0, 0));
    }

    #[test]
    fn fill_helpers_zero_whole_ranges() {
        let mut bus = bus();
        bus.write32(EWRAM_BASE, 0x1234_5678).expect("seed");
        fill_words(&mut bus, EWRAM_BASE, 8, 0).expect("fill");
        assert_eq!(bus.read32(EWRAM_BASE).expect("read"), 0);
        bus.write16(PALETTE_BASE, 0xABCD).expect("seed");
        fill_halfwords(&mut bus, PALETTE_BASE, 4).expect("fill");
        assert_eq!(bus.read16(PALETTE_BASE).expect("read"), 0);
    }
}
