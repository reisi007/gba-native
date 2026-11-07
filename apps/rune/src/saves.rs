//! Shared core save-slot operations with browser localStorage/download adapters.

use gba_native::frontend::saves::{MAX_SAVE_BYTES, SaveSlot, SaveSlots};

#[cfg(all(test, not(target_arch = "wasm32")))]
use std::path::PathBuf;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r##"
const savePrefix = "gba-native.rune.slot.";
const saveDefaultKey = "gba-native.rune.default-slot";
const savePendingKey = "gba-native.rune.save.pending";
function safeStorage() { try { return globalThis.localStorage; } catch (_) { return null; } }
export function runeSaveNames() {
  try {
    const storage = safeStorage();
    if (!storage) return "";
    return Object.keys(storage).filter((key) => key.startsWith(savePrefix))
      .map((key) => key.slice(savePrefix.length)).sort().join("\n");
  } catch (_) { return ""; }
}
export function runeSaveSize(name) {
  try { return (safeStorage()?.getItem(savePrefix + name) ?? "").length / 2; }
  catch (_) { return 0; }
}
export function runeSaveIsDefault(name) {
  try { return safeStorage()?.getItem(saveDefaultKey) === name; }
  catch (_) { return false; }
}
export function runeSaveStore(name, bytes) {
  try {
    const storage = safeStorage();
    if (!storage) return false;
    let hex = "";
    for (const byte of bytes) hex += byte.toString(16).padStart(2, "0");
    storage.setItem(savePrefix + name, hex);
    if (!storage.getItem(saveDefaultKey)) storage.setItem(saveDefaultKey, name);
    return true;
  } catch (_) { return false; }
}
export function runeSaveDelete(name) {
  try {
    const storage = safeStorage();
    if (!storage?.getItem(savePrefix + name)) return false;
    storage.removeItem(savePrefix + name);
    if (storage.getItem(saveDefaultKey) === name) {
      const next = runeSaveNames().split("\n").filter(Boolean)[0];
      if (next) storage.setItem(saveDefaultKey, next); else storage.removeItem(saveDefaultKey);
    }
    return true;
  } catch (_) { return false; }
}
export function runeSaveSetDefault(name) {
  try {
    const storage = safeStorage();
    if (!storage?.getItem(savePrefix + name)) return false;
    storage.setItem(saveDefaultKey, name);
    return true;
  } catch (_) { return false; }
}
export function runeSaveRead(name) {
  try { return safeStorage()?.getItem(savePrefix + name) ?? null; }
  catch (_) { return null; }
}
export function runeSaveReadPending() {
  try { return safeStorage()?.getItem(savePendingKey) ?? null; }
  catch (_) { return null; }
}
export function runeSaveClearPending() {
  try { safeStorage()?.removeItem(savePendingKey); } catch (_) {}
}
export function runeSaveRequestImport() {
  try { safeStorage()?.removeItem(savePendingKey); } catch (_) {}
  document.querySelector("#rune-save-import")?.click();
}
export function runeSaveDownload(name, bytes) {
  try {
    const blob = new Blob([bytes], { type: "application/octet-stream" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url; link.download = `${name}.sav`; link.click();
    // Let the browser resolve the download before releasing its blob URL.
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    return true;
  } catch (_) { return false; }
}
"##)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveNames)]
    fn browser_names() -> String;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveSize)]
    fn browser_size(name: &str) -> usize;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveIsDefault)]
    fn browser_is_default(name: &str) -> bool;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveStore)]
    fn browser_store(name: &str, bytes: &[u8]) -> bool;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveDelete)]
    fn browser_delete(name: &str) -> bool;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveSetDefault)]
    fn browser_set_default(name: &str) -> bool;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveRead)]
    fn browser_read(name: &str) -> Option<String>;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveReadPending)]
    fn browser_read_pending() -> Option<String>;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveClearPending)]
    fn browser_clear_pending();
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveRequestImport)]
    fn browser_request_import();
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveDownload)]
    fn browser_download(name: &str, bytes: &[u8]) -> bool;
}

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

    pub(crate) fn slots(&self) -> Result<Vec<SaveSlot>, String> {
        #[cfg(target_arch = "wasm32")]
        {
            let names = browser_names();
            return Ok(names
                .lines()
                .filter(|name| !name.is_empty())
                .map(|name| SaveSlot {
                    name: name.to_owned(),
                    bytes: browser_size(name),
                    default: browser_is_default(name),
                })
                .collect());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.library
                .as_ref()
                .ok_or_else(|| "no save directory is available".to_owned())?
                .slots()
                .map_err(|error| error.to_string())
        }
    }

    pub(crate) fn create(&self) -> Result<(), String> {
        let slots = self.slots()?;
        let mut number = 1usize;
        while slots
            .iter()
            .any(|slot| slot.name == format!("slot{number}"))
        {
            number += 1;
        }
        // Same naming and seed convention as the existing desktop and Android
        // frontends: `slot<n>` with the zero-based creation index as fixture seed.
        let name = format!("slot{number}");
        let bytes = SaveSlots::synthetic_image((number - 1) as u8);
        self.store(&name, &bytes)
    }

    pub(crate) fn delete(&self, name: &str) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        let result = browser_delete(name);
        #[cfg(not(target_arch = "wasm32"))]
        let result = self
            .library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?
            .delete(name)
            .map_err(|error| error.to_string())
            .map(|()| true)?;
        result
            .then_some(())
            .ok_or_else(|| format!("cannot delete save slot {name}"))
    }

    pub(crate) fn set_default(&self, name: &str) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        let result = browser_set_default(name);
        #[cfg(not(target_arch = "wasm32"))]
        let result = self
            .library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?
            .set_default(name)
            .map_err(|error| error.to_string())
            .map(|()| true)?;
        result
            .then_some(())
            .ok_or_else(|| format!("cannot select save slot {name}"))
    }

    #[cfg(any(test, target_arch = "wasm32", target_os = "ios"))]
    pub(crate) fn export(&self, name: &str) -> Result<String, String> {
        #[cfg(target_arch = "wasm32")]
        {
            let encoded =
                browser_read(name).ok_or_else(|| format!("save slot {name} is unavailable"))?;
            let bytes = decode_hex(&encoded)?;
            if !browser_download(name, &bytes) {
                return Err("browser download is unavailable".to_owned());
            }
            Ok(format!("downloaded {name}.sav"))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.library
                .as_ref()
                .ok_or_else(|| "no save directory is available".to_owned())?
                .export_to_file(name)
                .map(|path| format!("exported {name} to {}", path.display()))
                .map_err(|error| error.to_string())
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn import_path(&self, path: &std::path::Path) -> Result<String, String> {
        let library = self
            .library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?;
        let bytes = std::fs::read(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
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
        let library = self
            .library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?;
        let bytes = library.export(name).map_err(|error| error.to_string())?;
        std::fs::write(path, bytes)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        Ok(format!("exported {name} to {}", path.display()))
    }

    #[cfg(any(test, target_arch = "wasm32", target_os = "ios"))]
    pub(crate) fn import_selected(&self) -> Result<String, String> {
        #[cfg(target_arch = "wasm32")]
        {
            let Some(encoded) = browser_read_pending() else {
                browser_request_import();
                return Err("choose a .sav file, then press Import again".to_owned());
            };
            browser_clear_pending();
            let bytes = decode_hex(&encoded)?;
            validate_synthetic_image(&bytes)?;
            self.store("imported", &bytes)?;
            Ok("imported synthetic save fixture".to_owned())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let library = self
                .library
                .as_ref()
                .ok_or_else(|| "no save directory is available".to_owned())?;
            let path = library
                .root()
                .join(gba_native::frontend::saves::IMPORT_FILE);
            self.import_path(&path)
        }
    }

    fn store(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        if !valid_name(name) {
            return Err(format!("invalid save slot name: {name}"));
        }
        #[cfg(target_arch = "wasm32")]
        let result = browser_store(name, bytes);
        #[cfg(not(target_arch = "wasm32"))]
        let result = self
            .library
            .as_ref()
            .ok_or_else(|| "no save directory is available".to_owned())?
            .store(name, bytes)
            .map_err(|error| error.to_string())
            .map(|()| true)?;
        result
            .then_some(())
            .ok_or_else(|| format!("cannot store save slot {name}"))
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn validate_synthetic_image(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_SAVE_BYTES {
        return Err(format!("save image exceeds {MAX_SAVE_BYTES} bytes"));
    }
    if bytes.len() != 512 || bytes.get(..8) != Some(b"GBANSV01") {
        return Err("only 512-byte GBANSV01 synthetic save fixtures are accepted".to_owned());
    }
    if bytes != SaveSlots::synthetic_image(bytes[8]) {
        return Err("synthetic save fixture contents do not match the embedded seed".to_owned());
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn decode_hex(encoded: &str) -> Result<Vec<u8>, String> {
    if encoded.len() % 2 != 0 || encoded.len() / 2 > MAX_SAVE_BYTES {
        return Err("invalid or oversized save image".to_owned());
    }
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).map_err(|error| error.to_string())?;
            u8::from_str_radix(pair, 16).map_err(|error| error.to_string())
        })
        .collect()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gba-native-rune-saves-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn synthetic_slots_create_select_export_and_delete_through_core_library() {
        let root = temp_root("crud");
        let store = SaveStore::at(root.clone());
        assert!(store.slots().unwrap().is_empty());
        store.create().unwrap();
        store.create().unwrap();
        let slots = store.slots().unwrap();
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].name, "slot1");
        assert!(slots[0].default);
        store.set_default("slot2").unwrap();
        let path = root.join("chosen-export.sav");
        assert!(
            store
                .export_path("slot2", &path)
                .unwrap()
                .contains("chosen-export.sav")
        );
        assert_eq!(std::fs::read(&path).unwrap(), SaveSlots::synthetic_image(1));
        store.delete("slot1").unwrap();
        assert_eq!(store.slots().unwrap()[0].name, "slot2");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_only_accepts_exact_generated_synthetic_save_images() {
        let root = temp_root("import");
        let store = SaveStore::at(root.clone());
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(gba_native::frontend::saves::IMPORT_FILE),
            [1, 2, 3],
        )
        .unwrap();
        assert!(store.import_selected().is_err());
        assert!(store.slots().unwrap().is_empty());

        let synthetic = SaveSlots::synthetic_image(8);
        std::fs::write(
            root.join(gba_native::frontend::saves::IMPORT_FILE),
            &synthetic,
        )
        .unwrap();
        assert!(
            store
                .import_selected()
                .unwrap()
                .contains("synthetic fixture")
        );
        assert_eq!(store.slots().unwrap()[0].name, "imported");
        std::fs::remove_dir_all(root).unwrap();
    }
}
