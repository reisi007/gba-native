//! Host-side save library: multiple named save slots per ROM digest.
//!
//! Saves are stored outside the cartridge as raw backup images under
//! `<root>/<rom-sha256>/<slot>.sav`. The ROM SHA-256 is the only cartridge
//! identity; no ROM bytes are copied. A `<root>/<rom-sha256>/default` text file
//! records the preferred slot. Mutations require a versioned owner marker; a
//! pre-existing unmarked directories are legacy/unowned and are never adopted.
//! Their contents remain readable/listable/exportable. The format is
//! deliberately plain and dependency-free so native and browser frontends can
//! share the same model.
//!
//! This is persistence for the explicitly selected backup-device models. It is
//! not a save-device compatibility claim and it does not embed retail save data.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Maximum accepted backup image length (the documented flash size).
pub const MAX_SAVE_LEN: usize = 128 * 1024;
/// Maximum slot-name length.
pub const MAX_SLOT_NAME: usize = 32;
const OWNER_FILE: &str = ".gba-native-owner";
mod profile;
const MAX_TEMP_ATTEMPTS: usize = 128;

/// Save-library failures.
#[derive(Debug)]
pub enum SaveError {
    Io(io::Error),
    InvalidRomDigest,
    InvalidSlotName,
    InvalidProfileId,
    InvalidSaveLength(usize),
    UnknownSlot(String),
    UnownedDirectory,
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
            Self::InvalidProfileId => {
                write!(formatter, "profile id must be a safe slot-style name")
            }
            Self::InvalidSaveLength(len) => write!(
                formatter,
                "save image length {len} is not 1..={MAX_SAVE_LEN} bytes"
            ),
            Self::UnknownSlot(slot) => write!(formatter, "unknown save slot {slot:?}"),
            Self::UnownedDirectory => {
                write!(formatter, "save directory is not owned by this application")
            }
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
    profile: Option<String>,
}

