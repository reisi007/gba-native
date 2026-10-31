//! Explicit, fail-closed WAV playback through an installed platform player.
//!
//! This is the shared contract used by the desktop app and mirrors the CLI's
//! `--play` rules: nothing is downloaded, no player is guessed silently, and a
//! missing or failing player is an error. `GBA_NATIVE_AUDIO_PLAYER` may name an
//! explicit executable. Android implements the same feature with
//! `android.media.MediaPlayer`, because a POSIX player binary is not available
//! there.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Environment override naming the player executable.
pub const ENV_OVERRIDE: &str = "GBA_NATIVE_AUDIO_PLAYER";

const FILE_TOKEN: &str = "@FILE@";

/// A resolved platform player command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    program: PathBuf,
    args: Vec<String>,
    quote_file: bool,
}

impl Player {
    /// Program reported after a successful run.
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Run the player on one WAV file, returning the program that handled it.
    pub fn run(&self, path: &Path) -> Result<PathBuf, String> {
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
                format!(
                    "cannot run audio player {}: {error}",
                    self.program.display()
                )
            })?;
        if !status.success() {
            return Err(format!(
                "audio player {} failed with {status}",
                self.program.display()
            ));
        }
        Ok(self.program.clone())
    }
}

/// Play one WAV through the environment-configured or detected player.
pub fn play(path: &Path) -> Result<PathBuf, String> {
    let override_value = std::env::var_os(ENV_OVERRIDE);
    let path_env = std::env::var_os("PATH");
    detect(override_value.as_deref(), path_env.as_deref())?.run(path)
}

/// Resolve a player from an optional override and an optional `PATH`.
pub fn detect(override_value: Option<&OsStr>, path_env: Option<&OsStr>) -> Result<Player, String> {
    if let Some(value) = override_value {
        if value.is_empty() {
            return Err(format!("{ENV_OVERRIDE} is empty"));
        }
        let program = PathBuf::from(value);
        if program.components().count() > 1 && !program.is_file() {
            return Err(format!(
                "{ENV_OVERRIDE}={} is not an executable file",
                program.display()
            ));
        }
        return Ok(Player {
            program,
            args: Vec::new(),
            quote_file: false,
        });
    }
    if cfg!(target_os = "windows") {
        return detect_windows(path_env);
    }
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("afplay", &[])]
    } else {
        &[("paplay", &[]), ("aplay", &["-q", FILE_TOKEN])]
    };
    for (program, args) in candidates {
        if let Some(found) = find_in_path(program, path_env) {
            return Ok(Player {
                program: found,
                args: args.iter().map(|arg| (*arg).to_string()).collect(),
                quote_file: false,
            });
        }
    }
    let tried: Vec<&str> = candidates.iter().map(|(program, _)| *program).collect();
    Err(format!(
        "found no installed audio player (tried {}); install one or set {ENV_OVERRIDE}=<program>",
        tried.join(", ")
    ))
}

fn detect_windows(path_env: Option<&OsStr>) -> Result<Player, String> {
    let found = find_in_path("powershell", path_env).ok_or_else(|| {
        format!("found no installed audio player (tried powershell); set {ENV_OVERRIDE}=<program>")
    })?;
    Ok(Player {
        program: found,
        args: vec![
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-Command".to_string(),
            format!("(New-Object Media.SoundPlayer '{FILE_TOKEN}').PlaySync()"),
        ],
        quote_file: true,
    })
}

fn find_in_path(name: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
    let path_env = path_env?;
    for directory in std::env::split_paths(path_env) {
        let mut candidate = directory.join(name);
        if cfg!(target_os = "windows") {
            candidate.set_extension("exe");
        }
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn temp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gba-native-frontend-player-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or(0)
        ))
    }

    fn script(root: &Path, name: &str, body: &str) -> PathBuf {
        fs::create_dir_all(root).expect("dir");
        let path = root.join(name);
        fs::write(&path, body).expect("write");
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("chmod");
        path
    }

    #[test]
    fn an_empty_or_missing_override_fails_closed() {
        assert!(detect(Some(OsStr::new("")), None).is_err());
        assert!(detect(Some(OsStr::new("/nonexistent/player")), None).is_err());
    }

    #[test]
    fn an_explicit_override_wins_and_runs() {
        let root = temp("override");
        let player = script(&root, "player", "#!/bin/sh\nexit 0\n");
        let resolved = detect(Some(player.as_os_str()), None).expect("override");
        assert_eq!(resolved.program(), player);
        let wav = root.join("tone.wav");
        fs::write(&wav, b"RIFF").expect("wav");
        assert_eq!(resolved.run(&wav).expect("run"), player);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_failing_player_is_an_explicit_error() {
        let root = temp("failing");
        let player = script(&root, "player", "#!/bin/sh\nexit 7\n");
        let resolved = detect(Some(player.as_os_str()), None).expect("override");
        let error = resolved.run(&root.join("tone.wav")).expect_err("must fail");
        assert!(error.contains("failed with"), "{error}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn path_search_finds_a_candidate_and_reports_when_none_exists() {
        let root = temp("path");
        let player = script(&root, "afplay", "#!/bin/sh\nexit 0\n");
        let path_env = root.as_os_str();
        let resolved = detect(None, Some(path_env)).expect("detected");
        assert_eq!(resolved.program(), player);
        let empty = temp("empty-path");
        fs::create_dir_all(&empty).expect("dir");
        assert!(detect(None, Some(empty.as_os_str())).is_err());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(empty);
    }
}
