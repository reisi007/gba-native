use super::{Bus, KEYINPUT_RELEASED};
use crate::sha256;

impl Bus {
    /// SWI 01h group 7 over the modelled "other I/O" set.
    pub fn reset_group7_io(&mut self) {
        self.io.reset_group7();
    }

    /// SWI 01h group 5 over the modelled serial-register storage.
    pub fn reset_group5_sio(&mut self) {
        self.io.reset_group5();
    }

    /// SWI 01h group 6 over the modelled sound-register storage.
    pub fn reset_group6_sound(&mut self) {
        self.io.reset_group6();
    }

    pub fn state_digest(&self) -> [u8; 32] {
        let mut bytes = Vec::with_capacity(
            self.ewram.len()
                + self.iwram.len()
                + self.palette.len()
                + self.vram.len()
                + self.oam.len()
                + self.backup.len()
                + 10,
        );
        bytes.extend_from_slice(&self.ewram);
        bytes.extend_from_slice(&self.iwram);
        bytes.extend_from_slice(&self.palette);
        bytes.extend_from_slice(&self.vram);
        bytes.extend_from_slice(&self.oam);
        self.io.append_digest(&mut bytes);
        // Policy is host configuration; mutable GPIO register/input state is guest-visible.
        bytes.extend_from_slice(&self.backup);
        bytes.push(self.flash.command as u8);
        bytes.push(self.flash.bank);
        self.eeprom.append_digest(&mut bytes);
        bytes.extend_from_slice(&(KEYINPUT_RELEASED & !self.pressed_keys).to_le_bytes());
        bytes.extend_from_slice(&[
            self.gpio_data,
            self.gpio_direction,
            self.gpio_control,
            self.gpio_input,
        ]);
        sha256::digest(&bytes)
    }

    pub fn vram_digest(&self) -> [u8; 32] {
        sha256::digest(&self.vram)
    }
}
