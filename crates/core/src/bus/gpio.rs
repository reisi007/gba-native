use super::{Bus, GpioPolicy};
use crate::error::{Error, Result};

pub(super) const GPIO_BASE: u32 = 0x0800_00C4;

impl Bus {
    pub(super) fn has_gpio_overlay(&self, addr: u32) -> bool {
        self.gpio_policy == GpioPolicy::DocumentedStorage
            && (GPIO_BASE..=GPIO_BASE + 5).contains(&addr)
    }

    pub(super) fn gpio_read8(&self, addr: u32) -> Result<u8> {
        if self.gpio_control & 1 == 0 {
            return self.cart.read_u8(addr).ok_or(Error::RomOverread {
                addr,
                len: self.cart.len(),
            });
        }
        Err(Error::UnsupportedCartGpio {
            addr,
            width: 8,
            write: false,
        })
    }

    pub(super) fn gpio_read16(&self, addr: u32) -> Result<u16> {
        if self.gpio_control & 1 == 0 {
            return self.cart.read_u16(addr).ok_or(Error::RomOverread {
                addr,
                len: self.cart.len(),
            });
        }
        match addr {
            0x0800_00C4 => Ok(u16::from(self.gpio_data())),
            0x0800_00C6 => Ok(u16::from(self.gpio_direction)),
            0x0800_00C8 => Ok(u16::from(self.gpio_control)),
            _ => Err(Error::UnsupportedCartGpio {
                addr,
                width: 16,
                write: false,
            }),
        }
    }

    pub(super) fn gpio_read32(&self, addr: u32) -> Result<u32> {
        if self.gpio_control & 1 == 0 {
            return self.cart.read_u32(addr).ok_or(Error::RomOverread {
                addr,
                len: self.cart.len(),
            });
        }
        let low = u32::from(self.read16(addr)?);
        let high = u32::from(self.read16(addr + 2)?);
        Ok(low | (high << 16))
    }

    pub(super) fn gpio_write8(&mut self, addr: u32) -> Result<()> {
        Err(Error::UnsupportedCartGpio {
            addr,
            width: 8,
            write: true,
        })
    }

    pub(super) fn gpio_write16(&mut self, addr: u32, value: u16) -> Result<()> {
        match addr {
            0x0800_00C4 => self.gpio_data = value as u8 & 0x0F,
            0x0800_00C6 => self.gpio_direction = value as u8 & 0x0F,
            0x0800_00C8 => self.gpio_control = value as u8 & 1,
            _ => {
                return Err(Error::UnsupportedCartGpio {
                    addr,
                    width: 16,
                    write: true,
                });
            }
        }
        Ok(())
    }

    pub(super) fn gpio_write32(&mut self, addr: u32, value: u32) -> Result<()> {
        self.write16(addr, value as u16)?;
        self.write16(addr + 2, (value >> 16) as u16)
    }
}