impl SaveLibrary {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            profile: None,
        }
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
        Ok(self.data_dir(digest)?.join(format!("{slot}.sav")))
    }

    fn default_path(&self, digest: &str) -> SaveResult<PathBuf> {
        Ok(self.data_dir(digest)?.join("default"))
    }

    fn mutation_lock(&self, digest: &str) -> SaveResult<fs::File> {
        self.rom_dir(digest)?;
        if !fs::metadata(&self.root).is_ok_and(|metadata| metadata.is_dir()) {
            return Err(SaveError::UnownedDirectory);
        }
        let file = self.open_mutation_lock_file(digest)?;
        file.lock()?;
        Ok(file)
    }

    fn open_mutation_lock_file(&self, digest: &str) -> SaveResult<fs::File> {
        let path = self.root.join(format!(
            ".gba-native-mutation-{}.lock",
            digest.to_ascii_lowercase()
        ));
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => Ok(file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&path)?;
                if !metadata.file_type().is_file() {
                    return Err(SaveError::UnownedDirectory);
                }
                Ok(fs::OpenOptions::new().read(true).write(true).open(path)?)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn claim_for_store(&self, digest: &str) -> SaveResult<PathBuf> {
        let dir = self.rom_dir(digest)?;
        match fs::create_dir(&dir) {
            Ok(()) => {
                let dir = self.real_rom_dir(digest)?;
                if let Err(error) = write_owner_marker(&dir.join(OWNER_FILE), digest) {
                    // Remove only the newly created directory, and only when
                    // still empty; any concurrent unrelated entry is retained.
                    let _ = fs::remove_dir(&dir);
                    return Err(error);
                }
                Ok(dir)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                self.require_owned(digest)?;
                Ok(dir)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn require_owned(&self, digest: &str) -> SaveResult<()> {
        let dir = self.real_rom_dir(digest)?;
        let marker = dir.join(OWNER_FILE);
        let metadata = match fs::symlink_metadata(&marker) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(SaveError::UnownedDirectory);
            }
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_file() {
            return Err(SaveError::UnownedDirectory);
        }
        let expected = owner_marker(digest);
        let mut content = Vec::with_capacity(expected.len() + 1);
        fs::File::open(&marker)?
            .take((expected.len() + 1) as u64)
            .read_to_end(&mut content)?;
        if content != expected.as_bytes() {
            return Err(SaveError::UnownedDirectory);
        }
        Ok(())
    }

    fn real_rom_dir(&self, digest: &str) -> SaveResult<PathBuf> {
        let dir = self.rom_dir(digest)?;
        let metadata = match fs::symlink_metadata(&dir) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(SaveError::UnownedDirectory);
            }
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_dir() {
            return Err(SaveError::UnownedDirectory);
        }
        Ok(dir)
    }

    /// Existing slots, sorted by name.
    pub fn list(&self, digest: &str) -> SaveResult<Vec<String>> {
        let Some(dir) = profile::read_dir(&self.rom_dir(digest)?, self.profile.as_deref())? else {
            return Ok(Vec::new());
        };
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
        let path = self.slot_path(digest, slot)?;
        self.check_read_profile(digest)?;
        Ok(path.is_file())
    }

    pub fn load(&self, digest: &str, slot: &str) -> SaveResult<Vec<u8>> {
        let path = self.slot_path(digest, slot)?;
        self.check_read_profile(digest)?;
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(SaveError::UnknownSlot(slot.to_owned()));
            }
            Err(error) => return Err(error.into()),
        };
        // Read at most one byte beyond the accepted limit. A manually placed
        // oversized file must not allocate unbounded memory before rejection.
        let mut bytes = Vec::with_capacity(MAX_SAVE_LEN + 1);
        file.take((MAX_SAVE_LEN + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.is_empty() || bytes.len() > MAX_SAVE_LEN {
            return Err(SaveError::InvalidSaveLength(bytes.len()));
        }
        Ok(bytes)
    }

    /// Store a slot and make it default when no default exists yet.
    pub fn store(&self, digest: &str, slot: &str, bytes: &[u8]) -> SaveResult<()> {
        if bytes.is_empty() || bytes.len() > MAX_SAVE_LEN {
            return Err(SaveError::InvalidSaveLength(bytes.len()));
        }
        let path = self.slot_path(digest, slot)?;
        fs::create_dir_all(&self.root)?;
        let _mutation_lock = self.mutation_lock(digest)?;
        self.claim_for_store(digest)?;
        self.ensure_profile(digest)?;
        write_atomic(&path, bytes)?;
        if self.default_slot(digest)?.is_none() {
            self.set_default_unlocked(digest, slot)?;
        }
        Ok(())
    }

    pub fn delete(&self, digest: &str, slot: &str) -> SaveResult<()> {
        let path = self.slot_path(digest, slot)?;
        let _mutation_lock = self.mutation_lock(digest)?;
        self.require_owned(digest)?;
        self.require_profile(digest)?;
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
                self.set_default_unlocked(digest, next)?;
            }
        }
        Ok(())
    }

    pub fn set_default(&self, digest: &str, slot: &str) -> SaveResult<()> {
        self.slot_path(digest, slot)?;
        let _mutation_lock = self.mutation_lock(digest)?;
        self.set_default_unlocked(digest, slot)
    }

    fn set_default_unlocked(&self, digest: &str, slot: &str) -> SaveResult<()> {
        self.require_owned(digest)?;
        self.require_profile(digest)?;
        if !self.exists(digest, slot)? {
            return Err(SaveError::UnknownSlot(slot.to_owned()));
        }
        let path = self.default_path(digest)?;
        write_atomic(&path, slot.as_bytes())
    }

    pub fn default_slot(&self, digest: &str) -> SaveResult<Option<String>> {
        let path = self.default_path(digest)?;
        if !self.check_read_profile(digest)? {
            return Ok(None);
        }
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

fn owner_marker(digest: &str) -> String {
    format!(
        "gba-native-save-owner-v1\nrom-sha256={}\nsave-format=raw-backup-v1\n",
        digest.to_ascii_lowercase()
    )
}

fn write_owner_marker(path: &Path, digest: &str) -> SaveResult<()> {
    let content = owner_marker(digest);
    write_owner_marker_with(path, content.as_bytes(), |temporary, marker| {
        fs::hard_link(temporary, marker)
    })
}

fn write_owner_marker_with(
    path: &Path,
    content: &[u8],
    install: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> SaveResult<()> {
    let (temporary, mut file) = create_temporary_file(path)?;
    file.write_all(content)?;
    file.sync_all()?;
    drop(file);
    install(&temporary.path, path)?;
    // Drop removes only the private staged file. The marker itself appears in
    // one create-if-absent hard-link operation, never partially written.
    drop(temporary);
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> SaveResult<()> {
    write_atomic_with(path, bytes, |from, to| fs::rename(from, to))
}

fn write_atomic_with(
    path: &Path,
    bytes: &[u8],
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> SaveResult<()> {
    let (mut temporary, mut file) = create_temporary_file(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);

    // Same-directory rename replaces files atomically on Unix and supported
    // Windows/filesystems; elsewhere it fails closed, preserving the old save.
    // Never remove the destination as a fallback: that would expose a gap.
    rename(&temporary.path, path)?;
    temporary.disarm();
    Ok(())
}

fn create_temporary_file(path: &Path) -> io::Result<(TemporaryFile, fs::File)> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    create_temporary_file_with(path, || NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

fn create_temporary_file_with(
    path: &Path,
    mut next_id: impl FnMut() -> u64,
) -> io::Result<(TemporaryFile, fs::File)> {
    for _ in 0..MAX_TEMP_ATTEMPTS {
        let temporary_path = temporary_path(path, next_id())?;
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => {
                return Ok((
                    TemporaryFile {
                        path: temporary_path,
                        remove_on_drop: true,
                    },
                    file,
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique temporary save file",
    ))
}

fn temporary_path(path: &Path, id: u64) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "save path has no file name"))?;
    let mut temporary_name = std::ffi::OsString::from(".");
    temporary_name.push(name);
    temporary_name.push(format!(".{}.{}.tmp", std::process::id(), id));
    Ok(path.with_file_name(temporary_name))
}

struct TemporaryFile {
    path: PathBuf,
    remove_on_drop: bool,
}

impl TemporaryFile {
    fn disarm(&mut self) {
        self.remove_on_drop = false;
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.remove_on_drop {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests;
