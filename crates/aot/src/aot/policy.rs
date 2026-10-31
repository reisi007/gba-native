//! Host-selected synthetic device policies bound into a native or wasm harness.
//!
//! The default is the fail-closed runtime policy. Every non-default field is
//! an explicit experimental opt-in, mirroring `gba-native run`; generated
//! native harnesses print the effective policy names so a report cannot be
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

/// Render the bus-construction statements for a generated harness.
pub(super) fn bus_setup_source(policy: HarnessPolicy) -> String {
    let mut out = String::new();
    if policy.bios_zero_fill {
        out.push_str("    let mut bus = Bus::with_synthetic_bios_zero_fill(cart);\n");
    } else {
        out.push_str("    let mut bus = Bus::new(cart);\n");
    }
    if policy.bios_rom_writes {
        out.push_str(
            "    bus = bus.with_bios_write_policy(gba_native::bus::BiosWritePolicy::RomWritesDropped);\n",
        );
    }
    if policy.rom_writes_dropped {
        out.push_str(
            "    bus = bus.with_rom_write_policy(gba_native::bus::RomWritePolicy::Dropped);\n",
        );
    }
    if policy.backup != BackupPolicy::Strict {
        out.push_str(&format!(
            "    bus = bus.with_backup_policy(gba_native::bus::BackupPolicy::{});\n",
            backup_variant(policy)
        ));
    }
    if policy.decompression {
        out.push_str(
            "    bus = bus.with_decompression_policy(gba_native::bios::DecompressionPolicy::DocumentedFormats);\n",
        );
    }
    if policy.affine {
        out.push_str(
            "    bus = bus.with_affine_policy(gba_native::bios::AffinePolicy::Documented);\n",
        );
    }
    out
}

fn backup_variant(policy: HarnessPolicy) -> &'static str {
    match policy.backup {
        BackupPolicy::Strict => "Strict",
        BackupPolicy::Sram32KVolatile => "Sram32KVolatile",
        BackupPolicy::Sram64KVolatile => "Sram64KVolatile",
        BackupPolicy::Flash128KVolatile => "Flash128KVolatile",
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
