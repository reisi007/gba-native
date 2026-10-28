use crate::error::{Error, Result};

use super::dma::DmaState;
use super::irq::IrqState;
use super::sound::SoundState;
use super::timer::TimerState;
use super::{
    BG0CNT_ADDR, BG0HOFS_ADDR, BG0VOFS_ADDR, DISPCNT_ADDR, KEYINPUT_ADDR, KEYINPUT_RELEASED,
    WAITCNT_ADDR,
};

/// Valid host pressed-mask bits in KEYINPUT order: A, B, Select, Start, Right,
/// Left, Up, Down, R, L.
pub const KEY_MASK: u16 = KEYINPUT_RELEASED;
pub const KEY_A: u16 = 1 << 0;
pub const KEY_B: u16 = 1 << 1;
pub const KEY_SELECT: u16 = 1 << 2;
pub const KEY_START: u16 = 1 << 3;
pub const KEY_RIGHT: u16 = 1 << 4;
pub const KEY_LEFT: u16 = 1 << 5;
pub const KEY_UP: u16 = 1 << 6;
pub const KEY_DOWN: u16 = 1 << 7;
pub const KEY_R: u16 = 1 << 8;
pub const KEY_L: u16 = 1 << 9;

#[derive(Default)]
pub(super) struct IoState {
    dispcnt: u16,
    bg0cnt: u16,
    bg0hofs: u16,
    bg0vofs: u16,
    waitcnt: u16,
    dma: DmaState,
    irq: IrqState,
    sound: SoundState,
    timer: TimerState,
}

impl IoState {
    pub(super) fn dispcnt(&self) -> u16 {
        self.dispcnt
    }

    pub(super) fn bg0cnt(&self) -> u16 {
        self.bg0cnt
    }

    pub(super) fn bg0_scroll(&self) -> (u16, u16) {
        (self.bg0hofs, self.bg0vofs)
    }

    pub(super) fn read16(&self, addr: u32, pressed_keys: u16) -> Option<Result<u16>> {
        Some(match addr {
            DISPCNT_ADDR => Ok(self.dispcnt),
            BG0CNT_ADDR => Ok(self.bg0cnt),
            BG0HOFS_ADDR | BG0VOFS_ADDR => Err(Error::UnsupportedIoRead { addr, width: 16 }),
            WAITCNT_ADDR => Ok(self.waitcnt),
            KEYINPUT_ADDR => Ok(KEYINPUT_RELEASED & !pressed_keys),
            _ => return None,
        })
    }

    pub(super) fn write16(&mut self, addr: u32, value: u16) -> Option<Result<()>> {
        Some(match addr {
            DISPCNT_ADDR => {
                self.dispcnt = value;
                Ok(())
            }
            BG0CNT_ADDR => {
                self.bg0cnt = value;
                Ok(())
            }
            BG0HOFS_ADDR => {
                self.bg0hofs = value & 0x01FF;
                Ok(())
            }
            BG0VOFS_ADDR => {
                self.bg0vofs = value & 0x01FF;
                Ok(())
            }
            WAITCNT_ADDR => {
                // GBATEK: bits 0..13 are the wait-state fields, 14..15 unused.
                self.waitcnt = value & 0x3FFF;
                Ok(())
            }
            // KEYINPUT is read-only on hardware: the write has no effect.
            KEYINPUT_ADDR => Ok(()),
            _ => return None,
        })
    }

    pub(super) fn dma(&self) -> &DmaState {
        &self.dma
    }

    pub(super) fn dma_mut(&mut self) -> &mut DmaState {
        &mut self.dma
    }

    pub(super) fn timer(&self) -> &TimerState {
        &self.timer
    }

    pub(super) fn timer_mut(&mut self) -> &mut TimerState {
        &mut self.timer
    }

    pub(super) fn sound(&self) -> &SoundState {
        &self.sound
    }

    pub(super) fn sound_mut(&mut self) -> &mut SoundState {
        &mut self.sound
    }

    pub(super) fn irq(&self) -> &IrqState {
        &self.irq
    }

    pub(super) fn irq_mut(&mut self) -> &mut IrqState {
        &mut self.irq
    }

    pub(super) fn reset_group7(&mut self) {
        self.bg0cnt = 0;
        self.bg0hofs = 0;
        self.bg0vofs = 0;
        self.waitcnt = 0;
        self.dma = DmaState::default();
        self.irq.reset_group7();
        self.timer.reset();
    }

    pub(super) fn reset_group6(&mut self) {
        self.sound.reset_group6();
    }

    pub(super) fn append_digest(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&self.dispcnt.to_le_bytes());
        bytes.extend_from_slice(&self.bg0cnt.to_le_bytes());
        bytes.extend_from_slice(&self.bg0hofs.to_le_bytes());
        bytes.extend_from_slice(&self.bg0vofs.to_le_bytes());
        bytes.extend_from_slice(&self.waitcnt.to_le_bytes());
        self.dma.append_digest(bytes);
        self.irq.append_digest(bytes);
        self.sound.append_digest(bytes);
        self.timer.append_digest(bytes);
    }
}
