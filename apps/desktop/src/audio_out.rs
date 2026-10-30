//! Dependency-free desktop tone artifact.
//!
//! This is the native audio output path in this slice: the same mixer used by
//! the core writes a WAV at the persisted volume/mute. Live device playback is
//! target-specific and intentionally kept out of this module.

use std::path::Path;

use gba_native::audio;
use gba_native::input::Settings;

pub fn write_tone(path: &Path, settings: &Settings) -> Result<(), String> {
    let samples = audio::render_tone(440.0, audio::SAMPLE_RATE as usize, settings.audio());
    let wav = audio::wav_bytes(&samples, audio::SAMPLE_RATE);
    std::fs::write(path, &wav)
        .map_err(|error| format!("cannot write audio {}: {error}", path.display()))?;
    println!(
        "audio_out={} samples={} volume={} muted={} audible={}",
        path.display(),
        samples.len(),
        settings.master_volume,
        settings.muted,
        !settings.muted && samples.iter().any(|sample| *sample != 0)
    );
    Ok(())
}
