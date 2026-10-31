//! Persisted desktop settings: path selection plus the shared file format.
//!
//! The document format, validation and safe save live in the std-only core
//! `frontend::settings` module. This module owns only the platform path. A
//! missing or malformed file falls back to defaults with an explicit
//! diagnostic instead of preventing the synthetic frontend from starting.

use std::path::{Path, PathBuf};

pub use gba_native::frontend::settings::{load, save};

/// Default per-user settings path for the current platform.
pub fn default_path() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata)
            .join("gba-native")
            .join("settings.ini");
    }
    if cfg!(target_os = "macos")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("gba-native")
            .join("settings.ini");
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("gba-native").join("settings.ini");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("gba-native")
            .join("settings.ini");
    }
    PathBuf::from("gba-native-settings.ini")
}

/// Resolve an explicit path or the platform default.
pub fn path(explicit: Option<&Path>) -> PathBuf {
    explicit.map_or_else(default_path, Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_path_wins_over_the_platform_default() {
        let explicit = Path::new("/tmp/gba-native-explicit.ini");
        assert_eq!(path(Some(explicit)), explicit);
    }
}
