//! Host-side save library: multiple named save slots per ROM digest.
//!
//! Saves are stored outside the cartridge as raw backup images under
//! `<root>/<rom-sha256>/<slot>.sav`. The ROM SHA-256 is the only cartridge
//! identity; no ROM bytes are copied. A `<root>/<rom-sha256>/default` text file
//! records the preferred slot. The format is deliberately plain and
//! dependency-free so native and browser frontends can share the same model.
//!
//! This is persistence for the explicitly selected backup-device models. It is
//! not a save-device compatibility claim and it does not embed retail save data.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Maximum accepted backup image length (the documented flash size).
pub const MAX_SAVE_LEN: usize = 128 * 1024;
/// Maximum slot-name length.
pub const MAX_SLOT_NAME: usize = 32;

/// Save-library failures.
#[derive(Debug)]
pub enum SaveError {
    Io(io::Error),
    InvalidRomDigest,
    InvalidSlotName,
    InvalidSaveLength(usize),
    UnknownSlot(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::InvalidRomDigest => {
                write!(formatter, "ROM digest must be 64 lowercase hex chars")
            }
            Self::InvalidSlotName => write!(
                formatter,
                "slot name must be 1..={MAX_SLOT_NAME} chars of A-Z, a-z, 0-9, '.', '_' or '-'"
            ),
            Self::InvalidSaveLength(len) => write!(
                formatter,
                "save image length {len} is not 1..={MAX_SAVE_LEN} bytes"
            ),
            Self::UnknownSlot(slot) => write!(formatter, "unknown save slot {slot:?}"),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<SaveError> for crate::Error {
    fn from(error: SaveError) -> Self {
        match error {
            SaveError::Io(error) => Self::Io(error.to_string()),
            other => Self::Args(other.to_string()),
        }
    }
}

pub type SaveResult<T> = Result<T, SaveError>;

/// True for a lowercase or uppercase hex SHA-256 digest.
#[must_use]
pub fn valid_rom_digest(digest: &str) -> bool {
    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// True for a slot name that cannot escape the save directory.
#[must_use]
pub fn valid_slot_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_SLOT_NAME
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Directory-backed save library.
#[derive(Clone, Debug)]
pub struct SaveLibrary {
    root: PathBuf,
}

impl SaveLibrary {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn rom_dir(&self, digest: &str) -> SaveResult<PathBuf> {
        if !valid_rom_digest(digest) {
            return Err(SaveError::InvalidRomDigest);
        }
        Ok(self.root.join(digest.to_ascii_lowercase()))
    }

    fn slot_path(&self, digest: &str, slot: &str) -> SaveResult<PathBuf> {
        if !valid_slot_name(slot) {
            return Err(SaveError::InvalidSlotName);
        }
        Ok(self.rom_dir(digest)?.join(format!("{slot}.sav")))
    }

    fn default_path(&self, digest: &str) -> SaveResult<PathBuf> {
        Ok(self.rom_dir(digest)?.join("default"))
    }

    /// Existing slots, sorted by name.
    pub fn list(&self, digest: &str) -> SaveResult<Vec<String>> {
        let dir = self.rom_dir(digest)?;
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut slots = Vec::new();
        for entry in entries {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(slot) = name.strip_suffix(".sav") else {
                continue;
            };
            if valid_slot_name(slot) && entry.path().is_file() {
                slots.push(slot.to_owned());
            }
        }
        slots.sort();
        Ok(slots)
    }

    pub fn exists(&self, digest: &str, slot: &str) -> SaveResult<bool> {
        Ok(self.slot_path(digest, slot)?.is_file())
    }

    pub fn load(&self, digest: &str, slot: &str) -> SaveResult<Vec<u8>> {
        let path = self.slot_path(digest, slot)?;
        match fs::read(&path) {
            Ok(bytes) => {
                if bytes.is_empty() || bytes.len() > MAX_SAVE_LEN {
                    return Err(SaveError::InvalidSaveLength(bytes.len()));
                }
                Ok(bytes)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(SaveError::UnknownSlot(slot.to_owned()))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Store a slot and make it default when no default exists yet.
    pub fn store(&self, digest: &str, slot: &str, bytes: &[u8]) -> SaveResult<()> {
        if bytes.is_empty() || bytes.len() > MAX_SAVE_LEN {
            return Err(SaveError::InvalidSaveLength(bytes.len()));
        }
        let path = self.slot_path(digest, slot)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_atomic(&path, bytes)?;
        if self.default_slot(digest)?.is_none() {
            self.set_default(digest, slot)?;
        }
        Ok(())
    }

    pub fn delete(&self, digest: &str, slot: &str) -> SaveResult<()> {
        let path = self.slot_path(digest, slot)?;
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(SaveError::UnknownSlot(slot.to_owned()));
            }
            Err(error) => return Err(error.into()),
        }
        if self.default_slot(digest)?.is_none() {
            let _ = fs::remove_file(self.default_path(digest)?);
            if let Some(next) = self.list(digest)?.first() {
                self.set_default(digest, next)?;
            }
        }
        Ok(())
    }

    pub fn set_default(&self, digest: &str, slot: &str) -> SaveResult<()> {
        if !self.exists(digest, slot)? {
            return Err(SaveError::UnknownSlot(slot.to_owned()));
        }
        let path = self.default_path(digest)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_atomic(&path, slot.as_bytes())
    }

    pub fn default_slot(&self, digest: &str) -> SaveResult<Option<String>> {
        let path = self.default_path(digest)?;
        match fs::read_to_string(&path) {
            Ok(text) => {
                let slot = text.trim();
                if valid_slot_name(slot) && self.exists(digest, slot)? {
                    Ok(Some(slot.to_owned()))
                } else {
                    Ok(None)
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Resolve an explicit slot or the stored default.
    pub fn resolve(&self, digest: &str, requested: Option<&str>) -> SaveResult<Option<String>> {
        match requested {
            Some(slot) => {
                if self.exists(digest, slot)? {
                    Ok(Some(slot.to_owned()))
                } else {
                    Err(SaveError::UnknownSlot(slot.to_owned()))
                }
            }
            None => self.default_slot(digest),
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> SaveResult<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes)?;
    // Windows rename fails when the destination exists; remove first. The
    // caller owns the save directory and no other process should write there.
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let unique = format!(
            "gba-native-save-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).unwrap();
        path
    }

    const DIGEST: &str = "8a7667d43d9ec0333a5a7c0a032837919e101ca1c1893bdf5f95f51b27555539";
    const OTHER: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    #[test]
    fn multiple_slots_are_independent_and_default_is_tracked() {
        let root = temp_root("slots");
        let library = SaveLibrary::new(&root);
        assert_eq!(library.list(DIGEST).unwrap(), Vec::<String>::new());
        library.store(DIGEST, "first", &[0x47, 0x53]).unwrap();
        library.store(DIGEST, "second", &[0x41, 0x56]).unwrap();
        assert_eq!(library.list(DIGEST).unwrap(), ["first", "second"]);
        assert_eq!(library.load(DIGEST, "first").unwrap(), [0x47, 0x53]);
        assert_eq!(library.load(DIGEST, "second").unwrap(), [0x41, 0x56]);
        assert_eq!(
            library.default_slot(DIGEST).unwrap().as_deref(),
            Some("first")
        );
        library.set_default(DIGEST, "second").unwrap();
        assert_eq!(
            library.default_slot(DIGEST).unwrap().as_deref(),
            Some("second")
        );
        assert_eq!(
            library.resolve(DIGEST, None).unwrap().as_deref(),
            Some("second")
        );
        assert_eq!(
            library.resolve(DIGEST, Some("first")).unwrap().as_deref(),
            Some("first")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_the_default_falls_back_to_another_slot() {
        let root = temp_root("delete");
        let library = SaveLibrary::new(&root);
        library.store(DIGEST, "alpha", &[1]).unwrap();
        library.store(DIGEST, "beta", &[2]).unwrap();
        library.delete(DIGEST, "alpha").unwrap();
        assert_eq!(
            library.default_slot(DIGEST).unwrap().as_deref(),
            Some("beta")
        );
        assert!(library.load(DIGEST, "alpha").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn digests_are_separate_and_names_cannot_escape_the_root() {
        let root = temp_root("names");
        let library = SaveLibrary::new(&root);
        library.store(DIGEST, "slot", &[1]).unwrap();
        library.store(OTHER, "slot", &[2]).unwrap();
        assert_eq!(library.load(DIGEST, "slot").unwrap(), [1]);
        assert_eq!(library.load(OTHER, "slot").unwrap(), [2]);
        for bad in ["", ".", "..", "../x", "a/b", "a\\b", &"x".repeat(33)] {
            assert!(matches!(
                library.store(DIGEST, bad, &[1]),
                Err(SaveError::InvalidSlotName)
            ));
        }
        assert!(matches!(
            library.list("../x"),
            Err(SaveError::InvalidRomDigest)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejected_images_do_not_create_a_slot() {
        let root = temp_root("length");
        let library = SaveLibrary::new(&root);
        assert!(matches!(
            library.store(DIGEST, "slot", &[]),
            Err(SaveError::InvalidSaveLength(0))
        ));
        let too_big = vec![0; MAX_SAVE_LEN + 1];
        assert!(matches!(
            library.store(DIGEST, "slot", &too_big),
            Err(SaveError::InvalidSaveLength(_))
        ));
        assert!(library.list(DIGEST).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
