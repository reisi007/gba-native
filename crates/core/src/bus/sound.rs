//! Sound registers plus bounded Sound A/B FIFOs. No audio is generated.
//!
//! FIFO sample consumption and FIFO-DMA requests are explicit host events. No
//! timer, frame or sample clock is synthesized by this storage model.

use super::Bus;
use crate::error::{Error, Result};

pub(super) const SOUND_BASE: u32 = 0x0400_0060;
pub(super) const SOUND_LEN: usize = 0x48;
pub const FIFO_A_ADDR: u32 = 0x0400_00A0;
pub const FIFO_B_ADDR: u32 = 0x0400_00A4;
const FIFO_CAPACITY: usize = 32;
const SOUNDCNT_H_ADDR: u32 = 0x0400_0082;

/// One of the two write-only Direct Sound FIFOs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundFifo {
    A,
    B,
}

impl SoundFifo {
    pub const fn address(self) -> u32 {
        match self {
            Self::A => FIFO_A_ADDR,
            Self::B => FIFO_B_ADDR,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct ByteFifo {
    bytes: [u8; FIFO_CAPACITY],
    head: u8,
    len: u8,
}

impl ByteFifo {
    fn len(self) -> usize {
        usize::from(self.len)
    }

    fn available(self) -> usize {
        FIFO_CAPACITY - self.len()
    }

    fn push_word(&mut self, value: u32) -> bool {
        if self.available() < 4 {
            return false;
        }
        for byte in value.to_le_bytes() {
            let tail = (usize::from(self.head) + self.len()) & (FIFO_CAPACITY - 1);
            self.bytes[tail] = byte;
            self.len += 1;
        }
        true
    }

    fn pop_sample(&mut self) -> Option<i8> {
        if self.len == 0 {
            return None;
        }
        let sample = self.bytes[usize::from(self.head)] as i8;
        self.head = (self.head + 1) & (FIFO_CAPACITY as u8 - 1);
        self.len -= 1;
        Some(sample)
    }

    fn clear(&mut self) {
        *self = Self::default();
    }

    fn append_digest(self, bytes: &mut Vec<u8>) {
        bytes.push(self.len);
        for index in 0..FIFO_CAPACITY {
            let value = if index < self.len() {
                self.bytes[(usize::from(self.head) + index) & (FIFO_CAPACITY - 1)]
            } else {
                0
            };
            bytes.push(value);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct SoundState {
    regs: [u8; SOUND_LEN],
    fifo_a: ByteFifo,
    fifo_b: ByteFifo,
}

impl Default for SoundState {
    fn default() -> Self {
        Self {
            regs: [0; SOUND_LEN],
            fifo_a: ByteFifo::default(),
            fifo_b: ByteFifo::default(),
        }
    }
}

impl SoundState {
    fn is_fifo_address(addr: u32) -> bool {
        (FIFO_A_ADDR..FIFO_B_ADDR + 4).contains(&addr)
    }

    fn fifo(&self, fifo: SoundFifo) -> &ByteFifo {
        match fifo {
            SoundFifo::A => &self.fifo_a,
            SoundFifo::B => &self.fifo_b,
        }
    }

    fn fifo_mut(&mut self, fifo: SoundFifo) -> &mut ByteFifo {
        match fifo {
            SoundFifo::A => &mut self.fifo_a,
            SoundFifo::B => &mut self.fifo_b,
        }
    }

    fn offset(addr: u32) -> Option<usize> {
        let offset = addr.checked_sub(SOUND_BASE)? as usize;
        (offset < SOUND_LEN).then_some(offset)
    }

    pub(super) fn read8(&self, addr: u32) -> Option<u8> {
        if Self::is_fifo_address(addr) {
            return None;
        }
        Some(self.regs[Self::offset(addr)?])
    }

    pub(super) fn write8(&mut self, addr: u32, value: u8) -> bool {
        if Self::is_fifo_address(addr) {
            return false;
        }
        let Some(offset) = Self::offset(addr) else {
            return false;
        };
        if addr == SOUNDCNT_H_ADDR + 1 {
            if value & (1 << 3) != 0 {
                self.fifo_a.clear();
            }
            if value & (1 << 7) != 0 {
                self.fifo_b.clear();
            }
        }
        self.regs[offset] = value;
        true
    }

    /// Apply only the documented RegisterRamReset group-6 effects that are
    /// sufficiently specified in the reviewed behavior report. Other sound
    /// bytes stay untouched rather than guessing ambiguous end values.
    pub(super) fn reset_group6(&mut self) {
        self.regs[0x20..0x22].fill(0); // SOUNDCNT_L
        self.regs[0x22..0x24].copy_from_slice(&0x880Eu16.to_le_bytes()); // SOUNDCNT_H
        self.regs[0x24..0x26].fill(0); // SOUNDCNT_X ends disabled
        let bias = u16::from_le_bytes([self.regs[0x28], self.regs[0x29]]) & 0x03FF;
        self.regs[0x28..0x2A].copy_from_slice(&bias.to_le_bytes());
        self.regs[0x30..0x40].fill(0); // 16-byte wave-RAM area
        self.fifo_a.clear();
        self.fifo_b.clear();
    }

    pub(super) fn append_digest(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&self.regs);
        self.fifo_a.append_digest(bytes);
        self.fifo_b.append_digest(bytes);
    }

    pub(super) fn write16(&mut self, addr: u32, value: u16) -> bool {
        if Self::is_fifo_address(addr) {
            return false;
        }
        let Some(offset) = Self::offset(addr) else {
            return false;
        };
        if addr == SOUNDCNT_H_ADDR {
            if value & (1 << 11) != 0 {
                self.fifo_a.clear();
            }
            if value & (1 << 15) != 0 {
                self.fifo_b.clear();
            }
        }
        self.regs[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        true
    }

    pub(super) fn fifo_for_word_address(addr: u32) -> Option<SoundFifo> {
        match addr {
            FIFO_A_ADDR => Some(SoundFifo::A),
            FIFO_B_ADDR => Some(SoundFifo::B),
            _ => None,
        }
    }

    /// Snapshot of the write-only PSG register file (offsets from `0x04000060`).
    /// The audio renderer is a host-side consumer; guest reads stay unchanged.
    pub(super) fn register_file(&self) -> [u8; SOUND_LEN] {
        self.regs
    }

    pub(super) fn fifo_len(&self, fifo: SoundFifo) -> usize {
        self.fifo(fifo).len()
    }

    pub(super) fn fifo_available(&self, fifo: SoundFifo) -> usize {
        self.fifo(fifo).available()
    }

    pub(super) fn push_fifo_word(&mut self, fifo: SoundFifo, value: u32) -> Result<()> {
        let available = self.fifo(fifo).available();
        if available < 4 {
            return Err(Error::SoundFifoFull {
                addr: fifo.address(),
                needed: 4,
                available,
            });
        }
        if self.fifo_mut(fifo).push_word(value) {
            return Ok(());
        }
        unreachable!("available FIFO capacity was checked before the push")
    }

    pub(super) fn pop_fifo_sample(&mut self, fifo: SoundFifo) -> Option<i8> {
        self.fifo_mut(fifo).pop_sample()
    }
}

impl Bus {
    pub(super) fn sound_read8(&self, addr: u32) -> Option<u8> {
        self.io.sound().read8(addr)
    }

    pub(super) fn sound_write8(&mut self, addr: u32, value: u8) -> bool {
        self.io.sound_mut().write8(addr, value)
    }

    pub(super) fn sound_write_fifo_word(&mut self, fifo: SoundFifo, value: u32) -> Result<()> {
        self.io.sound_mut().push_fifo_word(fifo, value)
    }

    pub(super) fn sound_write_fifo_word_at(&mut self, addr: u32, value: u32) -> Option<Result<()>> {
        let fifo = SoundState::fifo_for_word_address(addr)?;
        Some(self.sound_write_fifo_word(fifo, value))
    }

    pub(super) fn sound_fifo_available(&self, fifo: SoundFifo) -> usize {
        self.io.sound().fifo_available(fifo)
    }

    /// Bytes currently queued in the modeled FIFO; no sound clock consumes them.
    pub fn sound_fifo_len(&self, fifo: SoundFifo) -> usize {
        self.io.sound().fifo_len(fifo)
    }

    /// Consume one signed 8-bit sample as an explicit host event. This does not
    /// synthesize a timer tick or automatically request more DMA data.
    pub fn pop_sound_fifo_sample(&mut self, fifo: SoundFifo) -> Option<i8> {
        self.io.sound_mut().pop_fifo_sample(fifo)
    }

    /// The block is byte-addressed state, so wider accesses compose bytes.
    pub(super) fn sound_read16(&self, addr: u32) -> Option<u16> {
        let low = self.sound_read8(addr)? as u16;
        let high = self.sound_read8(addr + 1)? as u16;
        Some(low | (high << 8))
    }

    pub(super) fn sound_write16(&mut self, addr: u32, value: u16) -> bool {
        self.io.sound_mut().write16(addr, value)
    }
}
