//! Sound register block `0x04000060..0x040000A7`: storage only.
//!
//! **Stated limitation.** No audio is generated and no channel state is
//! computed. Every documented sound register (channel control, `SOUNDCNT_*`,
//! `SOUNDBIAS`, wave RAM and both FIFOs) keeps exactly the bytes the guest
//! wrote, so a sound driver can read its own configuration back and see a
//! consistent picture. Channel-status bits therefore read as "not playing",
//! which is the truth here: nothing plays.

use super::Bus;

pub(super) const SOUND_BASE: u32 = 0x0400_0060;
pub(super) const SOUND_LEN: usize = 0x48;

#[derive(Clone, Copy)]
pub(super) struct SoundState {
    regs: [u8; SOUND_LEN],
}

impl Default for SoundState {
    fn default() -> Self {
        Self {
            regs: [0; SOUND_LEN],
        }
    }
}

impl SoundState {
    fn offset(addr: u32) -> Option<usize> {
        let offset = addr.checked_sub(SOUND_BASE)? as usize;
        (offset < SOUND_LEN).then_some(offset)
    }

    pub(super) fn read8(&self, addr: u32) -> Option<u8> {
        Some(self.regs[Self::offset(addr)?])
    }

    pub(super) fn write8(&mut self, addr: u32, value: u8) -> bool {
        let Some(offset) = Self::offset(addr) else {
            return false;
        };
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
    }

    pub(super) fn append_digest(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&self.regs);
    }
}

impl Bus {
    pub(super) fn sound_read8(&self, addr: u32) -> Option<u8> {
        self.io.sound().read8(addr)
    }

    pub(super) fn sound_write8(&mut self, addr: u32, value: u8) -> bool {
        self.io.sound_mut().write8(addr, value)
    }

    /// The block is byte-addressed state, so wider accesses compose bytes.
    pub(super) fn sound_read16(&self, addr: u32) -> Option<u16> {
        let low = self.sound_read8(addr)? as u16;
        let high = self.sound_read8(addr + 1)? as u16;
        Some(low | (high << 8))
    }

    pub(super) fn sound_write16(&mut self, addr: u32, value: u16) -> bool {
        let bytes = value.to_le_bytes();
        self.sound_write8(addr, bytes[0]) && self.sound_write8(addr + 1, bytes[1])
    }
}
