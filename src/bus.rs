//! Deliberately small memory bus.
//!
//! Implemented: EWRAM, IWRAM, DISPCNT/KEYINPUT, palette, VRAM, OAM and the
//! three documented ROM windows. Everything else is an explicit error. No
//! open-bus, DMA, save, BIOS, mirror or timing behavior is fabricated.

use crate::cart::{Cartridge, ROM_BASE};
use crate::error::{Error, Result};
use crate::sha256;

pub const EWRAM_BASE: u32 = 0x0200_0000;
pub const EWRAM_LEN: usize = 256 * 1024;
pub const IWRAM_BASE: u32 = 0x0300_0000;
pub const IWRAM_LEN: usize = 32 * 1024;
pub const IO_BASE: u32 = 0x0400_0000;
pub const PALETTE_BASE: u32 = 0x0500_0000;
pub const PALETTE_LEN: usize = 1024;
pub const VRAM_BASE: u32 = 0x0600_0000;
pub const VRAM_LEN: usize = 96 * 1024;
pub const OAM_BASE: u32 = 0x0700_0000;
pub const OAM_LEN: usize = 1024;
pub const DISPCNT_ADDR: u32 = 0x0400_0000;
pub const KEYINPUT_ADDR: u32 = 0x0400_0130;
pub const KEYINPUT_RELEASED: u16 = 0x03FF;

pub struct Bus {
    cart: Cartridge,
    ewram: Vec<u8>,
    iwram: Vec<u8>,
    palette: Vec<u8>,
    vram: Vec<u8>,
    oam: Vec<u8>,
    dispcnt: u16,
}

impl Bus {
    pub fn new(cart: Cartridge) -> Self {
        Self {
            cart,
            ewram: vec![0; EWRAM_LEN],
            iwram: vec![0; IWRAM_LEN],
            palette: vec![0; PALETTE_LEN],
            vram: vec![0; VRAM_LEN],
            oam: vec![0; OAM_LEN],
            dispcnt: 0,
        }
    }

    pub fn cart(&self) -> &Cartridge {
        &self.cart
    }

    pub fn dispcnt(&self) -> u16 {
        self.dispcnt
    }

    pub fn read8(&self, addr: u32) -> Result<u8> {
        if (0x0000_0000..=0x0000_3FFF).contains(&addr) {
            return Err(Error::BiosAccess { addr });
        }
        if let Some(offset) = offset_in(addr, EWRAM_BASE, EWRAM_LEN) {
            return Ok(self.ewram[offset]);
        }
        if let Some(offset) = offset_in(addr, IWRAM_BASE, IWRAM_LEN) {
            return Ok(self.iwram[offset]);
        }
        if let Some(offset) = offset_in(addr, PALETTE_BASE, PALETTE_LEN) {
            return Ok(self.palette[offset]);
        }
        if let Some(offset) = offset_in(addr, VRAM_BASE, VRAM_LEN) {
            return Ok(self.vram[offset]);
        }
        if let Some(offset) = offset_in(addr, OAM_BASE, OAM_LEN) {
            return Ok(self.oam[offset]);
        }
        if (0x0800_0000..=0x0DFF_FFFF).contains(&addr) {
            return self.cart.read_u8(addr).ok_or(Error::RomOverread {
                addr,
                len: self.cart.len(),
            });
        }
        if (0x0E00_0000..=0x0E00_FFFF).contains(&addr) {
            return Err(Error::UnsupportedSave { addr });
        }
        if (IO_BASE..=IO_BASE + 0x3FF).contains(&addr) {
            return Err(Error::UnsupportedIoRead { addr, width: 8 });
        }
        Err(Error::Unmapped { addr })
    }

