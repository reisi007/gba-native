//! `run --play` contract: explicit player invocation and fail-closed errors.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const ROM: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/roms/synthetic/input-tester.gba"
);
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "gba-native-cli-play-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl TempDir {
    fn path(&self) -> &Path {
        &self.0
    }
}

fn fake_player(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join("fake-player");
    std::fs::write(&path, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\n")
        .expect("write fake player");
    let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("chmod");
    path
}

fn cli(cwd: &Path, args: &[&str], player: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gba-native"));
    command.current_dir(cwd).args(args);
    match player {
        Some(player) => command.env("GBA_NATIVE_AUDIO_PLAYER", player),
        None => command.env_remove("GBA_NATIVE_AUDIO_PLAYER"),
    };
    command.output().expect("run CLI")
}

#[test]
fn play_invokes_the_configured_player_and_reports_it() {
    let temp = TempDir::new();
    let player = fake_player(temp.path());
    let output = cli(
        temp.path(),
        &[
            "run",
            ROM,
            "--budget",
            "200000",
            "--audio-out",
            "output/play.wav",
            "--play",
        ],
        Some(&player),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(printed.contains("audio_played=true"), "{printed}");
    assert!(
        printed.contains(&format!("audio_player={}", player.display())),
        "{printed}"
    );
    let recorded =
        std::fs::read_to_string(path_with_suffix(&player, ".args")).expect("player record");
    assert_eq!(recorded.trim(), "output/play.wav");
    let wav = std::fs::read(temp.path().join("output/play.wav")).expect("wav");
    assert_eq!(&wav[..4], b"RIFF");
}

#[test]
fn play_without_audio_out_fails_before_running() {
    let temp = TempDir::new();
    let output = cli(
        temp.path(),
        &["run", ROM, "--budget", "1000", "--play"],
        Some(&fake_player(temp.path())),
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--play needs --audio-out"), "{stderr}");
    assert!(!temp.path().join("output").exists());
}

#[test]
fn missing_explicit_player_fails_closed() {
    let temp = TempDir::new();
    let missing = temp.path().join("no-such-player");
    let output = cli(
        temp.path(),
        &[
            "run",
            ROM,
            "--budget",
            "200000",
            "--audio-out",
            "output/play.wav",
            "--play",
        ],
        Some(&missing),
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("GBA_NATIVE_AUDIO_PLAYER"), "{stderr}");
    assert!(stderr.contains("is not a file"), "{stderr}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("audio_played=true"));
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}
