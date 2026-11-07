//! Isolated Rune UI for the core's synthetic, static GBA snapshot.

#![forbid(unsafe_code)]

mod audio;
pub mod features;
pub mod model;
pub mod presentation;
mod saves;
mod storage;
mod ui;

pub use ui::{AppState, app_spec};

pub fn run_app() {
    rune::App::new(app_spec()).tab_visits_controls(true).run();
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    run_app();
}
