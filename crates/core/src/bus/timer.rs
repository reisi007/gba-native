//! Timer registers `0x04000100..0x0400010F`: storage only.
//!
//! **Stated limitation.** Timers do not tick. `TM0..TM3` keep the halfwords the
//! guest writes, including the enable, cascade, interrupt and prescaler bits,
//! and read back unchanged. There is no cycle counter to drive them, so no
//! counter ever increments and no timer interrupt is raised. Code that polls a
//! running timer will therefore not advance; that is recorded here rather than
//! hidden behind a fabricated clock.

use super::Bus;

pub(super) const TIMER_BASE: u32 = 0x0400_0100;
pub(super) const TIMER_LEN: usize = 16;

#[derive(Clone, Copy)]
pub(super) struct TimerState {
    regs: [u8; TIMER_LEN],
}

impl Default for TimerState {
    fn default() -> Self {
        Self {
            regs: [0; TIMER_LEN],
        }
    }
}

impl TimerState {
    fn offset(addr: u32) -> Option<usize> {
        let offset = addr.checked_sub(TIMER_BASE)? as usize;
        (offset < TIMER_LEN).then_some(offset)
    }

    pub(super) fn read16(&self, addr: u32) -> Option<u16> {
        let offset = Self::offset(addr)?;
        Some(u16::from_le_bytes([
            self.regs[offset],
            self.regs[offset + 1],
        ]))
    }

    pub(super) fn write16(&mut self, addr: u32, value: u16) -> bool {
        let Some(offset) = Self::offset(addr) else {
            return false;
        };
        self.regs[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
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
    pub(super) fn timer_read16(&self, addr: u32) -> Option<u16> {
        self.io.timer().read16(addr)
    }

    pub(super) fn timer_write16(&mut self, addr: u32, value: u16) -> bool {
        self.io.timer_mut().write16(addr, value)
    }
}