    pub fn read16(&self, addr: u32) -> Result<u16> {
        require_halfword(addr)?;
        if (0x0000_0000..=0x0000_3FFF).contains(&addr) {
            return Err(Error::BiosAccess { addr });
        }
        if let Some(offset) = offset_in(addr, EWRAM_BASE, EWRAM_LEN) {
            return Ok(u16::from_le_bytes([
                self.ewram[offset],
                self.ewram[offset + 1],
            ]));
        }
        if let Some(offset) = offset_in(addr, IWRAM_BASE, IWRAM_LEN) {
            return Ok(u16::from_le_bytes([
                self.iwram[offset],
                self.iwram[offset + 1],
            ]));
        }
        if (IO_BASE..=IO_BASE + 0x3FF).contains(&addr) {
            return match addr {
                DISPCNT_ADDR => Ok(self.dispcnt),
                KEYINPUT_ADDR => Ok(KEYINPUT_RELEASED),
                _ => Err(Error::UnsupportedIoRead { addr, width: 16 }),
            };
        }
        if let Some(offset) = offset_in(addr, PALETTE_BASE, PALETTE_LEN) {
            return Ok(u16::from_le_bytes([
                self.palette[offset],
                self.palette[offset + 1],
            ]));
        }
        if let Some(offset) = offset_in(addr, VRAM_BASE, VRAM_LEN) {
            return Ok(u16::from_le_bytes([
                self.vram[offset],
                self.vram[offset + 1],
            ]));
        }
        if let Some(offset) = offset_in(addr, OAM_BASE, OAM_LEN) {
            return Ok(u16::from_le_bytes([self.oam[offset], self.oam[offset + 1]]));
        }
        if (0x0800_0000..=0x0DFF_FFFF).contains(&addr) {
            return self.cart.read_u16(addr).ok_or(Error::RomOverread {
                addr,
                len: self.cart.len(),
            });
        }
        if (0x0E00_0000..=0x0E00_FFFF).contains(&addr) {
            return Err(Error::UnsupportedSave { addr });
        }
        Err(Error::Unmapped { addr })
    }

    pub fn read32(&self, addr: u32) -> Result<u32> {
        require_word(addr)?;
        if (0x0000_0000..=0x0000_3FFF).contains(&addr) {
            return Err(Error::BiosAccess { addr });
        }
        if let Some(offset) = offset_in(addr, EWRAM_BASE, EWRAM_LEN) {
            return Ok(u32::from_le_bytes([
                self.ewram[offset],
                self.ewram[offset + 1],
                self.ewram[offset + 2],
                self.ewram[offset + 3],
            ]));
        }
        if let Some(offset) = offset_in(addr, IWRAM_BASE, IWRAM_LEN) {
            return Ok(u32::from_le_bytes([
                self.iwram[offset],
                self.iwram[offset + 1],
                self.iwram[offset + 2],
                self.iwram[offset + 3],
            ]));
        }
        if (IO_BASE..=IO_BASE + 0x3FF).contains(&addr) {
            return Err(Error::UnsupportedIoRead { addr, width: 32 });
        }
        if (0x0800_0000..=0x0DFF_FFFF).contains(&addr) {
            return self.cart.read_u32(addr).ok_or(Error::RomOverread {
                addr,
                len: self.cart.len(),
            });
        }
        if (0x0E00_0000..=0x0E00_FFFF).contains(&addr) {
            return Err(Error::UnsupportedSave { addr });
        }
        // Palette, VRAM and OAM are 16-bit devices; assemble a word from two
        // implemented halfword accesses.
        if offset_in(addr, PALETTE_BASE, PALETTE_LEN).is_some()
            || offset_in(addr, VRAM_BASE, VRAM_LEN).is_some()
            || offset_in(addr, OAM_BASE, OAM_LEN).is_some()
        {
            let low = self.read16(addr)? as u32;
            let high = self.read16(addr + 2)? as u32;
            return Ok(low | (high << 16));
        }
        Err(Error::Unmapped { addr })
    }

    pub fn write8(&mut self, addr: u32, value: u8) -> Result<()> {
        if (0x0000_0000..=0x0000_3FFF).contains(&addr) {
            return Err(Error::BiosAccess { addr });
        }
        if let Some(offset) = offset_in(addr, EWRAM_BASE, EWRAM_LEN) {
            self.ewram[offset] = value;
            return Ok(());
        }
        if let Some(offset) = offset_in(addr, IWRAM_BASE, IWRAM_LEN) {
            self.iwram[offset] = value;
            return Ok(());
        }
        if (IO_BASE..=IO_BASE + 0x3FF).contains(&addr) {
            return Err(Error::UnsupportedIoWrite { addr, width: 8 });
        }
        if let Some(offset) = offset_in(addr, PALETTE_BASE, PALETTE_LEN) {
            // GBATEK: byte stores to 16-bit palette memory duplicate the byte.
            let aligned = offset & !1;
            self.palette[aligned] = value;
            self.palette[aligned + 1] = value;
            return Ok(());
        }
        if let Some(offset) = offset_in(addr, VRAM_BASE, VRAM_LEN) {
            // Same documented byte-store duplication for VRAM.
            let aligned = offset & !1;
            self.vram[aligned] = value;
            self.vram[aligned + 1] = value;
            return Ok(());
        }
        if offset_in(addr, OAM_BASE, OAM_LEN).is_some() {
            // OAM has no byte-write path; the store has no effect.
            return Ok(());
        }
        if (0x0800_0000..=0x0DFF_FFFF).contains(&addr) {
            return Err(Error::RomWrite { addr });
        }
        if (0x0E00_0000..=0x0E00_FFFF).contains(&addr) {
            return Err(Error::UnsupportedSave { addr });
        }
        Err(Error::Unmapped { addr })
    }

