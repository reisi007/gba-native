//! Shared settings persistence for every GUI target.
//!
//! The document format and validation live in [`crate::input::Settings`]. This
//! module owns the platform-neutral file I/O: a missing or malformed document
//! falls back to defaults with a diagnostic instead of preventing a frontend
//! from starting. Each platform supplies its own path.

use std::path::Path;

use crate::input::Settings;

/// Load settings, returning defaults plus a diagnostic on failure.
pub fn load(path: &Path) -> (Settings, Option<String>) {
    match std::fs::read_to_string(path) {
        Ok(text) => match Settings::parse(&text) {
            Ok(settings) => (settings, None),
            Err(error) => (
                Settings::default(),
                Some(format!(
                    "{} is not a valid settings document ({error}); using defaults",
                    path.display()
                )),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Settings::default(), None),
        Err(error) => (
            Settings::default(),
            Some(format!(
                "cannot read {} ({error}); using defaults",
                path.display()
            )),
        ),
    }
}

/// Persist settings atomically enough for a single-user frontend: write a
/// sibling temporary file, then rename it over the target.
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("ini.tmp");
    std::fs::write(&temporary, settings.render())?;
    // Windows refuses to rename over an existing file; remove the previous
    // document first. A single-user writer has no concurrent rename.
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Button, Key};

    fn temp(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "gba-native-frontend-settings-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn missing_file_loads_defaults_without_a_warning() {
        let path = temp("missing").join("settings.ini");
        let (settings, warning) = load(&path);
        assert_eq!(settings, Settings::default());
        assert!(warning.is_none());
    }

    #[test]
    fn malformed_file_falls_back_with_a_diagnostic() {
        let path = temp("bad").join("settings.ini");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "schema=99\n").unwrap();
        let (settings, warning) = load(&path);
        assert_eq!(settings, Settings::default());
        assert!(warning.unwrap().contains("using defaults"));
    }

    #[test]
    fn save_and_load_round_trip_preserves_bindings_and_modes() {
        let path = temp("roundtrip").join("nested").join("settings.ini");
        let mut settings = Settings {
            fullscreen: true,
            touch_controls: false,
            ..Settings::default()
        };
        settings
            .bindings
            .set(Button::A, vec![Key::from_code("Digit1").unwrap()])
            .unwrap();
        save(&path, &settings).expect("save");
        let (loaded, warning) = load(&path);
        assert!(warning.is_none());
        assert_eq!(loaded, settings);
        assert!(!path.with_extension("ini.tmp").exists());
    }
}
