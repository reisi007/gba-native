//! Desktop audio artifact.
//!
//! The mixer and WAV encoding are std-only core code shared with every GUI
//! target through `frontend::tone`; this module only prints the desktop report.
//! Live device playback is target-specific and intentionally kept out of this
//! module.

use std::path::Path;

use gba_native::input::Settings;

pub fn write_tone(path: &Path, settings: &Settings) -> Result<(), String> {
    let artifact = gba_native::frontend::tone::write_tone(path, settings)
        .map_err(|error| format!("cannot write audio {}: {error}", path.display()))?;
    println!(
        "audio_out={} samples={} volume={} muted={} audible={}",
        path.display(),
        artifact.samples,
        artifact.volume,
        artifact.muted,
        artifact.audible
    );
    Ok(())
}
