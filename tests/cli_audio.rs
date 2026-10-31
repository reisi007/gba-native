//! CLI audio output contract: dependency-free PCM/WAV, volume and mute.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "gba-native-cli-audio-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }
}

impl TempDir {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cli(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gba-native"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run CLI")
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn audio_writes_a_canonical_audible_wav() {
    let temp = TempDir::new();
    let output = cli(
        temp.path(),
        &[
            "audio",
            "--out",
            "output/tone.wav",
            "--frequency",
            "440",
            "--seconds",
            "0.25",
            "--volume",
            "80",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let printed = text(&output);
    assert!(printed.contains("sample_rate=32768"));
    assert!(printed.contains("samples=8192"));
    assert!(printed.contains("audible=true"));
    let wav = std::fs::read(temp.path().join("output/tone.wav")).expect("wav");
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(wav.len(), 44 + 8192 * 2);
    assert!(wav[44..].chunks(2).any(|sample| sample != [0, 0]));
}

#[test]
fn mute_produces_digital_silence_and_volume_changes_the_digest() {
    let temp = TempDir::new();
    let loud = cli(
        temp.path(),
        &["audio", "--out", "output/loud.wav", "--seconds", "0.1"],
    );
    assert!(loud.status.success());
    let quiet = cli(
        temp.path(),
        &[
            "audio",
            "--out",
            "output/quiet.wav",
            "--seconds",
            "0.1",
            "--volume",
            "20",
        ],
    );
    assert!(quiet.status.success());
    let muted = cli(
        temp.path(),
        &[
            "audio",
            "--out",
            "output/muted.wav",
            "--seconds",
            "0.1",
            "--mute",
        ],
    );
    assert!(muted.status.success());
    let loud_wav = std::fs::read(temp.path().join("output/loud.wav")).unwrap();
    let quiet_wav = std::fs::read(temp.path().join("output/quiet.wav")).unwrap();
    let muted_wav = std::fs::read(temp.path().join("output/muted.wav")).unwrap();
    assert_ne!(loud_wav, quiet_wav);
    assert!(muted_wav[44..].iter().all(|byte| *byte == 0));
    assert!(text(&muted).contains("audible=false"));
}

#[test]
fn invalid_audio_arguments_and_outputs_fail_without_files() {
    let temp = TempDir::new();
    let outside = temp.path().join("outside.wav");
    let output = cli(temp.path(), &["audio", "--out", outside.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(!outside.exists());
    let output = cli(
        temp.path(),
        &["audio", "--out", "output/x.wav", "--volume", "101"],
    );
    assert!(!output.status.success());
    assert!(!temp.path().join("output/x.wav").exists());
}
