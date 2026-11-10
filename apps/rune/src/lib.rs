//! Isolated Rune UI for the core's synthetic, static GBA snapshot.

#![forbid(unsafe_code)]

mod audio;
#[cfg(not(target_arch = "wasm32"))]
mod audio_sink;
#[cfg(target_arch = "wasm32")]
mod browser_rom;
pub mod features;
mod game;
pub mod model;
pub mod presentation;
mod rom_binding;
mod saves;
mod storage;
mod ui;

pub use game::{GameSession, SessionPolicy};
pub use ui::{AppState, app_spec};

pub fn run_app() {
    rune::App::new(app_spec()).tab_visits_controls(true).run();
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    run_app();
}
