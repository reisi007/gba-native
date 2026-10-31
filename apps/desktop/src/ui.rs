//! Desktop view of the shared synthetic shell and settings pages.
//!
//! The shell, fullscreen frame, settings page and touch hit testing are pure
//! pixel work in `gba_native::frontend::ui`, so Android composes the same
//! screens. This module only re-exports them under the desktop names.

pub use gba_native::frontend::ui::{HEIGHT, WIDTH, fullscreen, hit_test, settings_page, shell};
