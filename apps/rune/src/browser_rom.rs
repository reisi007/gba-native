//! Same-origin local file picker bridge. ROM bytes stay in the browser process.

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function runeTakePendingRom() {
  const bytes = window.__gbaNativeRunePendingRom;
  window.__gbaNativeRunePendingRom = null;
  return bytes ?? new Uint8Array();
}
export function runeTakeRomError() {
  const error = window.__gbaNativeRuneRomError;
  window.__gbaNativeRuneRomError = null;
  return error ?? null;
}
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeTakePendingRom)]
    fn take_pending_rom() -> Vec<u8>;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeTakeRomError)]
    fn take_rom_error() -> Option<String>;
}

#[cfg(target_arch = "wasm32")]
pub fn take() -> Vec<u8> {
    take_pending_rom()
}

#[cfg(target_arch = "wasm32")]
pub fn take_error() -> Option<String> {
    take_rom_error()
}
