//! Build-time identity for the single external ROM used by a game app.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomBinding {
    pub title: &'static str,
    pub game_code: &'static str,
    pub sha256: &'static str,
    pub len: usize,
    pub file_name: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/rom_binding.rs"));

/// The bound image compiled into a local single-file build.
#[cfg(rom_embedded)]
pub fn bound_rom_bytes() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/bound_rom.bin"))
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn bound_rom_url() -> Option<String> {
    BOUND_ROM.map(|rom| format!("./{}", rom.file_name))
}
