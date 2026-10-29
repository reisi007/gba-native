//! Game Pak backup window: explicit policies for volatile SRAM and flash.
//!
//! The prototype has no persistence. The strict policy reports every access in
//! the 64 KiB backup window as an explicit error. The opt-ins model a
//! byte-wide, volatile SRAM or a 128 KiB flash with the documented command
//! subset; all start erased. The backing store is a fixed 128 KiB erased image
//! so the guest-state digest does not depend on the selected host policy; the
//! policy only gates the accessible capacity.

use super::*;

/// Start of the documented Game Pak backup window.
pub const SAVE_BASE: u32 = 0x0E00_0000;
/// Length of the documented backup window.
pub const SAVE_WINDOW_LEN: u32 = 0x0001_0000;
/// Modelled volatile 32 KiB SRAM length.
pub const SAVE_LEN: usize = 32 * 1024;
/// Modelled volatile 64 KiB SRAM length (the documented window maximum).
pub const SAVE_LEN_64K: usize = 64 * 1024;
/// Modelled 128 KiB flash backing length (two 64 KiB banks).
pub const SAVE_LEN_FLASH: usize = 128 * 1024;

/// Host-selected model for the 64 KiB Game Pak backup window.
///
/// The prototype has no persistence. The strict policy reports every access as
/// [`Error::UnsupportedSave`]. The SRAM opt-ins model byte-accessible storage
/// that starts erased (`0xFF`), is lost when the process ends, and has no flash
/// command engine. The flash opt-in models two 64 KiB banks with the documented
/// autoselect/bank-switch/program/erase subset. Neither model claims general
/// save-device or flash compatibility.
///
/// This is a device model, not a save feature: nothing is written to disk.
/// The selected policy is host configuration and is excluded from
/// [`Bus::state_digest`]; only the SRAM contents are guest state and are
/// hashed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupPolicy {
    /// Fail closed: every backup-window access returns [`Error::UnsupportedSave`].
    Strict,
    /// **Experimental opt-in:** volatile 32 KiB SRAM, no persistence.
    Sram32KVolatile,
    /// **Experimental opt-in:** volatile 64 KiB SRAM (window maximum).
    Sram64KVolatile,
    /// **Experimental opt-in:** volatile 128 KiB flash (two 64 KiB banks)
    /// with the documented autoselect/program/erase command subset.
    Flash128KVolatile,
}

impl BackupPolicy {
    /// Stable label for CLI output and snapshot reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Sram32KVolatile => "sram-32k-volatile",
            Self::Sram64KVolatile => "sram-64k-volatile",
            Self::Flash128KVolatile => "flash-128k-volatile",
        }
    }

    /// Backing-store length for the selected device model.
    pub(super) fn capacity(self) -> usize {
        match self {
            Self::Strict | Self::Sram32KVolatile => SAVE_LEN,
            Self::Sram64KVolatile => SAVE_LEN_64K,
            Self::Flash128KVolatile => SAVE_LEN_FLASH,
        }
    }
}

/// Flash command state. Only the documented autoselect/bank/program/erase
/// subset needed by the device probe is modelled.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum FlashCommand {
    #[default]
    Array,
    Unlock1,
    Unlock2,
    Autoselect,
    BankSwitch,
    Program,
    Erase,
    EraseUnlock1,
    EraseUnlock2,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct FlashState {
    pub(super) command: FlashCommand,
    pub(super) bank: u8,
}

impl Bus {
    /// Applies the selected backup-device model to a read in the save window.
    ///
    /// The selected SRAM/flash device model answers; the rest of the 64 KiB
    /// window and every wider access stay explicit errors because the 8-bit
    /// backup bus is not composed here.
    pub(super) fn save_read(&self, addr: u32, width: usize) -> Result<u32> {
        match self.backup_policy {
            BackupPolicy::Strict => Err(Error::UnsupportedSave { addr }),
            BackupPolicy::Sram32KVolatile | BackupPolicy::Sram64KVolatile => {
                let offset = (addr - SAVE_BASE) as usize;
                if width != 1 || offset >= self.backup_policy.capacity() {
                    return Err(Error::UnsupportedSave { addr });
                }
                Ok(u32::from(self.backup[offset]))
            }
            BackupPolicy::Flash128KVolatile => self.flash_read(addr, width),
        }
    }

