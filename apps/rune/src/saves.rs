//! Shared core save-slot operations with browser localStorage/download adapters.

#[cfg(not(target_arch = "wasm32"))]
use gba_native::frontend::saves::SYNTHETIC_ROM_SHA;
use gba_native::frontend::saves::{MAX_SAVE_BYTES, SaveSlot, SaveSlots};
#[cfg(not(target_arch = "wasm32"))]
use gba_native::save::SaveLibrary;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read;

#[cfg(all(test, not(target_arch = "wasm32")))]
use std::path::PathBuf;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r##"
const savePrefix = "gba-native.rune.slot.";
const saveDefaultKey = "gba-native.rune.default-slot";
const savePendingKey = "gba-native.rune.save.pending";
const maxSaveHexLength = 2 * 128 * 1024;
function safeStorage() { try { return globalThis.localStorage; } catch (_) { return null; } }
function validSynthetic(bytes) {
  if (bytes.length !== 512 || String.fromCharCode(...bytes.slice(0, 8)) !== "GBANSV01") return false;
  const seed = bytes[8];
  for (let index = 9; index < bytes.length; index += 1) {
    if (bytes[index] !== ((index * 31 + seed) & 0xff)) return false;
  }
  return true;
}
function validRawHex(encoded) {
  return typeof encoded === "string" && encoded.length >= 2 && encoded.length <= maxSaveHexLength && encoded.length % 2 === 0 && /^[\da-f]+$/i.test(encoded);
}
function storedNames(storage) { return Object.keys(storage).filter((key) => key.startsWith(savePrefix)).map((key) => key.slice(savePrefix.length)).sort(); }
function syntheticImage(seed) {
  const bytes = new Uint8Array(512);
  bytes.set([0x47, 0x42, 0x41, 0x4e, 0x53, 0x56, 0x30, 0x31]);
  bytes[8] = seed & 0xff;
  for (let index = 9; index < bytes.length; index += 1) {
    bytes[index] = ((index * 31 + seed) & 0xff);
  }
  return bytes;
}
export function runeSaveNames() {
  try {
    const storage = safeStorage();
    if (!storage) return "";
    return storedNames(storage).filter((name) => validRawHex(storage.getItem(savePrefix + name))).join("\n");
  } catch (_) { return ""; }
}
export function runeSaveSize(name) {
  try { const encoded = safeStorage()?.getItem(savePrefix + name); return validRawHex(encoded) ? encoded.length / 2 : 0; }
  catch (_) { return 0; }
}
export function runeSaveIsDefault(name) {
  try { return safeStorage()?.getItem(saveDefaultKey) === name; }
  catch (_) { return false; }
}
export function runeSaveStore(name, bytes) {
  try {
    const storage = safeStorage();
    if (!storage || !validSynthetic(bytes)) return false;
    let hex = "";
    for (const byte of bytes) hex += byte.toString(16).padStart(2, "0");
    storage.setItem(savePrefix + name, hex);
    if (!storage.getItem(saveDefaultKey)) storage.setItem(saveDefaultKey, name);
    return true;
  } catch (_) { return false; }
}
export function runeSaveCreate() {
  try {
    const storage = safeStorage();
    if (!storage) return false;
    const count = storedNames(storage).length;
    let number = count + 1;
    let name = `slot${number}`;
    if (name.length > 32) return false;
    while (storage.getItem(savePrefix + name) !== null) {
      number += 1;
      name = `slot${number}`;
      if (name.length > 32) return false;
    }
    return runeSaveStore(name, syntheticImage(count));
  } catch (_) { return false; }
}
export function runeSaveDelete(name) {
  try {
    const storage = safeStorage();
    if (!storage || !validRawHex(storage.getItem(savePrefix + name))) return false;
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
    if (!storage || !validRawHex(storage.getItem(savePrefix + name))) return false;
    storage.setItem(saveDefaultKey, name);
    return true;
  } catch (_) { return false; }
}
export function runeSaveRead(name) {
  try { const encoded = safeStorage()?.getItem(savePrefix + name); return validRawHex(encoded) ? encoded : null; }
  catch (_) { return null; }
}
export function runeSaveReadPending() {
  try { return safeStorage()?.getItem(savePendingKey) ?? null; }
  catch (_) { return null; }
}
export function runeSaveClearPending() {
  try { safeStorage()?.removeItem(savePendingKey); } catch (_) {}
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
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeSaveCreate)]
    fn browser_create() -> bool;
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
            let mut slots = Vec::new();
            for name in browser_names().lines().filter(|name| valid_name(name)) {
                let Some(encoded) = browser_read(name) else {
                    continue;
                };
                let Ok(bytes) = decode_hex(&encoded) else {
                    continue;
                };
                if browser_size(name) == bytes.len() && validate_raw_image(&bytes).is_ok() {
                    slots.push(SaveSlot {
                        name: name.to_owned(),
                        bytes: bytes.len(),
                        default: browser_is_default(name),
                    });
                }
            }
            return Ok(slots);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
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
    }

    pub(crate) fn create(&self) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            browser_create()
                .then_some(())
                .ok_or_else(|| "cannot create synthetic save slot".to_owned())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            // Keep the desktop count-based name and seed when that name is
            // available, but never replace a hidden or corrupt existing file.
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
                number = number
                    .checked_add(1)
                    .ok_or_else(|| "no unused synthetic save slot name is available".to_owned())?;
            };
            let bytes = SaveSlots::synthetic_image(seed as u8);
            self.store(&name, &bytes)
        }
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
            validate_raw_image(&bytes)?;
            if !browser_download(name, &bytes) {
                return Err("browser download is unavailable".to_owned());
            }
            Ok(format!("downloaded {name}.sav"))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
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

    #[cfg(any(test, target_arch = "wasm32", target_os = "ios"))]
    pub(crate) fn import_selected(&self) -> Result<String, String> {
        #[cfg(target_arch = "wasm32")]
        {
            let Some(encoded) = browser_read_pending() else {
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
            {
                self.import_path(&path)
            }
        }
    }

    fn store(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        if !valid_name(name) {
            return Err(format!("invalid save slot name: {name}"));
        }
        validate_synthetic_image(bytes)?;
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

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn validate_raw_image(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_SAVE_BYTES {
        return Err(format!(
            "save image length must be 1..={MAX_SAVE_BYTES} bytes"
        ));
    }
    Ok(())
}

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
#[path = "saves/tests.rs"]
mod tests;
