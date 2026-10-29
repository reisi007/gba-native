use crate::error::{Error, Result};

use super::dma::DmaState;
use super::irq::IrqState;
use super::sound::SoundState;
use super::timer::TimerState;
use super::{
    BG0CNT_ADDR, BG0HOFS_ADDR, BG0VOFS_ADDR, BG1CNT_ADDR, BG1HOFS_ADDR, BG1VOFS_ADDR, BG2CNT_ADDR,
    BG2HOFS_ADDR, BG2PA_ADDR, BG2PB_ADDR, BG2PC_ADDR, BG2PD_ADDR, BG2VOFS_ADDR, BG2X_ADDR,
    BG2Y_ADDR, BG3CNT_ADDR, BG3HOFS_ADDR, BG3PA_ADDR, BG3PB_ADDR, BG3PC_ADDR, BG3PD_ADDR,
    BG3VOFS_ADDR, BG3X_ADDR, BG3Y_ADDR, BLDALPHA_ADDR, BLDCNT_ADDR, BLDY_ADDR, DISPCNT_ADDR,
    KEYINPUT_ADDR, KEYINPUT_RELEASED, MOSAIC_ADDR, RCNT_ADDR, SIOCNT_ADDR, SIODATA8_ADDR,
    SIODATA32_H_ADDR, SIODATA32_L_ADDR, WAITCNT_ADDR, WIN0H_ADDR, WIN0V_ADDR, WIN1H_ADDR,
    WIN1V_ADDR, WININ_ADDR, WINOUT_ADDR,
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
    bgcnt: [u16; 4],
    bghofs: [u16; 4],
    bgvofs: [u16; 4],
    win0h: u16,
    win1h: u16,
    win0v: u16,
    win1v: u16,
    winin: u16,
    winout: u16,
    mosaic: u16,
    bldcnt: u16,
    bldalpha: u16,
    bldy: u16,
    bg_affine: [[u16; 4]; 2],
    bg_reference: [[u32; 2]; 2],
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

    pub(super) fn bgcnt(&self, index: u8) -> u16 {
        self.bgcnt[index as usize]
    }

    pub(super) fn bg_scroll(&self, index: u8) -> (u16, u16) {
        (self.bghofs[index as usize], self.bgvofs[index as usize])
    }

    pub(super) fn bg_affine(&self, index: u8) -> [i16; 4] {
        let values = self.bg_affine[index as usize];
        [
            values[0] as i16,
            values[1] as i16,
            values[2] as i16,
            values[3] as i16,
        ]
    }

    pub(super) fn bg_reference(&self, index: u8) -> [u32; 2] {
        self.bg_reference[index as usize]
    }

    pub(super) fn bldalpha(&self) -> u16 {
        self.bldalpha
    }

    pub(super) fn bldcnt(&self) -> u16 {
        self.bldcnt
    }

    pub(super) fn bldy(&self) -> u16 {
        self.bldy
    }

    /// Window 0 horizontal/vertical ranges and the inside/outside masks.
    pub(super) fn window0(&self) -> (u16, u16, u16, u16) {
        (self.win0h, self.win0v, self.winin, self.winout)
    }

    pub(super) fn read16(&self, addr: u32, pressed_keys: u16) -> Option<Result<u16>> {
        Some(match addr {
            DISPCNT_ADDR => Ok(self.dispcnt),
            BG0CNT_ADDR => Ok(self.bgcnt[0]),
            BG1CNT_ADDR => Ok(self.bgcnt[1]),
            BG2CNT_ADDR => Ok(self.bgcnt[2]),
            BG3CNT_ADDR => Ok(self.bgcnt[3]),
            BG0HOFS_ADDR | BG0VOFS_ADDR | BG1HOFS_ADDR | BG1VOFS_ADDR | BG2HOFS_ADDR
            | BG2VOFS_ADDR | BG3HOFS_ADDR | BG3VOFS_ADDR => {
                Err(Error::UnsupportedIoRead { addr, width: 16 })
            }
            WIN0H_ADDR => Ok(self.win0h),
            WIN1H_ADDR => Ok(self.win1h),
            WIN0V_ADDR => Ok(self.win0v),
            WIN1V_ADDR => Ok(self.win1v),
            WININ_ADDR => Ok(self.winin),
            WINOUT_ADDR => Ok(self.winout),
            BG2PA_ADDR | BG2PB_ADDR | BG2PC_ADDR | BG2PD_ADDR => {
                let offset = (addr - BG2PA_ADDR) as usize / 2;
                Ok(self.bg_affine[0][offset])
            }
            BG3PA_ADDR | BG3PB_ADDR | BG3PC_ADDR | BG3PD_ADDR => {
                let offset = (addr - BG3PA_ADDR) as usize / 2;
                Ok(self.bg_affine[1][offset])
            }
            BG2X_ADDR | BG2Y_ADDR | BG3X_ADDR | BG3Y_ADDR => {
                let (index, offset) = if (BG2X_ADDR..=BG2Y_ADDR + 2).contains(&addr) {
                    (0, (addr - BG2X_ADDR) as usize)
                } else {
                    (1, (addr - BG3X_ADDR) as usize)
                };
                let value = self.bg_reference[index][offset / 4];
                Ok(if offset & 2 == 0 {
                    value as u16
                } else {
                    (value >> 16) as u16
                })
            }
            MOSAIC_ADDR => Ok(self.mosaic),
            BLDCNT_ADDR => Ok(self.bldcnt),
            BLDALPHA_ADDR => Ok(self.bldalpha),
            BLDY_ADDR => Ok(self.bldy),
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
                self.bgcnt[0] = value;
                Ok(())
            }
            BG1CNT_ADDR => {
                self.bgcnt[1] = value;
                Ok(())
            }
            BG2CNT_ADDR => {
                self.bgcnt[2] = value;
                Ok(())
            }
            BG3CNT_ADDR => {
                self.bgcnt[3] = value;
                Ok(())
            }
            BG0HOFS_ADDR => {
                self.bghofs[0] = value & 0x01FF;
                Ok(())
            }
            BG0VOFS_ADDR => {
                self.bgvofs[0] = value & 0x01FF;
                Ok(())
            }
            BG1HOFS_ADDR => {
                self.bghofs[1] = value & 0x01FF;
                Ok(())
            }
            BG1VOFS_ADDR => {
                self.bgvofs[1] = value & 0x01FF;
                Ok(())
            }
            BG2HOFS_ADDR => {
                self.bghofs[2] = value & 0x01FF;
                Ok(())
            }
            BG2VOFS_ADDR => {
                self.bgvofs[2] = value & 0x01FF;
                Ok(())
            }
            BG3HOFS_ADDR => {
                self.bghofs[3] = value & 0x01FF;
                Ok(())
            }
            BG3VOFS_ADDR => {
                self.bgvofs[3] = value & 0x01FF;
                Ok(())
            }
            WIN0H_ADDR => {
                self.win0h = value;
                Ok(())
            }
            WIN1H_ADDR => {
                self.win1h = value;
                Ok(())
            }
            WIN0V_ADDR => {
                self.win0v = value;
                Ok(())
            }
            WIN1V_ADDR => {
                self.win1v = value;
                Ok(())
            }
            WININ_ADDR => {
                self.winin = value;
                Ok(())
            }
            WINOUT_ADDR => {
                self.winout = value;
                Ok(())
            }
            BG2PA_ADDR | BG2PB_ADDR | BG2PC_ADDR | BG2PD_ADDR => {
                let offset = (addr - BG2PA_ADDR) as usize / 2;
                self.bg_affine[0][offset] = value;
                Ok(())
            }
            BG3PA_ADDR | BG3PB_ADDR | BG3PC_ADDR | BG3PD_ADDR => {
                let offset = (addr - BG3PA_ADDR) as usize / 2;
                self.bg_affine[1][offset] = value;
                Ok(())
            }
            BG2X_ADDR | BG2Y_ADDR | BG3X_ADDR | BG3Y_ADDR => {
                let (index, offset) = if (BG2X_ADDR..=BG2Y_ADDR + 2).contains(&addr) {
                    (0, (addr - BG2X_ADDR) as usize)
                } else {
                    (1, (addr - BG3X_ADDR) as usize)
                };
                let slot = offset / 4;
                self.bg_reference[index][slot] = if offset & 2 == 0 {
                    (self.bg_reference[index][slot] & 0xFFFF_0000) | u32::from(value)
                } else {
                    (self.bg_reference[index][slot] & 0x0000_FFFF) | (u32::from(value) << 16)
                };
                Ok(())
            }
            MOSAIC_ADDR => {
                self.mosaic = value;
                Ok(())
            }
            BLDCNT_ADDR => {
                self.bldcnt = value;
                Ok(())
            }
            BLDALPHA_ADDR => {
                self.bldalpha = value;
                Ok(())
            }
            BLDY_ADDR => {
                self.bldy = value;
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
        self.bgcnt = [0; 4];
        self.bghofs = [0; 4];
        self.bgvofs = [0; 4];
        self.win0h = 0;
        self.win1h = 0;
        self.win0v = 0;
        self.win1v = 0;
        self.winin = 0;
        self.winout = 0;
        self.mosaic = 0;
        self.bldcnt = 0;
        self.bldalpha = 0;
        self.bldy = 0;
        self.bg_affine = [[0; 4]; 2];
        self.bg_reference = [[0; 2]; 2];
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
        for value in self.bgcnt {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in self.bghofs {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in self.bgvofs {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [
            self.win0h,
            self.win1h,
            self.win0v,
            self.win1v,
            self.winin,
            self.winout,
            self.mosaic,
            self.bldcnt,
            self.bldalpha,
            self.bldy,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for values in self.bg_affine {
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for values in self.bg_reference {
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
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
