use super::{Cpu, Mode};

impl Cpu {
    /// Read a register from the User bank without changing the active mode.
    pub fn user_reg(&self, index: u8) -> u32 {
        match index {
            8..=12 if self.mode() == Mode::Fiq => self.high[index as usize - 8],
            13 => self.sp[0],
            14 => self.lr[0],
            _ => self.reg(index),
        }
    }

    /// Write a register in the User bank without changing the active mode.
    pub fn set_user_reg(&mut self, index: u8, value: u32) {
        match index {
            8..=12 if self.mode() == Mode::Fiq => self.high[index as usize - 8] = value,
            13 => self.sp[0] = value,
            14 => self.lr[0] = value,
            _ => self.set_reg(index, value),
        }
    }
}
