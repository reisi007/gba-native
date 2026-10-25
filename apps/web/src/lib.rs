//! Isolated browser foundation for the synthetic gba-native demo.
//!
//! The crate depends directly on `gba-native-core` (aliased to `gba_native`)
//! and never on the host AOT tooling or the root compatibility facade. Host
//! targets compile the pure model, presentation and static-server modules; the
//! WASM target adds the Winit/Pixels browser lifecycle.

#![forbid(unsafe_code)]

pub mod model;
pub mod presentation;

// The static server uses std::fs and is host-only; it is not compiled into the
// browser WASM artefact.
#[cfg(not(target_arch = "wasm32"))]
pub mod serve;

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
pub use web::{poll_gpu_status, set_status_sink, start};
