use crate::error::{Error, Result};

use super::{
    BG0CNT_ADDR, BG0HOFS_ADDR, BG0VOFS_ADDR, DISPCNT_ADDR, KEYINPUT_ADDR, KEYINPUT_RELEASED,
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
            KEYINPUT_ADDR => Err(Error::UnsupportedIoWrite { addr, width: 16 }),
            _ => return None,
        })
    }

    /// SWI 01h group 7 over the *modelled* "other I/O" set. DISPCNT is
    /// deliberately absent: the service forces blank separately, and GBATEK
    /// documents the forced-blank value surviving the group-7 clear.
    pub(super) fn reset_group7(&mut self) {
        self.bg0cnt = 0;
        self.bg0hofs = 0;
        self.bg0vofs = 0;
    }

    pub(super) fn append_digest(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&self.dispcnt.to_le_bytes());
        bytes.extend_from_slice(&self.bg0cnt.to_le_bytes());
        bytes.extend_from_slice(&self.bg0hofs.to_le_bytes());
        bytes.extend_from_slice(&self.bg0vofs.to_le_bytes());
    }
}
