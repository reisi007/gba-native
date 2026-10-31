//! Explicit, fail-closed native WAV playback through an installed player.
//!
//! The emulator itself stays dependency-free: `--play` only runs a platform
//! audio player that is already installed. Nothing is downloaded, no fallback
//! player is guessed silently, and a missing or failing player is an error.
//! `GBA_NATIVE_AUDIO_PLAYER` can name an explicit executable for tests or
//! unusual systems.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use gba_native::error::{Error, Result};

/// Environment override naming the player executable.
pub(super) const ENV_OVERRIDE: &str = "GBA_NATIVE_AUDIO_PLAYER";

const FILE_TOKEN: &str = "@FILE@";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Player {
    pub(super) program: PathBuf,
    args: Vec<String>,
    quote_file: bool,
}

impl Player {
    fn direct(program: &str, args: &[&str]) -> Self {
        Self {
            program: PathBuf::from(program),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            quote_file: false,
        }
    }

    fn run(&self, path: &Path) -> Result<PathBuf> {
        let file = path.display().to_string();
        let quoted = file.replace('\'', "''");
        let mut args: Vec<String> = self
            .args
            .iter()
            .map(|arg| arg.replace(FILE_TOKEN, if self.quote_file { &quoted } else { &file }))
            .collect();
        if !self.args.iter().any(|arg| arg.contains(FILE_TOKEN)) {
            args.push(file);
        }
        let status = Command::new(&self.program)
            .args(&args)
            .status()
            .map_err(|error| {
                Error::Args(format!(
                    "cannot run audio player {}: {error}",
                    self.program.display()
                ))
            })?;
        if !status.success() {
            return Err(Error::Args(format!(
                "audio player {} failed with {status}",
                self.program.display()
            )));
        }
        Ok(self.program.clone())
    }
}

/// Play one generated WAV, returning the player that handled it.
pub(super) fn play(path: &Path) -> Result<PathBuf> {
    let override_value = std::env::var_os(ENV_OVERRIDE);
    let path_env = std::env::var_os("PATH");
    resolve_player(override_value.as_deref(), path_env.as_deref())?.run(path)
}

fn resolve_player(override_value: Option<&OsStr>, path_env: Option<&OsStr>) -> Result<Player> {
    if let Some(value) = override_value {
        if value.is_empty() {
            return Err(Error::Args(format!("{ENV_OVERRIDE} is empty")));
        }
        let program = PathBuf::from(value);
        if program.components().count() > 1 && !program.is_file() {
            return Err(Error::Args(format!(
                "{ENV_OVERRIDE}={} is not a file",
                program.display()
            )));
        }
        return Ok(Player::direct(&program.display().to_string(), &[]));
    }
    let candidates = candidates();
    for candidate in &candidates {
        if let Some(program) = find_in_path(&candidate.program, path_env) {
            return Ok(Player {
                program,
                ..candidate.clone()
            });
        }
    }
    let names: Vec<String> = candidates
        .iter()
        .map(|candidate| candidate.program.display().to_string())
        .collect();
    Err(Error::Args(format!(
        "--play found no installed audio player (tried {}); install one or set {ENV_OVERRIDE}=<program>",
        names.join(", ")
    )))
}

fn find_in_path(name: &Path, path_env: Option<&OsStr>) -> Option<PathBuf> {
    let path_env = path_env?;
    for directory in std::env::split_paths(path_env) {
        let candidate = directory.join(executable_name(name));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(windows)]
fn executable_name(name: &Path) -> PathBuf {
    if name.extension().is_some() {
        name.to_path_buf()
    } else {
        name.with_extension("exe")
    }
}

#[cfg(not(windows))]
fn executable_name(name: &Path) -> PathBuf {
    name.to_path_buf()
}

fn candidates() -> Vec<Player> {
    if cfg!(target_os = "macos") {
        vec![Player::direct("afplay", &[])]
    } else if cfg!(target_os = "windows") {
        vec![Player {
            program: PathBuf::from("powershell"),
            args: vec![
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-Command".to_string(),
                format!("(New-Object Media.SoundPlayer '{FILE_TOKEN}').PlaySync()"),
            ],
            quote_file: true,
        }]
    } else {
        vec![
            Player::direct("paplay", &[]),
            Player::direct("aplay", &["-q"]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "gba-native-play-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Retry a test-only script exec that briefly reports `ETXTBSY`. A
    /// concurrently forked test process can inherit the descriptor while the
    /// script is written; the retry keeps the assertion about playback, not
    /// about that fork race.
    #[cfg(unix)]
    fn retry_text_file_busy<T>(mut operation: impl FnMut() -> Result<T>) -> Result<T> {
        for _ in 0..50 {
            match operation() {
                Err(error) if format!("{error}").contains("Text file busy") => {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                outcome => return outcome,
            }
        }
        operation()
    }

    #[cfg(unix)]
    fn fake_player(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join("fake-player");
        fs::write(&path, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\n")
            .expect("write fake player");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("chmod");
        path
    }

    #[cfg(unix)]
    #[test]
    fn explicit_override_runs_and_records_the_wav_path() {
        let temp = TempDir::new();
        let player = fake_player(&temp.0);
        let wav = temp.0.join("tone.wav");
        fs::write(&wav, b"RIFF").expect("wav");
        let resolved = resolve_player(Some(player.as_os_str()), None).expect("resolve override");
        assert_eq!(resolved.program, player);
        let used = retry_text_file_busy(|| resolved.run(&wav)).expect("run player");
        assert_eq!(used, player);
        let recorded = fs::read_to_string(player.with_extension("args")).expect("record");
        assert_eq!(recorded.trim(), wav.display().to_string());
    }

    #[cfg(unix)]
    #[test]
    fn path_search_finds_the_first_candidate_and_fails_closed() {
        let temp = TempDir::new();
        let player = fake_player(&temp.0);
        let path_env = temp.0.as_os_str();
        let found = find_in_path(Path::new("fake-player"), Some(path_env)).expect("found");
        assert_eq!(found, player);
        assert!(find_in_path(Path::new("missing-player"), Some(path_env)).is_none());
        let error = resolve_player(None, Some(path_env)).expect_err("no candidate");
        let message = format!("{error}");
        assert!(
            message.contains("found no installed audio player"),
            "{message}"
        );
        assert!(message.contains(ENV_OVERRIDE), "{message}");
    }

    #[cfg(unix)]
    #[test]
    fn failing_player_is_an_explicit_error() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new();
        let player = temp.0.join("failing-player");
        fs::write(&player, "#!/bin/sh\nexit 7\n").expect("write");
        let mut permissions = fs::metadata(&player).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&player, permissions).expect("chmod");
        let wav = temp.0.join("tone.wav");
        fs::write(&wav, b"RIFF").expect("wav");
        let error =
            retry_text_file_busy(|| Player::direct(&player.display().to_string(), &[]).run(&wav))
                .expect_err("failing player");
        assert!(format!("{error}").contains("failed with"), "{error}");
    }

    #[test]
    fn empty_override_is_rejected() {
        let error = resolve_player(Some(OsStr::new("")), None).expect_err("empty override");
        assert!(format!("{error}").contains("is empty"));
    }
}
