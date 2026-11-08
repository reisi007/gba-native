//! Rune's declaration of the shared app-facing feature contract.

use gba_native::frontend::features::Feature;

/// Features exposed by the iOS Rune shell and its isolated adapter.
pub const IOS_FEATURES: &[Feature] = &[
    Feature::DemoFrame,
    Feature::TouchControls,
    Feature::Fullscreen,
    Feature::Settings,
    Feature::AudioArtifact,
    Feature::AudioPlayback,
    Feature::SaveSlots,
    Feature::SaveImportExport,
];

#[cfg(not(target_os = "ios"))]
pub const FEATURES: &[Feature] = Feature::ALL;

/// Features available in the iOS Rune shell and its native adapter.
#[cfg(target_os = "ios")]
pub const FEATURES: &[Feature] = IOS_FEATURES;

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_os = "ios"))]
    use gba_native::frontend::features::{FEATURE_IDS, ids_of};

    #[cfg(not(target_os = "ios"))]
    #[test]
    fn host_and_browser_feature_parity_matches_the_shared_contract() {
        assert_eq!(ids_of(FEATURES), FEATURE_IDS);
    }

    #[test]
    fn ios_feature_declaration_matches_the_isolated_native_adapters() {
        assert!(IOS_FEATURES.contains(&Feature::AudioPlayback));
        assert!(IOS_FEATURES.contains(&Feature::AudioArtifact));
        assert!(IOS_FEATURES.contains(&Feature::SaveImportExport));
        assert!(rune_ios_platform::supports(
            rune_ios_platform::Capability::TestTonePlayback
        ));
        assert!(rune_ios_platform::supports(
            rune_ios_platform::Capability::SaveImportPicker
        ));
    }
}
