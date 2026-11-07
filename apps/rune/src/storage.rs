//! Settings persistence adapters for native Rune windows and the Rune browser shell.

use gba_native::input::Settings;

#[cfg(any(test, not(target_arch = "wasm32")))]
use std::path::Path;
#[cfg(any(test, not(target_arch = "wasm32")))]
use std::path::PathBuf;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function runeReadSettings() {
  try { return globalThis.localStorage?.getItem("gba-native.rune.settings") ?? null; }
  catch (_) { return null; }
}
export function runeWriteSettings(value) {
  try { globalThis.localStorage?.setItem("gba-native.rune.settings", value); return true; }
  catch (_) { return false; }
}
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeReadSettings)]
    fn read_browser_settings() -> Option<String>;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeWriteSettings)]
    fn write_browser_settings(value: &str) -> bool;
}

pub(super) fn load_settings() -> (Settings, Option<String>) {
    #[cfg(test)]
    return (Settings::default(), None);

    #[cfg(all(not(test), target_arch = "wasm32"))]
    {
        match read_browser_settings() {
            Some(document) => parse_document(&document),
            None => (Settings::default(), None),
        }
    }

    #[cfg(all(not(test), not(target_arch = "wasm32")))]
    {
        let Some(path) = settings_path() else {
            return (
                Settings::default(),
                Some("No settings directory is available; using defaults".into()),
            );
        };
        load_native_path(&path)
    }
}

pub(super) fn save_settings(settings: &Settings) -> Result<(), String> {
    #[cfg(test)]
    {
        let _ = settings;
        Ok(())
    }

    #[cfg(all(not(test), target_arch = "wasm32"))]
    {
        write_browser_settings(&settings.render())
            .then_some(())
            .ok_or_else(|| "browser localStorage is unavailable".to_owned())
    }

    #[cfg(all(not(test), not(target_arch = "wasm32")))]
    {
        let path =
            settings_path().ok_or_else(|| "no settings directory is available".to_owned())?;
        save_native_path(&path, settings)
            .map_err(|error| format!("cannot save {}: {error}", path.display()))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn saves_root() -> Option<PathBuf> {
    #[cfg(test)]
    {
        Some(std::env::temp_dir().join(format!("gba-native-rune-saves-{}", std::process::id())))
    }
    #[cfg(all(not(test), target_os = "ios"))]
    {
        let home = std::env::var_os("HOME")?;
        let root = ios_documents_root(Path::new(&home)).join("saves");
        std::fs::create_dir_all(&root).ok()?;
        Some(root)
    }
    #[cfg(all(not(test), not(target_os = "ios")))]
    {
        settings_path().map(|path| {
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .join("saves")
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn tone_path() -> Option<PathBuf> {
    #[cfg(test)]
    {
        Some(std::env::temp_dir().join(format!("gba-native-rune-tone-{}.wav", std::process::id())))
    }
    #[cfg(all(not(test), target_os = "ios"))]
    {
        let home = std::env::var_os("HOME")?;
        Some(ios_documents_root(Path::new(&home)).join("gba-native-tone.wav"))
    }
    #[cfg(all(not(test), not(target_os = "ios")))]
    {
        settings_path().map(|path| {
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .join("gba-native-tone.wav")
        })
    }
}

#[cfg(any(test, target_os = "ios"))]
fn ios_documents_root(home: &Path) -> PathBuf {
    home.join("Documents/GBA Native Rune")
}

#[cfg(all(not(test), not(target_arch = "wasm32"), target_os = "windows"))]
fn settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|root| PathBuf::from(root).join("gba-native/settings.ini"))
}

#[cfg(all(
    not(test),
    not(target_arch = "wasm32"),
    any(target_os = "macos", target_os = "ios")
))]
fn settings_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|root| PathBuf::from(root).join("Library/Application Support/gba-native/settings.ini"))
}

#[cfg(all(not(test), not(target_arch = "wasm32"), target_os = "linux"))]
fn settings_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|root| PathBuf::from(root).join(".config")))
        .map(|root| root.join("gba-native/settings.ini"))
}

#[cfg(all(
    not(test),
    not(target_arch = "wasm32"),
    not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "linux"
    ))
))]
fn settings_path() -> Option<PathBuf> {
    None
}

#[cfg(all(not(target_arch = "wasm32"), any(not(test), target_os = "linux")))]
fn load_native_path(path: &Path) -> (Settings, Option<String>) {
    gba_native::frontend::settings::load(path)
}

#[cfg(all(not(target_arch = "wasm32"), any(not(test), target_os = "linux")))]
fn save_native_path(path: &Path, settings: &Settings) -> Result<(), String> {
    gba_native::frontend::settings::save(path, settings).map_err(|error| error.to_string())
}

#[cfg(any(test, target_arch = "wasm32"))]
fn parse_document(document: &str) -> (Settings, Option<String>) {
    match Settings::parse(document) {
        Ok(settings) => (settings, None),
        Err(error) => (
            Settings::default(),
            Some(format!(
                "Invalid browser settings ({error}); using defaults"
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_document_parser_round_trips_and_rejects_invalid_values() {
        let settings = Settings {
            fullscreen: true,
            master_volume: 37,
            ..Settings::default()
        };
        let (loaded, warning) = parse_document(&settings.render());
        assert!(warning.is_none());
        assert_eq!(loaded, settings);
        let (fallback, warning) = parse_document("schema=99\n");
        assert_eq!(fallback, Settings::default());
        assert!(warning.unwrap().contains("using defaults"));
    }

    #[test]
    fn ios_user_artifacts_live_under_the_files_shared_documents_folder() {
        let home = Path::new("/app/container");
        let shared = ios_documents_root(home);
        assert_eq!(shared, home.join("Documents/GBA Native Rune"));
        assert_eq!(
            shared.join("saves/import.sav"),
            home.join("Documents/GBA Native Rune/saves/import.sav")
        );
        assert_eq!(
            shared.join("gba-native-tone.wav"),
            home.join("Documents/GBA Native Rune/gba-native-tone.wav")
        );
    }

    #[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
    #[test]
    fn native_shared_settings_adapter_round_trips_a_private_temp_document() {
        let path = std::env::temp_dir().join(format!(
            "gba-native-rune-settings-{}-{}.ini",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let settings = Settings {
            touch_controls: false,
            master_volume: 73,
            ..Settings::default()
        };
        save_native_path(&path, &settings).expect("save through core settings adapter");
        let (loaded, warning) = load_native_path(&path);
        assert!(warning.is_none());
        assert_eq!(loaded, settings);
        std::fs::remove_file(path).expect("remove test settings document");
    }
}
