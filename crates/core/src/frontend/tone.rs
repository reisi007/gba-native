//! Shared frontend audio artifact: the mixer's tone at the persisted volume.
//!
//! Every GUI target writes the same WAV from the same core mixer. This is an
//! artifact, not a live device sink; platform playback stays target-specific
//! and, where it exists, explicit.

use std::path::Path;

use crate::audio;
use crate::input::Settings;

/// What was written, so the platform layer only prints the shared report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToneArtifact {
    pub samples: usize,
    pub volume: u8,
    pub muted: bool,
    pub audible: bool,
}

/// Render the reference tone as a WAV at the persisted volume/mute.
pub fn tone_wav(settings: &Settings) -> Vec<u8> {
    let samples = audio::render_tone(440.0, audio::SAMPLE_RATE as usize, settings.audio());
    audio::wav_bytes(&samples, audio::SAMPLE_RATE)
}

/// Write the shared tone artifact and report its properties.
pub fn write_tone(path: &Path, settings: &Settings) -> std::io::Result<ToneArtifact> {
    let samples = audio::render_tone(440.0, audio::SAMPLE_RATE as usize, settings.audio());
    let wav = audio::wav_bytes(&samples, audio::SAMPLE_RATE);
    std::fs::write(path, &wav)?;
    Ok(ToneArtifact {
        samples: samples.len(),
        volume: settings.master_volume,
        muted: settings.muted,
        audible: !settings.muted && samples.iter().any(|sample| *sample != 0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_is_a_mono_sixteen_bit_riff_document() {
        let wav = tone_wav(&Settings::default());
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[20..22], &1u16.to_le_bytes(), "PCM format");
        assert_eq!(&wav[22..24], &1u16.to_le_bytes(), "mono channel count");
        assert_eq!(&wav[34..36], &16u16.to_le_bytes(), "16-bit samples");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + audio::SAMPLE_RATE as usize * 2);
    }

    #[test]
    fn mute_produces_a_silent_but_complete_artifact() {
        let settings = Settings {
            muted: true,
            ..Settings::default()
        };
        let artifact = write_tone(&temp_path("muted"), &settings).expect("write");
        assert_eq!(artifact.samples, audio::SAMPLE_RATE as usize);
        assert_eq!(artifact.volume, settings.master_volume);
        assert!(artifact.muted);
        assert!(!artifact.audible);
    }

    #[test]
    fn audible_volume_reports_a_non_silent_tone() {
        let artifact = write_tone(&temp_path("audible"), &Settings::default()).expect("write");
        assert!(!artifact.muted);
        assert!(artifact.audible);
        assert!(artifact.volume > 0);
    }

    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "gba-native-frontend-tone-{tag}-{}-{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or(0)
        ))
    }
}
