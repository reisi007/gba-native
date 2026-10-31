//! Window-free frontend model shared by every GUI target.
//!
//! The desktop, browser and Android frontends all consume the same image
//! pipeline, viewport math, settings persistence and tone artifact. A platform
//! layer only owns its surface, event loop and per-user path selection, so the
//! user-facing features cannot drift between targets.

pub mod features;
pub mod image;
pub mod player;
pub mod saves;
pub mod settings;
pub mod tone;
pub mod ui;
pub mod viewport;

pub use features::{FEATURE_COUNT, FEATURE_IDS, Feature, ids as feature_ids, ids_of};
pub use image::{
    ImageError, LoadError, MAX_IMAGE_PIXELS, RgbaImage, convert_rgb888_to_rgba, demo_image,
};
pub use player::{ENV_OVERRIDE, Player, detect as detect_player, play as play_wav};
pub use saves::{IMPORT_FILE, MAX_SAVE_BYTES, SYNTHETIC_ROM_SHA, SaveSlot, SaveSlots};
pub use settings::{load, save};
pub use tone::{ToneArtifact, tone_wav, write_tone};
pub use viewport::{Viewport, compute_viewport};
