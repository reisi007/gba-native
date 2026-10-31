//! Desktop declaration of the shared app feature contract.
//!
//! The test compares this list with `gba_native::frontend::features`, so the
//! desktop target fails CI if it loses a feature the contract requires.

#[cfg(test)]
pub use gba_native::frontend::features::Feature;

#[cfg(test)]
pub const FEATURES: &[Feature] = &[
    Feature::DemoFrame,
    Feature::TouchControls,
    Feature::Fullscreen,
    Feature::Settings,
    Feature::AudioArtifact,
    Feature::AudioPlayback,
    Feature::SaveSlots,
    Feature::SaveImportExport,
];

#[cfg(test)]
mod tests {
    use super::*;
    use gba_native::frontend::features::{FEATURE_IDS, ids_of};

    #[test]
    fn feature_parity_with_the_shared_contract() {
        assert_eq!(ids_of(FEATURES), FEATURE_IDS);
    }
}
