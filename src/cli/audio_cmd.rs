//! Dependency-free synthetic audio output command.

use std::path::PathBuf;

use gba_native::audio::{self, AudioSettings};
use gba_native::error::{Error, Result};

use super::output::write_safely;

pub(super) fn run(mut args: std::iter::Skip<std::env::Args>) -> Result<()> {
    let mut out = None;
    let mut frequency = 440.0f32;
    let mut seconds = 2.0f32;
    let mut volume = AudioSettings::default().master_volume;
    let mut muted = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out" => {
                out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    Error::Args("--out needs a WAV path under output/".to_string())
                })?));
            }
            "--frequency" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::Args("--frequency needs Hz".to_string()))?;
                frequency = value
                    .parse()
                    .map_err(|_| Error::Args(format!("invalid frequency {value}")))?;
            }
            "--seconds" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::Args("--seconds needs a duration".to_string()))?;
                seconds = value
                    .parse()
                    .map_err(|_| Error::Args(format!("invalid seconds {value}")))?;
            }
            "--volume" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::Args("--volume needs 0..=100".to_string()))?;
                volume = value
                    .parse()
                    .map_err(|_| Error::Args(format!("invalid volume {value}")))?;
                if volume > 100 {
                    return Err(Error::Args("--volume must be 0..=100".to_string()));
                }
            }
            "--mute" => muted = true,
            _ => return Err(Error::Args(format!("unknown audio option {argument}"))),
        }
    }
    let out = out.ok_or_else(|| Error::Args("audio needs --out output/<name>.wav".to_string()))?;
    if !(1.0..=20_000.0).contains(&frequency) {
        return Err(Error::Args("--frequency must be 1..=20000 Hz".to_string()));
    }
    if !(0.05..=30.0).contains(&seconds) {
        return Err(Error::Args("--seconds must be 0.05..=30".to_string()));
    }
    let frames = (f64::from(seconds) * f64::from(audio::SAMPLE_RATE)) as usize;
    let settings = AudioSettings::new(volume, muted);
    let samples = audio::render_tone(frequency, frames, settings);
    let wav = audio::wav_bytes(&samples, audio::SAMPLE_RATE);
    write_safely(&out, &wav)?;
    println!("kind=synthetic-square-tone");
    println!("wav={}", out.display());
    println!("sample_rate={}", audio::SAMPLE_RATE);
    println!("samples={}", samples.len());
    println!("frequency={frequency}");
    println!("master_volume={volume}");
    println!("muted={muted}");
    println!(
        "audible={}",
        !muted && samples.iter().any(|sample| *sample != 0)
    );
    println!(
        "wav_sha256={}",
        gba_native::sha256::hex(&gba_native::sha256::digest(&wav))
    );
    println!("note=dependency-free PCM output; live playback is a frontend concern");
    Ok(())
}
