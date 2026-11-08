//! The iOS-only system adapters used by Rune.
//!
//! Objective-C interaction is intentionally isolated in this crate. The Rune
//! application crate remains safe Rust and consumes only the small platform API
//! below.

#[cfg(any(target_os = "ios", test))]
mod staging;

#[cfg(target_os = "ios")]
mod ios;

/// iOS-only features supplied by this adapter crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// Play the core-generated test-tone WAV through AVFoundation.
    TestTonePlayback,
    /// Pick and stage a bounded `.sav` document for Rune's importer.
    SaveImportPicker,
}

/// The capabilities actually implemented by the iOS adapter.
pub const IOS_CAPABILITIES: &[Capability] =
    &[Capability::TestTonePlayback, Capability::SaveImportPicker];

/// Whether the isolated adapter implements an iOS capability.
#[must_use]
pub fn supports(capability: Capability) -> bool {
    IOS_CAPABILITIES.contains(&capability)
}

#[cfg(target_os = "ios")]
pub use ios::{play_test_tone, present_save_import_picker, take_save_import_result};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ios_adapter_exposes_only_implemented_platform_features() {
        assert!(supports(Capability::TestTonePlayback));
        assert!(supports(Capability::SaveImportPicker));
        assert_eq!(IOS_CAPABILITIES.len(), 2);
    }
}
