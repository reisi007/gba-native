//! Window-free frontend model shared by every GUI target.
//!
//! The desktop, browser and Android frontends all consume the same image
//! pipeline, viewport math, settings persistence and tone artifact. A platform
//! layer only owns its surface, event loop and per-user path selection, so the
//! user-facing features cannot drift between targets.

pub mod image;
pub mod settings;
pub mod tone;
pub mod ui;
pub mod viewport;

pub use image::{
    ImageError, LoadError, MAX_IMAGE_PIXELS, RgbaImage, convert_rgb888_to_rgba, demo_image,
};
pub use settings::{load, save};
pub use tone::{ToneArtifact, tone_wav, write_tone};
pub use viewport::{Viewport, compute_viewport};