    /// Applies the selected backup-device model to a store in the save window.
    pub(super) fn save_write(&mut self, addr: u32, value: u32, width: usize) -> Result<()> {
        match self.backup_policy {
            BackupPolicy::Strict => Err(Error::UnsupportedSave { addr }),
            BackupPolicy::Sram32KVolatile | BackupPolicy::Sram64KVolatile => {
                let offset = (addr - SAVE_BASE) as usize;
                if width != 1 || offset >= self.backup_policy.capacity() {
                    return Err(Error::UnsupportedSave { addr });
                }
                self.backup[offset] = value as u8;
                Ok(())
            }
            BackupPolicy::Flash128KVolatile => self.flash_write(addr, value, width),
        }
    }

    fn flash_read(&self, addr: u32, width: usize) -> Result<u32> {
        if width != 1 {
            return Err(Error::UnsupportedSave { addr });
        }
        let offset = (addr - SAVE_BASE) as usize;
        if self.flash.command == FlashCommand::Autoselect {
            match offset {
                0 => return Ok(0x62),
                1 => return Ok(0x13),
                _ => {}
            }
        }
        let index = usize::from(self.flash.bank) * 0x1_0000 + offset;
        Ok(u32::from(self.backup[index]))
    }

    fn flash_write(&mut self, addr: u32, value: u32, width: usize) -> Result<()> {
        if width != 1 {
            return Err(Error::UnsupportedSave { addr });
        }
        let offset = (addr - SAVE_BASE) as usize;
        let byte = value as u8;
        match self.flash.command {
            FlashCommand::Array => {
                if offset == 0x5555 && byte == 0xAA {
                    self.flash.command = FlashCommand::Unlock1;
                }
            }
            FlashCommand::Unlock1 => {
                self.flash.command = if offset == 0x2AAA && byte == 0x55 {
                    FlashCommand::Unlock2
                } else {
                    FlashCommand::Array
                };
            }
            FlashCommand::Unlock2 => {
                self.flash.command = if offset == 0x5555 {
                    match byte {
                        0x90 => FlashCommand::Autoselect,
                        0xB0 => FlashCommand::BankSwitch,
                        0x80 => FlashCommand::Erase,
                        0xA0 => FlashCommand::Program,
                        0xF0 => FlashCommand::Array,
                        _ => FlashCommand::Array,
                    }
                } else {
                    FlashCommand::Array
                };
            }
            FlashCommand::BankSwitch => {
                if offset == 0 {
                    self.flash.bank = byte & 1;
                }
                self.flash.command = FlashCommand::Array;
            }
            FlashCommand::Autoselect => {
                if byte == 0xF0 {
                    self.flash.command = FlashCommand::Array;
                }
            }
            FlashCommand::Program => {
                let index = usize::from(self.flash.bank) * 0x1_0000 + offset;
                self.backup[index] = byte;
                self.flash.command = FlashCommand::Array;
            }
            FlashCommand::Erase => {
                self.flash.command = if offset == 0x5555 && byte == 0xAA {
                    FlashCommand::EraseUnlock1
                } else {
                    FlashCommand::Array
                };
            }
            FlashCommand::EraseUnlock1 => {
                self.flash.command = if offset == 0x2AAA && byte == 0x55 {
                    FlashCommand::EraseUnlock2
                } else {
                    FlashCommand::Array
                };
            }
            FlashCommand::EraseUnlock2 => {
                match byte {
                    0x10 => self.backup.fill(0xFF),
                    0x30 => {
                        let index = usize::from(self.flash.bank) * 0x1_0000 + (offset & !0x0FFF);
                        let end = (index + 0x1000).min(self.backup.len());
                        self.backup[index..end].fill(0xFF);
                    }
                    _ => {}
                }
                self.flash.command = FlashCommand::Array;
            }
        }
        Ok(())
    }
}
