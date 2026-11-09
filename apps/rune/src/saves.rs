//! Shared core save-slot operations with native and IndexedDB browser adapters.

#[cfg(not(target_arch = "wasm32"))]
use gba_native::frontend::saves::{MAX_SAVE_BYTES, SYNTHETIC_ROM_SHA, SaveSlot, SaveSlots};
#[cfg(not(target_arch = "wasm32"))]
use gba_native::save::SaveLibrary;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read;

#[cfg(all(test, not(target_arch = "wasm32")))]
use std::path::PathBuf;

#[cfg(target_arch = "wasm32")]
mod browser;

#[derive(Debug)]
pub(crate) struct SaveStore {
    #[cfg(not(target_arch = "wasm32"))]
    library: Option<SaveSlots>,
}

impl Default for SaveStore {
    fn default() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            library: crate::storage::saves_root().map(SaveSlots::new),
        }
    }
}

impl SaveStore {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn at(root: PathBuf) -> Self {
        Self {
            library: Some(SaveSlots::new(root)),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn start_browser_operation(
        &self,
        operation: &str,
        profile: &str,
        name: &str,
        value: &str,
    ) -> u32 {
        browser::start(operation, profile, name, value)
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn take_browser_result(&self) -> Option<String> {
        browser::take_result()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn slots(&self) -> Result<Vec<SaveSlot>, String> {
        let root = self
            .library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?
            .root()
            .to_path_buf();
        let library = SaveLibrary::new(root);
        let names = library
            .list(SYNTHETIC_ROM_SHA)
            .map_err(|error| error.to_string())?;
        let default = library
            .default_slot(SYNTHETIC_ROM_SHA)
            .map_err(|error| error.to_string())?;
        let mut slots = Vec::new();
        for name in names {
            let Ok(bytes) = library.load(SYNTHETIC_ROM_SHA, &name) else {
                continue;
            };
            if validate_raw_image(&bytes).is_ok() {
                slots.push(SaveSlot {
                    bytes: bytes.len(),
                    default: default.as_deref() == Some(name.as_str()),
                    name,
                });
            }
        }
        Ok(slots)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn create(&self) -> Result<(), String> {
        // Keep the desktop count-based name and seed when available, but never
        // replace a hidden or corrupt existing file.
        let root = self
            .library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?
            .root()
            .to_path_buf();
        let library = SaveLibrary::new(root);
        let existing = library
            .list(SYNTHETIC_ROM_SHA)
            .map_err(|error| error.to_string())?;
        let seed = existing.len();
        let mut number = seed + 1;
        let name = loop {
            let candidate = format!("slot{number}");
            if !valid_name(&candidate) {
                return Err("no unused valid synthetic save slot name is available".to_owned());
            }
            if !existing.iter().any(|slot| slot == &candidate) {
                break candidate;
            }
            number = number.checked_add(1).ok_or_else(|| {
                "no unused valid synthetic save slot name is available".to_owned()
            })?;
        };
        self.store(&name, &SaveSlots::synthetic_image(seed as u8))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn delete(&self, name: &str) -> Result<(), String> {
        self.library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?
            .delete(name)
            .map_err(|error| error.to_string())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn set_default(&self, name: &str) -> Result<(), String> {
        self.library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?
            .set_default(name)
            .map_err(|error| error.to_string())
    }

    #[cfg(all(any(test, target_os = "ios"), not(target_arch = "wasm32")))]
    pub(crate) fn export(&self, name: &str) -> Result<String, String> {
        let bytes = self.native_bytes(name)?;
        let library = self
            .library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?;
        let path = library.root().join(format!("export-{name}.sav"));
        std::fs::write(&path, bytes)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        Ok(format!("exported {name} to {}", path.display()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn import_path(&self, path: &std::path::Path) -> Result<String, String> {
        let library = self
            .library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?;
        let bytes = read_bounded(path)?;
        validate_synthetic_image(&bytes)?;
        library
            .import("imported", &bytes)
            .map_err(|error| error.to_string())?;
        Ok(format!(
            "imported synthetic fixture from {}",
            path.display()
        ))
    }

    #[cfg(all(not(target_arch = "wasm32"), any(not(target_os = "ios"), test)))]
    pub(crate) fn export_path(&self, name: &str, path: &std::path::Path) -> Result<String, String> {
        let bytes = self.native_bytes(name)?;
        std::fs::write(path, bytes)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        Ok(format!("exported {name} to {}", path.display()))
    }

    #[cfg(all(any(test, target_os = "ios"), not(target_arch = "wasm32")))]
    pub(crate) fn import_selected(&self) -> Result<String, String> {
        let library = self
            .library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?;
        let path = library
            .root()
            .join(gba_native::frontend::saves::IMPORT_FILE);
        #[cfg(target_os = "ios")]
        {
            match rune_ios_platform::take_save_import_result() {
                Some(result) => result?,
                None => {
                    rune_ios_platform::present_save_import_picker(&path)?;
                    return Ok("choose a .sav file, then press Import again".to_owned());
                }
            }
            if !path.is_file() {
                return Err("the selected save was not staged".to_owned());
            }
            let result = self.import_path(&path);
            let _ = std::fs::remove_file(&path);
            result
        }
        #[cfg(not(target_os = "ios"))]
        self.import_path(&path)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn store(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        if !valid_name(name) {
            return Err(format!("invalid save slot name: {name}"));
        }
        validate_synthetic_image(bytes)?;
        self.library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?
            .store(name, bytes)
            .map_err(|error| error.to_string())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn native_bytes(&self, name: &str) -> Result<Vec<u8>, String> {
        if !valid_name(name) {
            return Err(format!("invalid save slot name: {name}"));
        }
        let bytes = self
            .library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?
            .load(name)
            .map_err(|error| error.to_string())?;
        validate_raw_image(&bytes)?;
        Ok(bytes)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_bounded(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let mut bytes = Vec::with_capacity(MAX_SAVE_BYTES + 1);
    file.take((MAX_SAVE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if bytes.len() > MAX_SAVE_BYTES {
        return Err(format!("save image exceeds {MAX_SAVE_BYTES} bytes"));
    }
    Ok(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_raw_image(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_SAVE_BYTES {
        return Err(format!(
            "save image length must be 1..={MAX_SAVE_BYTES} bytes"
        ));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_synthetic_image(bytes: &[u8]) -> Result<(), String> {
    validate_raw_image(bytes)?;
    if bytes.len() != 512 || bytes.get(..8) != Some(b"GBANSV01") {
        return Err("only 512-byte GBANSV01 synthetic save fixtures are accepted".to_owned());
    }
    if bytes != SaveSlots::synthetic_image(bytes[8]) {
        return Err("synthetic save fixture contents do not match the embedded seed".to_owned());
    }
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "saves/tests.rs"]
mod tests;
