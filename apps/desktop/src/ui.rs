//! Desktop view of the shared synthetic shell, settings and saves pages.
//!
//! The shell, fullscreen frame, settings page, saves page and touch hit testing
//! are pure pixel work in `gba_native::frontend::ui`, so Android composes the
//! same screens. This module re-exports the pieces the window code needs.

pub use gba_native::frontend::ui::{HEIGHT, WIDTH, hit_test};
