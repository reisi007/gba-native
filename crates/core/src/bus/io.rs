use crate::error::{Error, Result};

use super::dma::DmaState;
use super::irq::IrqState;
use super::sound::SoundState;
use super::timer::TimerState;
use super::{
    BG0CNT_ADDR, BG0HOFS_ADDR, BG0VOFS_ADDR, DISPCNT_ADDR, KEYINPUT_ADDR, KEYINPUT_RELEASED,
    RCNT_ADDR, SIOCNT_ADDR, SIODATA8_ADDR, SIODATA32_H_ADDR, SIODATA32_L_ADDR, WAITCNT_ADDR,
};

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

/// Valid host pressed-mask bits in KEYINPUT order: A, B, Select, Start, Right,
/// Left, Up, Down, R, L.
pub const KEY_MASK: u16 = KEYINPUT_RELEASED;

/// SIOCNT bit 2: opponent SO is high, i.e. no link partner is connected.
const SIOCNT_SI_HIGH: u16 = 1 << 2;
/// SIOCNT bit 7: start/active.
const SIOCNT_START: u16 = 1 << 7;
/// SIOCNT bit 13 selects multiplay/UART over normal mode; bit 12 is the
/// 8/32-bit transfer length in normal mode.
const SIOCNT_MODE_MASK: u16 = 0x2000;
/// SIOCNT bit 0: internal shift clock.
const SIOCNT_INTERNAL_CLOCK: u16 = 0x0001;
/// SIOCNT bits the guest can change; bit 2 and bit 15 are read-only.
const SIOCNT_WRITABLE: u16 = 0x7F8B;

impl IoState {
    /// Completes an internally clocked transfer with no connected partner.
    ///
    /// The documented disconnected-port state drives SI high, so every received
    /// bit is one. The prototype has no cycle timing: the transfer is finished
    /// when the start bit is written, and the start bit reads back cleared. An
    /// externally clocked start is not completed, because no partner supplies SC
    /// edges and the guest must observe a transfer that never ends.
    fn finish_unconnected_transfer(&mut self) {
        self.siocnt &= !SIOCNT_START;
        if self.siocnt & 0x1000 != 0 {
            self.siodata_l = 0xFFFF;
            self.siodata_h = 0xFFFF;
        } else {
            self.siodata8 = 0x00FF;
        }
    }
}

#[derive(Default)]
pub(super) struct IoState {
    dispcnt: u16,
    bg0cnt: u16,
    bg0hofs: u16,
    bg0vofs: u16,
    waitcnt: u16,
    siodata_l: u16,
    siodata_h: u16,
    siocnt: u16,
    siodata8: u16,
    rcnt: u16,
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
            SIODATA32_L_ADDR => Ok(self.siodata_l),
            SIODATA32_H_ADDR => Ok(self.siodata_h),
            SIOCNT_ADDR => Ok(self.siocnt | SIOCNT_SI_HIGH),
            SIODATA8_ADDR => Ok(self.siodata8),
            RCNT_ADDR => Ok(self.rcnt),
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
            // Serial data registers keep their documented storage value.
            SIODATA32_L_ADDR => {
                self.siodata_l = value;
                Ok(())
            }
            SIODATA32_H_ADDR => {
                self.siodata_h = value;
                Ok(())
            }
            SIODATA8_ADDR => {
                self.siodata8 = value & 0x00FF;
                Ok(())
            }
            // RCNT keeps its documented R/W bits; bits 9..13 read as zero.
            RCNT_ADDR => {
                self.rcnt = value & 0xC1FF;
                Ok(())
            }
            // SIOCNT is documented R/W storage. Only a normal-mode transfer with
            // the internal shift clock can be answered, and only because a
            // disconnected port has a documented state. Multiplay, UART,
            // GP/JOY and external-clock transfers stay explicit errors.
            SIOCNT_ADDR => {
                let starting = value & SIOCNT_START != 0;
                // GBATEK: in general-purpose/JOY-BUS mode `SIOCNT` is not used
                // for transfers, so the register is plain storage there.
                let general_purpose = self.rcnt & 0x8000 != 0;
                let normal_mode = value & SIOCNT_MODE_MASK == 0;
                let internal_clock = value & SIOCNT_INTERNAL_CLOCK != 0;
                // Multiplay and UART need peer/baud behaviour this prototype
                // does not model, so a start there stays an explicit error.
                if starting && !general_purpose && !normal_mode {
                    return Some(Err(Error::UnsupportedIoWrite { addr, width: 16 }));
                }
                self.siocnt = value & SIOCNT_WRITABLE;
                if starting && !general_purpose && internal_clock {
                    self.finish_unconnected_transfer();
                }
                // With the external shift clock and no partner there are no SC
                // edges, so the start bit stays set and the data registers keep
                // their values. The guest observes a transfer that never ends.
                Ok(())
            }
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

    /// RegisterRamReset group 5: clears the serial registers and selects the
    /// documented general-purpose mode by setting `RCNT` bit 15.
    pub(super) fn reset_group5(&mut self) {
        self.siodata_l = 0;
        self.siodata_h = 0;
        self.siocnt = 0;
        self.siodata8 = 0;
        self.rcnt = 0x8000;
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
        bytes.extend_from_slice(&self.siodata_l.to_le_bytes());
        bytes.extend_from_slice(&self.siodata_h.to_le_bytes());
        bytes.extend_from_slice(&self.siocnt.to_le_bytes());
        bytes.extend_from_slice(&self.siodata8.to_le_bytes());
        bytes.extend_from_slice(&self.rcnt.to_le_bytes());
        self.dma.append_digest(bytes);
        self.irq.append_digest(bytes);
        self.sound.append_digest(bytes);
        self.timer.append_digest(bytes);
    }
}
