//! Shared test-tone artifact with native, iOS and browser playback adapters.

use gba_native::input::Settings;

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function runePlayTone(bytes) {
  try {
    const context = new AudioContext();
    const payload = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
    context.resume();
    context.decodeAudioData(payload).then((buffer) => {
      const source = context.createBufferSource();
      source.buffer = buffer;
      source.connect(context.destination);
      source.onended = () => context.close();
      source.start();
    }).catch(() => context.close());
    return true;
  } catch (_) { return false; }
}
let guestContext = null;
let guestCursor = 0;
export function runeStartGuestAudio() {
  try {
    guestContext ??= new AudioContext();
    guestContext.resume();
    return true;
  } catch (_) { return false; }
}
export function runePlayGuestSamples(samples) {
  try {
    if (!samples.length) return true;
    guestContext ??= new AudioContext();
    guestContext.resume();
    const buffer = guestContext.createBuffer(1, samples.length, 32768);
    buffer.copyToChannel(Float32Array.from(samples, (sample) => sample / 32768), 0);
    const source = guestContext.createBufferSource();
    source.buffer = buffer;
    source.connect(guestContext.destination);
    const start = Math.max(guestContext.currentTime, guestCursor);
    source.start(start);
    guestCursor = start + buffer.duration;
    return true;
  } catch (_) { return false; }
}
export function runeDownloadTone(bytes) {
  try {
    const blob = new Blob([bytes], { type: "audio/wav" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url; link.download = "gba-native-tone.wav"; link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    return true;
  } catch (_) { return false; }
}
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runePlayTone)]
    fn browser_play_tone(bytes: &[u8]) -> bool;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeStartGuestAudio)]
    fn browser_start_guest_audio() -> bool;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runePlayGuestSamples)]
    fn browser_play_guest_samples(samples: &[i16]) -> bool;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = runeDownloadTone)]
    fn browser_download_tone(bytes: &[u8]) -> bool;
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn start_guest_audio() -> bool {
    browser_start_guest_audio()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn play_guest_samples(samples: &[i16]) -> bool {
    browser_play_guest_samples(samples)
}

pub(crate) fn create_artifact(settings: &Settings) -> Result<String, String> {
    let bytes = gba_native::frontend::tone::tone_wav(settings);
    #[cfg(target_arch = "wasm32")]
    {
        if browser_download_tone(&bytes) {
            Ok("downloaded gba-native-tone.wav".to_owned())
        } else {
            Err("browser WAV download is unavailable".to_owned())
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = tone_path()?;
        write_native_artifact(&path, settings)?;
        Ok(format!("wrote {} ({} bytes)", path.display(), bytes.len()))
    }
}

pub(crate) fn play(settings: &Settings) -> Result<String, String> {
    #[cfg(target_arch = "wasm32")]
    let bytes = gba_native::frontend::tone::tone_wav(settings);
    #[cfg(target_arch = "wasm32")]
    {
        if browser_play_tone(&bytes) {
            Ok("requested Web Audio playback; browser output is not confirmed".to_owned())
        } else {
            Err("Web Audio is unavailable or blocked by the browser".to_owned())
        }
    }
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios")))]
    {
        let path = tone_path()?;
        let artifact = write_native_artifact(&path, settings)?;
        let player = gba_native::frontend::player::play(&path)?;
        Ok(format!(
            "played via {} ({} samples, {}% volume, muted={})",
            player.display(),
            artifact.samples,
            artifact.volume,
            artifact.muted
        ))
    }
    #[cfg(all(not(target_arch = "wasm32"), target_os = "ios"))]
    {
        let path = tone_path()?;
        write_native_artifact(&path, settings)?;
        rune_ios_platform::play_test_tone(&path)?;
        Ok("requested AVFoundation playback; audio output is not confirmed".to_owned())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn tone_path() -> Result<PathBuf, String> {
    crate::storage::tone_path()
        .ok_or_else(|| "no writable tone-artifact directory is available".to_owned())
}

#[cfg(not(target_arch = "wasm32"))]
fn write_native_artifact(
    path: &std::path::Path,
    settings: &Settings,
) -> Result<gba_native::frontend::tone::ToneArtifact, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    gba_native::frontend::tone::write_tone(path, settings)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_artifact_uses_the_shared_core_wav_and_mute_settings() {
        let audible = gba_native::frontend::tone::tone_wav(&Settings::default());
        assert_eq!(&audible[..4], b"RIFF");
        let quiet = gba_native::frontend::tone::tone_wav(&Settings {
            master_volume: 25,
            ..Settings::default()
        });
        assert_ne!(&quiet[44..], &audible[44..]);
        let silent = gba_native::frontend::tone::tone_wav(&Settings {
            muted: true,
            ..Settings::default()
        });
        assert_eq!(&silent[..4], b"RIFF");
        assert!(silent[44..].iter().all(|sample| *sample == 0));
    }
}
