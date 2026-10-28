//! Flat bus access: region dispatch, width composition and the I/O policy.
//!
//! Writes to I/O addresses that no modelled register claims are ignored, which
//! is what the hardware does with unused I/O space; each one is recorded so the
//! set cannot grow unnoticed. Reads stay explicit errors because open-bus
//! behaviour is not modelled.

use super::*;

const MAX_IGNORED_IO_WRITES: usize = 4096;

/// Work RAM mirrors its bank across a whole 16 MiB window (GBATEK
/// `#gba-memory-map`): 256 KiB EWRAM over `0x02000000..=0x02FFFFFF` and 32 KiB
/// IWRAM over `0x03000000..=0x03FFFFFF`. Both lengths are powers of two.
pub(super) fn mirror_offset(addr: u32, base: u32, len: usize) -> Option<usize> {
    const WINDOW: u32 = 0x0100_0000;
    let offset = addr.checked_sub(base)?;
    if offset >= WINDOW {
        return None;
    }
    Some((offset as usize) & (len - 1))
}

impl Bus {
    /// I/O writes that no modelled register claimed. Kept out of the state
    /// digest: they are an observation about the model, not guest state.
    pub fn ignored_io_writes(&self) -> &[u32] {
        &self.ignored_io_writes
    }

    /// Number of ignored byte/halfword bus operations, including accesses
    /// omitted from the bounded address sample. A 32-bit I/O write can count
    /// twice because it is composed from two halfword accesses.
    pub fn ignored_io_write_count(&self) -> u64 {
        self.ignored_io_write_count
    }

    /// Whether at least one distinct ignored address fell outside the retained
    /// address sample.
    pub fn ignored_io_writes_truncated(&self) -> bool {
        self.ignored_io_writes_truncated
    }

    fn record_ignored_io_write(&mut self, addr: u32) {
        self.ignored_io_write_count = self.ignored_io_write_count.saturating_add(1);
        if self.ignored_io_write_addresses.contains(&addr) {
            return;
        }
        if self.ignored_io_writes.len() < MAX_IGNORED_IO_WRITES {
            self.ignored_io_write_addresses.insert(addr);
            self.ignored_io_writes.push(addr);
        } else {
            self.ignored_io_writes_truncated = true;
        }
    }

    pub fn read8(&self, addr: u32) -> Result<u8> {
        if is_bios(addr) {
            return match self.bios_data_policy {
                BiosDataPolicy::Strict => Err(Error::BiosAccess { addr }),
                BiosDataPolicy::SyntheticZeroFill => Ok(0),
            };
        }
        if let Some(offset) = mirror_offset(addr, EWRAM_BASE, EWRAM_LEN) {
            return Ok(self.ewram[offset]);
        }
        if let Some(offset) = mirror_offset(addr, IWRAM_BASE, IWRAM_LEN) {
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
            if let Some(value) = self.io.irq().read8(addr) {
                return Ok(value);
            }
            if let Some(value) = self.sound_read8(addr) {
                return Ok(value);
            }
            return Err(Error::UnsupportedIoRead { addr, width: 8 });
        }
        Err(Error::Unmapped { addr })
    }

