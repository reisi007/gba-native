use super::*;

fn is_serial_register(addr: u32) -> bool {
    matches!(
        addr & !1,
        SIODATA32_L_ADDR | SIODATA32_H_ADDR | SIOCNT_ADDR | SIODATA8_ADDR | RCNT_ADDR
    )
}

impl Bus {
    pub(super) fn serial_read8(&self, addr: u32) -> Option<u8> {
        if !is_serial_register(addr) {
            return None;
        }
        let value = self.io.read16(addr & !1, self.pressed_keys)?.ok()?;
        Some(if addr & 1 == 0 {
            value as u8
        } else {
            (value >> 8) as u8
        })
    }

    pub(super) fn serial_write8(&mut self, addr: u32, value: u8) -> Option<Result<()>> {
        if !is_serial_register(addr) {
            return None;
        }
        let base = addr & !1;
        let current = self.io.read16(base, self.pressed_keys)?.ok()?;
        let merged = if addr & 1 == 0 {
            (current & 0xFF00) | u16::from(value)
        } else {
            (current & 0x00FF) | (u16::from(value) << 8)
        };
        self.io.write16(base, merged, self.pressed_keys)
    }

    pub(super) fn rom_store(&self, addr: u32) -> Result<()> {
        match self.rom_write_policy {
            RomWritePolicy::Strict => Err(Error::RomWrite { addr }),
            RomWritePolicy::Dropped => Ok(()),
        }
    }

    pub(super) fn bios_store(&self, addr: u32) -> Result<()> {
        match self.bios_write_policy {
            BiosWritePolicy::Strict => Err(Error::BiosAccess { addr }),
            BiosWritePolicy::RomWritesDropped => Ok(()),
        }
    }
}
