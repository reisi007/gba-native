//! Desktop view of the shared frontend image pipeline.
//!
//! The validated RGBA conversion, synthetic demo snapshot and integer
//! letterbox math live in `gba_native::frontend` so every GUI target runs the
//! same code. This module only keeps the desktop-facing names stable; it owns
//! no pixel rules of its own.

pub use gba_native::frontend::image::{RgbaImage, demo_image as synthetic_demo_image};
pub use gba_native::frontend::viewport::{Viewport, compute_viewport};