    pub fn read16(&self, addr: u32) -> Result<u16> {
        require_halfword(addr)?;
        if is_bios(addr) {
            return match self.bios_data_policy {
                BiosDataPolicy::Strict => Err(Error::BiosAccess { addr }),
                BiosDataPolicy::SyntheticZeroFill => Ok(0),
            };
        }
        if let Some(offset) = mirror_offset(addr, EWRAM_BASE, EWRAM_LEN) {
            return Ok(u16::from_le_bytes([
                self.ewram[offset],
                self.ewram[offset + 1],
            ]));
        }
        if let Some(offset) = mirror_offset(addr, IWRAM_BASE, IWRAM_LEN) {
            return Ok(u16::from_le_bytes([
                self.iwram[offset],
                self.iwram[offset + 1],
            ]));
        }
        if (IO_BASE..=IO_BASE + 0x3FF).contains(&addr) {
            if let Some(value) = self.dma_read16(addr) {
                return Ok(value);
            }
            if let Some(value) = self.io.irq().read16(addr) {
                return Ok(value);
            }
            if let Some(value) = self.sound_read16(addr) {
                return Ok(value);
            }
            if let Some(value) = self.timer_read16(addr) {
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
        if is_bios(addr) {
            return match self.bios_data_policy {
                BiosDataPolicy::Strict => Err(Error::BiosAccess { addr }),
                BiosDataPolicy::SyntheticZeroFill => Ok(0),
            };
        }
        if let Some(offset) = mirror_offset(addr, EWRAM_BASE, EWRAM_LEN) {
            return Ok(u32::from_le_bytes([
                self.ewram[offset],
                self.ewram[offset + 1],
                self.ewram[offset + 2],
                self.ewram[offset + 3],
            ]));
        }
        if let Some(offset) = mirror_offset(addr, IWRAM_BASE, IWRAM_LEN) {
            return Ok(u32::from_le_bytes([
                self.iwram[offset],
                self.iwram[offset + 1],
                self.iwram[offset + 2],
                self.iwram[offset + 3],
            ]));
        }
        if (IO_BASE..=IO_BASE + 0x3FF).contains(&addr) {
            // 32-bit I/O is two halfword reads on hardware; a read of an
            // unmodelled register still fails because open bus is not modelled.
            let low = u32::from(self.read16(addr)?);
            let high = u32::from(self.read16(addr + 2)?);
            return Ok(low | (high << 16));
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
        if is_bios(addr) {
            // The data policy never permits BIOS writes.
            return Err(Error::BiosAccess { addr });
        }
        if let Some(offset) = mirror_offset(addr, EWRAM_BASE, EWRAM_LEN) {
            self.ewram[offset] = value;
            return Ok(());
        }
        if let Some(offset) = mirror_offset(addr, IWRAM_BASE, IWRAM_LEN) {
            self.iwram[offset] = value;
            return Ok(());
        }
        if (IO_BASE..=IO_BASE + 0x3FF).contains(&addr) {
            if self.sound_write8(addr, value) {
                return Ok(());
            }
            self.record_ignored_io_write(addr);
            return Ok(());
        }
        if (0x0400_0400..=0x04FF_FFFF).contains(&addr) {
            // Unused area above the I/O block; hardware raises no write error.
            self.record_ignored_io_write(addr);
            return Ok(());
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
        if (0x0400_0400..=0x04FF_FFFF).contains(&addr) {
            // Unused area above the I/O block; hardware raises no write error.
            self.record_ignored_io_write(addr);
            return Ok(());
        }
        if is_bios(addr) {
            return Err(Error::BiosAccess { addr });
        }
        if let Some(offset) = mirror_offset(addr, EWRAM_BASE, EWRAM_LEN) {
            self.ewram[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if let Some(offset) = mirror_offset(addr, IWRAM_BASE, IWRAM_LEN) {
            self.iwram[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if (IO_BASE..=IO_BASE + 0x3FF).contains(&addr) {
            if let Some(result) = self.dma_write16(addr, value) {
                return result;
            }
            if self.io.irq_mut().write16(addr, value).is_some() {
                return Ok(());
            }
            if self.sound_write16(addr, value) {
                return Ok(());
            }
            if self.timer_write16(addr, value) {
                return Ok(());
            }
            if let Some(result) = self.io.write16(addr, value) {
                return result;
            }
            self.record_ignored_io_write(addr);
            return Ok(());
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
        if (0x0400_0400..=0x04FF_FFFF).contains(&addr) {
            // Unused area above the I/O block; hardware raises no write error.
            self.record_ignored_io_write(addr);
            return Ok(());
        }
        if is_bios(addr) {
            return Err(Error::BiosAccess { addr });
        }
        if let Some(offset) = mirror_offset(addr, EWRAM_BASE, EWRAM_LEN) {
            self.ewram[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if let Some(offset) = mirror_offset(addr, IWRAM_BASE, IWRAM_LEN) {
            self.iwram[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            return Ok(());
        }
        if (IO_BASE..=IO_BASE + 0x3FF).contains(&addr) {
            if let Some(result) = self.sound_write_fifo_word_at(addr, value) {
                return result;
            }
            // 32-bit I/O is two halfword accesses on hardware.
            self.write16(addr, value as u16)?;
            self.write16(addr + 2, (value >> 16) as u16)?;
            return Ok(());
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
}
