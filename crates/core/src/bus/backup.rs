//! Game Pak backup window: explicit policy and the modelled volatile SRAM.
//!
//! The prototype has no persistence and no flash command engine. The strict
//! policy reports every access in the 64 KiB backup window as an explicit
//! error; the opt-ins model a byte-wide, volatile SRAM that starts erased. The
//! 32 KiB and 64 KiB capacities are separate, explicitly selected device
//! models (64 KiB is the documented window maximum). The backing store is a
//! fixed 64 KiB erased image so the guest-state digest does not depend on the
//! selected host policy; the policy only gates the accessible capacity.

use super::*;

/// Start of the documented Game Pak backup window.
pub const SAVE_BASE: u32 = 0x0E00_0000;
/// Length of the documented backup window.
pub const SAVE_WINDOW_LEN: u32 = 0x0001_0000;
/// Modelled volatile 32 KiB SRAM length.
pub const SAVE_LEN: usize = 32 * 1024;
/// Modelled volatile 64 KiB SRAM length (the documented window maximum).
pub const SAVE_LEN_64K: usize = 64 * 1024;

/// Host-selected model for the 64 KiB Game Pak backup window.
///
/// The prototype has no persistence. The strict policy reports every access as
/// [`Error::UnsupportedSave`]. The explicit opt-ins model a volatile SRAM:
/// byte-accessible storage that starts erased (`0xFF`), is lost when the
/// process ends, and has no flash command engine. The model answers such a
/// window with plain SRAM bytes; it does not implement, emulate or claim any
/// flash device-ID or command sequence.
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
}

impl BackupPolicy {
    /// Stable label for CLI output and snapshot reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Sram32KVolatile => "sram-32k-volatile",
            Self::Sram64KVolatile => "sram-64k-volatile",
        }
    }

    /// Backing-store length for the selected device model.
    pub(super) fn capacity(self) -> usize {
        match self {
            Self::Strict | Self::Sram32KVolatile => SAVE_LEN,
            Self::Sram64KVolatile => SAVE_LEN_64K,
        }
    }
}

impl Bus {
    /// Applies the selected backup-device model to a read in the save window.
    ///
    /// Only the selected SRAM capacity answers; the rest of the 64 KiB window
    /// and every wider access stay explicit errors because the 8-bit backup
    /// bus is not composed here.
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
        }
    }
}
