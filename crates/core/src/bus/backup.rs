//! Game Pak backup window: explicit policy and the modelled volatile SRAM.
//!
//! The prototype has no persistence and no flash command engine. The strict
//! policy reports every access in the 64 KiB backup window as an explicit
//! error; the opt-in models a byte-wide, volatile 32 KiB SRAM that starts
//! erased. Nothing is written to disk.

use super::*;

/// Start of the documented Game Pak backup window.
pub const SAVE_BASE: u32 = 0x0E00_0000;
/// Length of the documented backup window.
pub const SAVE_WINDOW_LEN: u32 = 0x0001_0000;
/// Modelled volatile SRAM length.
pub const SAVE_LEN: usize = 32 * 1024;

/// Host-selected model for the 64 KiB Game Pak backup window.
///
/// The prototype has no persistence. The strict policy reports every access as
/// [`Error::UnsupportedSave`]. The explicit opt-in models a volatile 32 KiB
/// SRAM: byte-accessible storage that starts erased (`0xFF`), is lost when the
/// process ends, and has no flash command engine. A guest that probes for flash
/// therefore sees no flash device ID and falls back to SRAM.
///
/// This is a device model, not a save feature: nothing is written to disk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupPolicy {
    /// Fail closed: every backup-window access returns [`Error::UnsupportedSave`].
    Strict,
    /// **Experimental opt-in:** volatile 32 KiB SRAM, no persistence.
    Sram32KVolatile,
}

impl BackupPolicy {
    /// Stable label for CLI output and snapshot reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Sram32KVolatile => "sram-32k-volatile",
        }
    }
}

impl Bus {
    /// Applies the selected backup-device model to a read in the save window.
    ///
    /// Only the modelled 32 KiB SRAM answers; the window beyond it and every
    /// wider access stay explicit errors because the 8-bit backup bus is not
    /// composed here.
    pub(super) fn save_read(&self, addr: u32, width: usize) -> Result<u32> {
        match self.backup_policy {
            BackupPolicy::Strict => Err(Error::UnsupportedSave { addr }),
            BackupPolicy::Sram32KVolatile => {
                let offset = (addr - SAVE_BASE) as usize;
                if width != 1 || offset >= self.backup.len() {
                    return Err(Error::UnsupportedSave { addr });
                }
                Ok(u32::from(self.backup[offset]))
            }
        }
    }

    /// Applies the selected backup-device model to a store in the save window.
    pub(super) fn save_write(&mut self, addr: u32, value: u32, width: usize) -> Result<()> {
        match self.backup_policy {
            BackupPolicy::Strict => Err(Error::UnsupportedSave { addr }),
            BackupPolicy::Sram32KVolatile => {
                let offset = (addr - SAVE_BASE) as usize;
                if width != 1 || offset >= self.backup.len() {
                    return Err(Error::UnsupportedSave { addr });
                }
                self.backup[offset] = value as u8;
                Ok(())
            }
        }
    }
}
