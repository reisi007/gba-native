use super::{SaveError, SaveLibrary, SaveResult, write_owner_marker_with};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MARKER: &str = ".gba-native-profile-owner";

fn owner_profile_marker(digest: &str, id: &str) -> String {
    format!(
        "gba-native-save-profile-owner-v1\nrom-sha256={}\nprofile-id={}\nsave-format=raw-backup-v1\n",
        digest.to_ascii_lowercase(),
        id
    )
}

impl SaveLibrary {
    /// Enumerate only profile directories whose ownership marker exactly matches this ROM.
    pub fn profiles(&self, digest: &str) -> SaveResult<Vec<String>> {
        let rom = self.rom_dir(digest)?;
        let profiles = rom.join("profiles");
        match fs::symlink_metadata(&rom) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
            Ok(m) if !m.file_type().is_dir() => return Err(SaveError::UnownedDirectory),
            Ok(_) => self.require_owned(digest)?,
        }
        match fs::symlink_metadata(&profiles) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
            Ok(m) if !m.file_type().is_dir() => return Err(SaveError::UnownedDirectory),
            _ => {}
        }
        let mut found = Vec::new();
        for entry in fs::read_dir(profiles)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if !super::valid_slot_name(&id) || id == "default" {
                continue;
            }
            if entry.file_type()?.is_dir() {
                match require(&rom, digest, &id) {
                    Ok(()) => found.push(id),
                    Err(SaveError::UnownedDirectory) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        found.sort();
        Ok(found)
    }

    /// Safely create an empty, owned profile namespace. Existing owned profiles are retained.
    pub fn initialize_profile(&self, digest: &str) -> SaveResult<()> {
        let id = self.profile.as_deref().ok_or(SaveError::InvalidProfileId)?;
        fs::create_dir_all(&self.root)?;
        let _mutation_lock = self.mutation_lock(digest)?;
        self.claim_for_store(digest)?;
        ensure(&self.rom_dir(digest)?, digest, id)
    }

    /// Select a profile namespace; `default` preserves the legacy save paths.
    pub fn for_profile(root: impl Into<PathBuf>, profile_id: &str) -> SaveResult<Self> {
        if !super::valid_slot_name(profile_id) {
            return Err(SaveError::InvalidProfileId);
        }
        if profile_id == "default" {
            return Ok(Self::new(root));
        }
        Ok(Self {
            root: root.into(),
            profile: Some(profile_id.to_owned()),
        })
    }

    pub(super) fn data_dir(&self, digest: &str) -> SaveResult<PathBuf> {
        let rom = self.rom_dir(digest)?;
        Ok(match &self.profile {
            Some(id) => rom.join("profiles").join(id),
            None => rom,
        })
    }

    pub(super) fn require_profile(&self, digest: &str) -> SaveResult<()> {
        if let Some(id) = &self.profile {
            require(&self.rom_dir(digest)?, digest, id)?;
        }
        Ok(())
    }

    pub(super) fn ensure_profile(&self, digest: &str) -> SaveResult<()> {
        if let Some(id) = &self.profile {
            ensure(&self.rom_dir(digest)?, digest, id)?;
        }
        Ok(())
    }

    pub(super) fn check_read_profile(&self, digest: &str) -> SaveResult<bool> {
        if let Some(id) = &self.profile {
            read_dir(&self.rom_dir(digest)?, Some(id)).map(|p| p.is_some())
        } else {
            Ok(true)
        }
    }
}

pub(super) fn read_dir(rom: &Path, id: Option<&str>) -> SaveResult<Option<PathBuf>> {
    let Some(id) = id else {
        return Ok(Some(rom.to_path_buf()));
    };
    match fs::symlink_metadata(rom) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(SaveError::UnownedDirectory);
        }
        Ok(_) => {}
    }
    let profiles = rom.join("profiles");
    match fs::symlink_metadata(&profiles) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(m) if !m.file_type().is_dir() => return Err(SaveError::UnownedDirectory),
        _ => {}
    }
    let dir = profiles.join(id);
    match fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
        Ok(m) if !m.file_type().is_dir() => Err(SaveError::UnownedDirectory),
        Ok(_) => Ok(Some(dir)),
    }
}

pub(super) fn ensure(rom: &Path, digest: &str, id: &str) -> SaveResult<()> {
    checked_dir(rom)?;
    let profiles = rom.join("profiles");
    create_or_check(&profiles)?;
    let dir = profiles.join(id);
    match fs::create_dir(&dir) {
        Ok(()) => {
            if let Err(error) = write_owner_marker_with(
                &dir.join(MARKER),
                owner_profile_marker(digest, id).as_bytes(),
                |tmp, dst| fs::hard_link(tmp, dst),
            ) {
                let _ = fs::remove_dir(&dir);
                return Err(error);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => require(rom, digest, id)?,
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

pub(super) fn require(rom: &Path, digest: &str, id: &str) -> SaveResult<()> {
    let profiles = rom.join("profiles");
    checked_dir(&profiles)?;
    let dir = profiles.join(id);
    checked_dir(&dir)?;
    let marker = dir.join(MARKER);
    let meta = fs::symlink_metadata(&marker).map_err(|_| SaveError::UnownedDirectory)?;
    if !meta.file_type().is_file() {
        return Err(SaveError::UnownedDirectory);
    }
    let expected = owner_profile_marker(digest, id);
    let mut bytes = Vec::new();
    fs::File::open(marker)?
        .take((expected.len() + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes != expected.as_bytes() {
        return Err(SaveError::UnownedDirectory);
    }
    Ok(())
}

fn checked_dir(path: &Path) -> SaveResult<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => Ok(()),
        _ => Err(SaveError::UnownedDirectory),
    }
}

fn create_or_check(path: &PathBuf) -> SaveResult<()> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => checked_dir(path),
        Err(e) => Err(e.into()),
    }
}
