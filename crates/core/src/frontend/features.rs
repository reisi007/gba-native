//! Canonical app feature contract.
//!
//! Every GUI target declares the features it implements and a test compares that
//! declaration with this list, so a target cannot silently lose a feature. The
//! list is also the single source the browser Node test parses; the ids are
//! stable, lowercase and kebab-case.

/// Every app-facing feature, in contract order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    DemoFrame,
    TouchControls,
    Fullscreen,
    Settings,
    AudioArtifact,
    AudioPlayback,
    SaveSlots,
    SaveImportExport,
}

impl Feature {
    /// Contract order; every target must declare this same set.
    pub const ALL: &'static [Feature] = &[
        Self::DemoFrame,
        Self::TouchControls,
        Self::Fullscreen,
        Self::Settings,
        Self::AudioArtifact,
        Self::AudioPlayback,
        Self::SaveSlots,
        Self::SaveImportExport,
    ];

    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::DemoFrame => "demo-frame",
            Self::TouchControls => "touch-controls",
            Self::Fullscreen => "fullscreen",
            Self::Settings => "settings",
            Self::AudioArtifact => "audio-artifact",
            Self::AudioPlayback => "audio-playback",
            Self::SaveSlots => "save-slots",
            Self::SaveImportExport => "save-import-export",
        }
    }
}

/// The app-facing feature ids every target must expose.
pub const FEATURE_IDS: &[&str] = &[
    "demo-frame",
    "touch-controls",
    "fullscreen",
    "settings",
    "audio-artifact",
    "audio-playback",
    "save-slots",
    "save-import-export",
];

/// Number of features in the contract.
pub const FEATURE_COUNT: usize = FEATURE_IDS.len();

/// The ids as a slice, for per-target comparisons.
#[must_use]
pub fn ids() -> &'static [&'static str] {
    FEATURE_IDS
}

/// Map a declared feature set to its ids, for per-target comparisons.
#[must_use]
pub fn ids_of(features: &[Feature]) -> Vec<&'static str> {
    features.iter().map(|feature| feature.id()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_id_is_unique_and_kebab_case() {
        let unique: BTreeSet<&str> = FEATURE_IDS.iter().copied().collect();
        assert_eq!(unique.len(), FEATURE_IDS.len());
        for id in FEATURE_IDS {
            assert!(!id.is_empty());
            assert!(
                id.bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'-'),
                "{id} is not kebab-case"
            );
        }
    }

    #[test]
    fn the_contract_covers_every_feature() {
        assert_eq!(FEATURE_COUNT, Feature::ALL.len());
    }

    #[test]
    fn the_enum_and_the_id_list_agree() {
        assert_eq!(ids_of(Feature::ALL), FEATURE_IDS);
    }
}
