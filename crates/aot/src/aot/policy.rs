//! Host-selected synthetic device policies bound into a native harness.
//!
//! The default is the fail-closed runtime policy. Every non-default field is
//! an explicit experimental opt-in, mirroring `gba-native run`; generated
//! harnesses print the effective policy names so a native report cannot be
//! mistaken for a default-policy run.

use gba_native::bus::BackupPolicy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HarnessPolicy {
    pub bios_zero_fill: bool,
    pub bios_rom_writes: bool,
    pub rom_writes_dropped: bool,
    pub backup: BackupPolicy,
    pub decompression: bool,
    pub affine: bool,
}

impl Default for HarnessPolicy {
    fn default() -> Self {
        Self {
            bios_zero_fill: false,
            bios_rom_writes: false,
            rom_writes_dropped: false,
            backup: BackupPolicy::Strict,
            decompression: false,
            affine: false,
        }
    }
}

impl HarnessPolicy {
    pub fn is_strict(self) -> bool {
        self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_strict() {
        let policy = HarnessPolicy::default();
        assert!(policy.is_strict());
        assert_eq!(policy.backup, BackupPolicy::Strict);
    }
}
