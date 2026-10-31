//! Shared save-image library for every GUI target.
//!
//! The synthetic apps never execute a ROM, so slots are keyed by the same
//! synthetic "save-tester" digest the browser panel uses. Bytes are raw backup
//! images; named slots, the default marker, validation and atomic writes come
//! from [`crate::save::SaveLibrary`]. The browser keeps the same fields in
//! `localStorage`, so the feature set is equal on every target.

use std::path::PathBuf;

use crate::save::{SaveLibrary, SaveResult};

/// The committed save-tester fixture's identity, shared with `apps/web`.
pub const SYNTHETIC_ROM_SHA: &str =
    "f8dbe41f4dcd1e134759a23cce988b4a05f1185f102b3153d69240600e76c4e3";

/// Largest raw backup image a slot may hold.
pub const MAX_SAVE_BYTES: usize = 128 * 1024;

/// Raw image a native frontend imports when the user taps "import".
pub const IMPORT_FILE: &str = "import.sav";

/// One stored slot as the settings/saves pages display it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveSlot {
    pub name: String,
    pub bytes: usize,
    pub default: bool,
}

/// Directory-backed save slots for the synthetic app.
#[derive(Clone, Debug)]
pub struct SaveSlots {
    library: SaveLibrary,
}

impl SaveSlots {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            library: SaveLibrary::new(root),
        }
    }

    /// Existing slots with length and default marker, sorted by name.
    pub fn slots(&self) -> SaveResult<Vec<SaveSlot>> {
        let default = self.library.default_slot(SYNTHETIC_ROM_SHA)?;
        let mut slots = Vec::new();
        for name in self.library.list(SYNTHETIC_ROM_SHA)? {
            let bytes = self.library.load(SYNTHETIC_ROM_SHA, &name)?.len();
            slots.push(SaveSlot {
                default: default.as_deref() == Some(name.as_str()),
                name,
                bytes,
            });
        }
        Ok(slots)
    }

    pub fn store(&self, name: &str, bytes: &[u8]) -> SaveResult<()> {
        self.library.store(SYNTHETIC_ROM_SHA, name, bytes)
    }

    pub fn load(&self, name: &str) -> SaveResult<Vec<u8>> {
        self.library.load(SYNTHETIC_ROM_SHA, name)
    }

    /// Root directory that holds the synthetic slots.
    pub fn root(&self) -> &std::path::Path {
        self.library.root()
    }

    /// Import `<root>/import.sav` as the `imported` slot.
    pub fn import_from_file(&self) -> SaveResult<String> {
        let path = self.root().join(IMPORT_FILE);
        let bytes = std::fs::read(&path)?;
        self.import("imported", &bytes)?;
        Ok("imported".to_string())
    }

    /// Export a slot to `<root>/export-<name>.sav` and return that path.
    pub fn export_to_file(&self, name: &str) -> SaveResult<std::path::PathBuf> {
        let bytes = self.export(name)?;
        let path = self.root().join(format!("export-{name}.sav"));
        std::fs::write(&path, bytes)?;
        Ok(path)
    }

    /// Export a slot's raw bytes, for a file or a browser download.
    pub fn export(&self, name: &str) -> SaveResult<Vec<u8>> {
        self.load(name)
    }

    /// Import raw slot bytes under a validated name.
    pub fn import(&self, name: &str, bytes: &[u8]) -> SaveResult<()> {
        self.store(name, bytes)
    }

    pub fn delete(&self, name: &str) -> SaveResult<()> {
        self.library.delete(SYNTHETIC_ROM_SHA, name)
    }

    pub fn set_default(&self, name: &str) -> SaveResult<()> {
        self.library.set_default(SYNTHETIC_ROM_SHA, name)
    }

    pub fn default_slot(&self) -> SaveResult<Option<String>> {
        self.library.default_slot(SYNTHETIC_ROM_SHA)
    }

    /// Deterministic raw image used by the "new slot" action.
    #[must_use]
    pub fn synthetic_image(seed: u8) -> Vec<u8> {
        let mut bytes = vec![0u8; 512];
        bytes[..8].copy_from_slice(b"GBANSV01");
        bytes[8] = seed;
        for (index, byte) in bytes.iter_mut().enumerate().skip(9) {
            *byte = (index as u8).wrapping_mul(31).wrapping_add(seed);
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gba-native-frontend-saves-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn storing_slots_sets_the_first_default_and_round_trips_bytes() {
        let root = temp("roundtrip");
        let slots = SaveSlots::new(&root);
        assert!(slots.slots().expect("list").is_empty());
        slots
            .store("intro", &SaveSlots::synthetic_image(1))
            .expect("store");
        slots
            .store("route-2", &SaveSlots::synthetic_image(2))
            .expect("store");

        let listed = slots.slots().expect("list");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "intro");
        assert!(listed[0].default);
        assert!(!listed[1].default);
        assert_eq!(
            slots.default_slot().expect("default"),
            Some("intro".to_string())
        );
        assert_eq!(
            slots.load("route-2").expect("load"),
            SaveSlots::synthetic_image(2)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn default_moves_with_set_default_and_deletion_falls_back() {
        let root = temp("default");
        let slots = SaveSlots::new(&root);
        slots
            .store("a", &SaveSlots::synthetic_image(1))
            .expect("store");
        slots
            .store("b", &SaveSlots::synthetic_image(2))
            .expect("store");
        slots.set_default("b").expect("default");
        assert_eq!(
            slots.default_slot().expect("default"),
            Some("b".to_string())
        );

        slots.delete("b").expect("delete");
        let listed = slots.slots().expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "a");
        assert!(listed[0].default);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn import_and_export_round_trip_raw_images() {
        let root = temp("import-export");
        let slots = SaveSlots::new(&root);
        let bytes = SaveSlots::synthetic_image(9);
        slots.import("imported", &bytes).expect("import");
        assert_eq!(slots.export("imported").expect("export"), bytes);
        assert_eq!(
            slots.default_slot().expect("default"),
            Some("imported".into())
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn file_import_and_export_round_trip() {
        let root = temp("files");
        let slots = SaveSlots::new(&root);
        std::fs::create_dir_all(slots.root()).expect("root");
        let bytes = SaveSlots::synthetic_image(4);
        std::fs::write(slots.root().join(IMPORT_FILE), &bytes).expect("import file");
        assert_eq!(slots.import_from_file().expect("import"), "imported");
        let exported = slots.export_to_file("imported").expect("export");
        assert_eq!(std::fs::read(exported).expect("read"), bytes);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_import_file_is_an_error() {
        let root = temp("missing-import");
        let slots = SaveSlots::new(&root);
        assert!(slots.import_from_file().is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_names_and_unknown_slots_are_errors() {
        let root = temp("invalid");
        let slots = SaveSlots::new(&root);
        assert!(
            slots
                .store("../escape", &SaveSlots::synthetic_image(1))
                .is_err()
        );
        assert!(slots.load("missing").is_err());
        assert!(slots.delete("missing").is_err());
        assert!(slots.set_default("missing").is_err());
        assert!(slots.store("empty", &[]).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_synthetic_digest_matches_the_browser_panel() {
        // apps/web/www/saves.mjs uses the same digest for its synthetic ROM.
        assert_eq!(SYNTHETIC_ROM_SHA.len(), 64);
        assert!(
            SYNTHETIC_ROM_SHA
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
        assert_eq!(SaveSlots::synthetic_image(0).len(), 512);
    }
}
