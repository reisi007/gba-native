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

    pub(super) fn reset(&mut self) {
        *self = Self::default();
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

    pub(super) fn sound_read32(&self, addr: u32) -> Option<u32> {
        let low = self.sound_read16(addr)? as u32;
        let high = self.sound_read16(addr + 2)? as u32;
        Some(low | (high << 16))
    }

    pub(super) fn sound_write32(&mut self, addr: u32, value: u32) -> bool {
        let halves = value.to_le_bytes();
        self.sound_write16(addr, u16::from_le_bytes([halves[0], halves[1]]))
            && self.sound_write16(addr + 2, u16::from_le_bytes([halves[2], halves[3]]))
    }
}
