//! Desktop tone playback.
//!
//! The desktop app writes the shared tone WAV and hands it to an installed
//! platform player through the shared `frontend::player` rules. A missing or
//! failing player is printed, never silently ignored.

use std::path::Path;

use super::Frontend;

impl Frontend {
    pub(super) fn play_tone(&self) {
        let directory = self
            .settings_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let path = directory.join("gba-native-tone.wav");
        let artifact = match gba_native::frontend::tone::write_tone(&path, &self.settings) {
            Ok(artifact) => artifact,
            Err(error) => {
                eprintln!("audio: cannot write {}: {error}", path.display());
                return;
            }
        };
        match gba_native::frontend::player::play(&path) {
            Ok(program) => println!(
                "audio: played {} (samples={} volume={} muted={} audible={})",
                program.display(),
                artifact.samples,
                artifact.volume,
                artifact.muted,
                artifact.audible
            ),
            Err(error) => eprintln!("audio: {error}"),
        }
    }
}
