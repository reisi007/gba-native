//! Game Pak EEPROM device model with an explicit 512 B or 8 KiB selection.
//!
//! GBATEK `#gba-cart-backup-eeprom` documents a serial device on data-bus bit 0
//! at `0x0DFFFF00..=0x0DFFFFFF`, driven by DMA3 with 16-bit transfers. A read
//! request is `11` + n address bits (MSB first) + `0` and is answered by 68
//! bits (4 ignored + 64 data). A write request is `10` + n address bits + 64
//! data bits + `0`; writing auto-erases the old 64-bit block. GBATEK documents
//! no autodetection, so the host selects the 512-byte (6 address bits) or
//! 8-KiB (14 address bits, only the lower 10 used) part explicitly.
//!
//! The model is volatile and timing-free: a completed write is immediately
//! ready, and the documented ~6.5 ms program delay is not simulated. Manual
//! halfword stores are rejected because GBATEK states they do not drive the
//! serial chip; manual halfword reads are allowed for the documented ready poll.

use std::cell::Cell;

use super::*;

/// Documented EEPROM window inside the ROM address space.
pub const EEPROM_WINDOW_BASE: u32 = 0x0DFF_FF00;
/// Documented EEPROM window length (256 bytes).
pub const EEPROM_WINDOW_LEN: u32 = 0x100;
/// EEPROM data/address unit in bytes (64 bits).
pub const EEPROM_BLOCK_LEN: usize = 8;

const MAX_COMMAND_BITS: usize = 2 + 14 + 64 + 1;
const READ_STREAM_BITS: usize = 4 + 64;

/// Explicit EEPROM capacity selected by the host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EepromSize {
    /// 512 bytes with a 6-bit block address.
    Bytes512,
    /// 8 KiB with a 14-bit wire address of which only the lower 10 bits are used.
    Bytes8K,
}

impl EepromSize {
    /// Persistable device capacity.
    pub(super) fn capacity(self) -> usize {
        match self {
            Self::Bytes512 => 512,
            Self::Bytes8K => 8 * 1024,
        }
    }

    fn address_bits(self) -> usize {
        match self {
            Self::Bytes512 => 6,
            Self::Bytes8K => 14,
        }
    }

    fn block_mask(self) -> u32 {
        (self.capacity() / EEPROM_BLOCK_LEN - 1) as u32
    }
}

/// Serial receive/transmit state of the selected EEPROM part.
///
/// `received` collects the bits of an in-flight command, `read_bits` holds the
/// 68-bit answer of a completed read request and `read_pos` counts the answer
/// bits already consumed. `Cell` lets the read-only bus path advance the answer
/// position without a second mutable state.
#[derive(Debug)]
pub(super) struct EepromState {
    received: [bool; MAX_COMMAND_BITS],
    received_len: usize,
    read_bits: [bool; READ_STREAM_BITS],
    read_len: usize,
    read_pos: Cell<usize>,
}

impl Default for EepromState {
    fn default() -> Self {
        Self {
            received: [false; MAX_COMMAND_BITS],
            received_len: 0,
            read_bits: [false; READ_STREAM_BITS],
            read_len: 0,
            read_pos: Cell::new(0),
        }
    }
}

impl EepromState {
    /// Consumes one bit from a DMA3 store, executing a command once complete.
    pub(super) fn receive_bit(
        &mut self,
        size: EepromSize,
        backup: &mut [u8],
        addr: u32,
        bit: bool,
    ) -> Result<()> {
        if self.received_len >= MAX_COMMAND_BITS {
            self.reset_command();
            return Err(Error::UnsupportedSave { addr });
        }
        if self.received_len == 0 {
            // A new /CS-low transaction supersedes any unread old answer.
            self.read_len = 0;
            self.read_pos.set(0);
        }
        self.received[self.received_len] = bit;
        self.received_len += 1;
        if self.received_len < 2 {
            return Ok(());
        }
        match (self.received[0], self.received[1]) {
            (true, true) => {
                let total = 2 + size.address_bits() + 1;
                if self.received_len == total {
                    let address = self.address(size);
                    let trailing = self.received[total - 1];
                    self.reset_command();
                    if trailing {
                        return Err(Error::UnsupportedSave { addr });
                    }
                    self.prepare_read(backup, address);
                }
            }
            (true, false) => {
                let total = 2 + size.address_bits() + 64 + 1;
                if self.received_len == total {
                    let address = self.address(size);
                    let trailing = self.received[total - 1];
                    let bits = self.received;
                    self.reset_command();
                    if trailing {
                        return Err(Error::UnsupportedSave { addr });
                    }
                    apply_write(size, backup, address, &bits);
                    // A completed write is ready immediately; polling reads 1.
                    self.read_len = 0;
                    self.read_pos.set(0);
                }
            }
            _ => {
                self.reset_command();
                return Err(Error::UnsupportedSave { addr });
            }
        }
        Ok(())
    }

