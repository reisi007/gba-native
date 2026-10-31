//! Android synthetic shell over the shared, window-free frontend.
//!
//! The host-testable model owns composition, touch routing and settings; the
//! Android-only module owns the native window and event loop.

pub mod features;
pub mod model;

#[cfg(target_os = "android")]
mod app;