    pub fn write16(&mut self, addr: u32, value: u16) -> Result<()> {
        require_halfword(addr)?;
        if (0x0000_0000..=0x0000_3FFF).contains(&addr) {
            return Err(Error::BiosAccess { addr });
        }
        if let Some(offset) = offset_in(addr, EWRAM_BASE, EWRAM_LEN) {
            self.ewram[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if let Some(offset) = offset_in(addr, IWRAM_BASE, IWRAM_LEN) {
            self.iwram[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if (IO_BASE..=IO_BASE + 0x3FF).contains(&addr) {
            return match addr {
                DISPCNT_ADDR => {
                    self.dispcnt = value;
                    Ok(())
                }
                KEYINPUT_ADDR => Err(Error::UnsupportedIoWrite { addr, width: 16 }),
                _ => Err(Error::UnsupportedIoWrite { addr, width: 16 }),
            };
        }
        if let Some(offset) = offset_in(addr, PALETTE_BASE, PALETTE_LEN) {
            self.palette[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if let Some(offset) = offset_in(addr, VRAM_BASE, VRAM_LEN) {
            self.vram[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if let Some(offset) = offset_in(addr, OAM_BASE, OAM_LEN) {
            self.oam[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if (0x0800_0000..=0x0DFF_FFFF).contains(&addr) {
            return Err(Error::RomWrite { addr });
        }
        if (0x0E00_0000..=0x0E00_FFFF).contains(&addr) {
            return Err(Error::UnsupportedSave { addr });
        }
        Err(Error::Unmapped { addr })
    }

    pub fn write32(&mut self, addr: u32, value: u32) -> Result<()> {
        require_word(addr)?;
        if (0x0000_0000..=0x0000_3FFF).contains(&addr) {
            return Err(Error::BiosAccess { addr });
        }
        if let Some(offset) = offset_in(addr, EWRAM_BASE, EWRAM_LEN) {
            self.ewram[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if let Some(offset) = offset_in(addr, IWRAM_BASE, IWRAM_LEN) {
            self.iwram[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if (IO_BASE..=IO_BASE + 0x3FF).contains(&addr) {
            return Err(Error::UnsupportedIoWrite { addr, width: 32 });
        }
        if (0x0800_0000..=0x0DFF_FFFF).contains(&addr) {
            return Err(Error::RomWrite { addr });
        }
        if (0x0E00_0000..=0x0E00_FFFF).contains(&addr) {
            return Err(Error::UnsupportedSave { addr });
        }
        if offset_in(addr, PALETTE_BASE, PALETTE_LEN).is_some()
            || offset_in(addr, VRAM_BASE, VRAM_LEN).is_some()
            || offset_in(addr, OAM_BASE, OAM_LEN).is_some()
        {
            self.write16(addr, value as u16)?;
            self.write16(addr + 2, (value >> 16) as u16)?;
            return Ok(());
        }
        Err(Error::Unmapped { addr })
    }

    /// Instruction fetch. Only ROM and work RAM are executable by design.
    pub fn fetch16(&self, addr: u32) -> Result<u16> {
        if !addr.is_multiple_of(2) {
            return Err(Error::UnalignedFetch { addr, thumb: true });
        }
        if is_rom(addr)
            || offset_in(addr, EWRAM_BASE, EWRAM_LEN).is_some()
            || offset_in(addr, IWRAM_BASE, IWRAM_LEN).is_some()
        {
            return self.read16(addr);
        }
        Err(Error::NonExecutable { addr })
    }

    pub fn fetch32(&self, addr: u32) -> Result<u32> {
        if !addr.is_multiple_of(4) {
            return Err(Error::UnalignedFetch { addr, thumb: false });
        }
        if is_rom(addr)
            || offset_in(addr, EWRAM_BASE, EWRAM_LEN).is_some()
            || offset_in(addr, IWRAM_BASE, IWRAM_LEN).is_some()
        {
            return self.read32(addr);
        }
        Err(Error::NonExecutable { addr })
    }

    pub fn state_digest(&self) -> [u8; 32] {
        let mut bytes = Vec::with_capacity(
            self.ewram.len()
                + self.iwram.len()
                + self.palette.len()
                + self.vram.len()
                + self.oam.len()
                + 4,
        );
        bytes.extend_from_slice(&self.ewram);
        bytes.extend_from_slice(&self.iwram);
        bytes.extend_from_slice(&self.palette);
        bytes.extend_from_slice(&self.vram);
        bytes.extend_from_slice(&self.oam);
        bytes.extend_from_slice(&self.dispcnt.to_le_bytes());
        bytes.extend_from_slice(&KEYINPUT_RELEASED.to_le_bytes());
        sha256::digest(&bytes)
    }

    pub fn vram_digest(&self) -> [u8; 32] {
        sha256::digest(&self.vram)
    }
}

fn is_rom(addr: u32) -> bool {
    (ROM_BASE..=0x0DFF_FFFF).contains(&addr)
}

fn offset_in(addr: u32, base: u32, len: usize) -> Option<usize> {
    addr.checked_sub(base)
        .map(|offset| offset as usize)
        .filter(|offset| *offset < len)
}

fn require_halfword(addr: u32) -> Result<()> {
    if addr.is_multiple_of(2) {
        Ok(())
    } else {
        Err(Error::UnalignedHalfword { addr })
    }
}

fn require_word(addr: u32) -> Result<()> {
    if addr.is_multiple_of(4) {
        Ok(())
    } else {
        Err(Error::UnalignedWord { addr })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus() -> Bus {
        let cart = Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart");
        Bus::new(cart)
    }

    #[test]
    fn ram_is_little_endian() {
        let mut bus = bus();
        bus.write32(EWRAM_BASE, 0x1234_5678).expect("write");
        assert_eq!(bus.read8(EWRAM_BASE).expect("byte"), 0x78);
        assert_eq!(bus.read16(EWRAM_BASE).expect("half"), 0x5678);
        assert_eq!(bus.read32(EWRAM_BASE).expect("word"), 0x1234_5678);
    }

    #[test]
    fn byte_writes_to_video_memory_duplicate() {
        let mut bus = bus();
        bus.write8(VRAM_BASE + 3, 0xAB).expect("vram byte");
        assert_eq!(bus.read16(VRAM_BASE + 2).expect("vram half"), 0xABAB);
        bus.write8(PALETTE_BASE + 1, 0xCD).expect("palette byte");
        assert_eq!(bus.read16(PALETTE_BASE).expect("palette half"), 0xCDCD);
        bus.write8(OAM_BASE + 1, 0xEF).expect("oam byte");
        assert_eq!(bus.read16(OAM_BASE).expect("oam half"), 0);
    }

    #[test]
    fn rejects_unsupported_and_out_of_bounds() {
        let mut bus = bus();
        assert!(matches!(
            bus.read8(0x0000_0010),
            Err(Error::BiosAccess { .. })
        ));
        assert!(matches!(
            bus.read16(0x0400_0004),
            Err(Error::UnsupportedIoRead { .. })
        ));
        assert!(matches!(
            bus.write16(0x0400_0004, 0),
            Err(Error::UnsupportedIoWrite { .. })
        ));
        assert!(matches!(
            bus.write8(0x0800_0000, 0),
            Err(Error::RomWrite { .. })
        ));
        assert!(matches!(
            bus.read16(0x0600_0001),
            Err(Error::UnalignedHalfword { .. })
        ));
        assert!(matches!(
            bus.read32(0x0200_0002),
            Err(Error::UnalignedWord { .. })
        ));
        assert!(matches!(
            bus.read8(0x0204_0000),
            Err(Error::Unmapped { .. })
        ));
        assert!(matches!(
            bus.read8(0x0E00_0000),
            Err(Error::UnsupportedSave { .. })
        ));
    }
}