    /// Returns the next serial output bit during DMA3; `1` means ready when no
    /// read answer is pending. A pending data stream cannot be consumed by a
    /// manual LDRH because only DMA3 drives the serial transfer.
    pub(super) fn read_bit(&self, dma_active: bool, addr: u32) -> Result<u16> {
        let position = self.read_pos.get();
        if position < self.read_len {
            if !dma_active {
                return Err(Error::UnsupportedSave { addr });
            }
            self.read_pos.set(position + 1);
            Ok(u16::from(self.read_bits[position]))
        } else {
            Ok(1)
        }
    }

    /// Ends the /CS-low DMA transaction; incomplete commands stay explicit.
    pub(super) fn finish_dma(&mut self) -> Result<()> {
        let incomplete = self.received_len != 0;
        self.reset_command();
        if incomplete {
            Err(Error::UnsupportedSave {
                addr: EEPROM_WINDOW_BASE,
            })
        } else {
            Ok(())
        }
    }

    /// Appends the transient serial state to a guest-state digest.
    pub(super) fn append_digest(&self, bytes: &mut Vec<u8>) {
        bytes.push(self.received_len as u8);
        for bit in &self.received[..self.received_len] {
            bytes.push(u8::from(*bit));
        }
        bytes.push(self.read_len as u8);
        bytes.extend_from_slice(&(self.read_pos.get() as u16).to_le_bytes());
        for bit in &self.read_bits[..self.read_len] {
            bytes.push(u8::from(*bit));
        }
    }

    fn reset_command(&mut self) {
        self.received_len = 0;
    }

    fn address(&self, size: EepromSize) -> u32 {
        let mut value = 0_u32;
        for index in 0..size.address_bits() {
            value = (value << 1) | u32::from(self.received[2 + index]);
        }
        value & size.block_mask()
    }

    fn prepare_read(&mut self, backup: &[u8], address: u32) {
        self.read_pos.set(0);
        for bit in &mut self.read_bits[..4] {
            *bit = false;
        }
        let offset = address as usize * EEPROM_BLOCK_LEN;
        for index in 0..64 {
            let byte = backup[offset + index / 8];
            self.read_bits[4 + index] = (byte >> (7 - index % 8)) & 1 != 0;
        }
        self.read_len = READ_STREAM_BITS;
    }
}

fn apply_write(size: EepromSize, backup: &mut [u8], address: u32, bits: &[bool]) {
    let offset = address as usize * EEPROM_BLOCK_LEN;
    let data_start = 2 + size.address_bits();
    for index in 0..64 {
        let mask = 1_u8 << (7 - index % 8);
        let byte = &mut backup[offset + index / 8];
        if bits[data_start + index] {
            *byte |= mask;
        } else {
            *byte &= !mask;
        }
    }
}

impl Bus {
    /// Selected EEPROM capacity, or `None` for every other backup policy.
    pub(super) fn eeprom_size(&self) -> Option<EepromSize> {
        match self.backup_policy {
            BackupPolicy::Eeprom512Volatile => Some(EepromSize::Bytes512),
            BackupPolicy::Eeprom8KVolatile => Some(EepromSize::Bytes8K),
            _ => None,
        }
    }

    /// Whether `addr` is the documented EEPROM serial window.
    pub(super) fn is_eeprom_window(addr: u32) -> bool {
        (EEPROM_WINDOW_BASE..EEPROM_WINDOW_BASE + EEPROM_WINDOW_LEN).contains(&addr)
    }

    /// Answers a halfword read in the EEPROM window (DMA data or ready poll).
    pub(super) fn eeprom_read16(&self, addr: u32) -> Result<u16> {
        if !Self::is_eeprom_window(addr) {
            return Err(Error::Unmapped { addr });
        }
        self.eeprom.read_bit(self.eeprom_dma_active, addr)
    }

    /// Accepts one serial bit from a DMA3 halfword store.
    ///
    /// Manual stores are rejected because GBATEK documents that they do not
    /// keep the chip selected through the transfer.
    pub(super) fn eeprom_write16(&mut self, addr: u32, value: u16) -> Result<()> {
        if !Self::is_eeprom_window(addr) {
            return Err(Error::Unmapped { addr });
        }
        if !self.eeprom_dma_active {
            return Err(Error::UnsupportedSave { addr });
        }
        let size = self.eeprom_size().ok_or(Error::UnsupportedSave { addr })?;
        self.eeprom
            .receive_bit(size, &mut self.backup, addr, value & 1 != 0)
    }

    /// Ends a DMA3 EEPROM transaction, discarding any partial command bits.
    pub(super) fn eeprom_finish_dma(&mut self) -> Result<()> {
        self.eeprom.finish_dma()
    }
}
