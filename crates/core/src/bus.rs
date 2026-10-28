//! Deliberately small memory bus.
//!
//! Implemented: EWRAM, IWRAM, the documented DISPCNT/KEYINPUT and BG0 registers,
//! palette, VRAM, OAM and the three documented ROM windows. KEYINPUT receives a
//! host-injected pressed mask, without a frame-sampling or timing guarantee.
//! Everything else is an explicit error. No open-bus, DMA, save, BIOS, mirror
//! or timing behavior is fabricated.

mod dma;
mod io;
pub use io::{
    KEY_A, KEY_B, KEY_DOWN, KEY_L, KEY_LEFT, KEY_MASK, KEY_R, KEY_RIGHT, KEY_SELECT, KEY_START,
    KEY_UP,
};

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
pub const BG0CNT_ADDR: u32 = 0x0400_0008;
pub const BG0HOFS_ADDR: u32 = 0x0400_0010;
pub const BG0VOFS_ADDR: u32 = 0x0400_0012;
pub const WAITCNT_ADDR: u32 = 0x0400_0204;
pub const KEYINPUT_ADDR: u32 = 0x0400_0130;
pub const KEYINPUT_RELEASED: u16 = 0x03FF;

pub struct Bus {
    cart: Cartridge,
    ewram: Vec<u8>,
    iwram: Vec<u8>,
    palette: Vec<u8>,
    vram: Vec<u8>,
    oam: Vec<u8>,
    io: io::IoState,
    pressed_keys: u16,
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
            io: io::IoState::default(),
            pressed_keys: 0,
        }
    }

    /// Sets the host-injected GBA key mask, where one means pressed.
    ///
    /// KEYINPUT reads expose this state active-low. Bits outside [`KEY_MASK`]
    /// are ignored. This does not provide frame-synchronous sampling or a
    /// timing guarantee.
    pub fn set_pressed_keys(&mut self, pressed: u16) {
        self.pressed_keys = pressed & KEY_MASK;
    }

    pub fn cart(&self) -> &Cartridge {
        &self.cart
    }

    pub fn dispcnt(&self) -> u16 {
        self.io.dispcnt()
    }

    pub(crate) fn bg0cnt(&self) -> u16 {
        self.io.bg0cnt()
    }

    pub(crate) fn bg0_scroll(&self) -> (u16, u16) {
        self.io.bg0_scroll()
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
            if let Some(value) = self.dma_read16(addr) {
                return Ok(value);
            }
            return self
                .io
                .read16(addr, self.pressed_keys)
                .unwrap_or(Err(Error::UnsupportedIoRead { addr, width: 16 }));
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
            // DMA address registers are 32-bit halves of a 16-bit I/O block.
            if self.dma_read16(addr).is_some() && self.dma_read16(addr + 2).is_some() {
                let low = u32::from(self.read16(addr)?);
                let high = u32::from(self.read16(addr + 2)?);
                return Ok(low | (high << 16));
            }
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
            if let Some(result) = self.dma_write16(addr, value) {
                return result;
            }
            return self
                .io
                .write16(addr, value)
                .unwrap_or(Err(Error::UnsupportedIoWrite { addr, width: 16 }));
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
            if self.dma_read16(addr).is_some() && self.dma_read16(addr + 2).is_some() {
                self.write16(addr, value as u16)?;
                self.write16(addr + 2, (value >> 16) as u16)?;
                return Ok(());
            }
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

    /// SWI 01h group 7 over the *modelled* "other I/O" set. Groups 5 (SIO) and
    /// 6 (sound) model no state at all, so their selected set is empty.
    pub fn reset_group7_io(&mut self) {
        self.io.reset_group7();
    }

    pub fn state_digest(&self) -> [u8; 32] {
        let mut bytes = Vec::with_capacity(
            self.ewram.len()
                + self.iwram.len()
                + self.palette.len()
                + self.vram.len()
                + self.oam.len()
                + 10,
        );
        bytes.extend_from_slice(&self.ewram);
        bytes.extend_from_slice(&self.iwram);
        bytes.extend_from_slice(&self.palette);
        bytes.extend_from_slice(&self.vram);
        bytes.extend_from_slice(&self.oam);
        self.io.append_digest(&mut bytes);
        bytes.extend_from_slice(&(KEYINPUT_RELEASED & !self.pressed_keys).to_le_bytes());
        sha256::digest(&bytes)
    }

    pub fn vram_digest(&self) -> [u8; 32] {
        sha256::digest(&self.vram)
    }

    pub(crate) fn is_work_ram_range(addr: u32, len: usize) -> bool {
        let Some(last) = len
            .checked_sub(1)
            .and_then(|size| u32::try_from(size).ok())
            .and_then(|size| addr.checked_add(size))
        else {
            return false;
        };
        [(EWRAM_BASE, EWRAM_LEN), (IWRAM_BASE, IWRAM_LEN)]
            .iter()
            .any(|(base, size)| {
                offset_in(addr, *base, *size).is_some() && offset_in(last, *base, *size).is_some()
            })
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
mod tests;
