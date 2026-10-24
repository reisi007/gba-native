#![cfg(unix)]
//! Failure paths of the documented short/full gate scripts.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-scripts-{tag}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn path_with_cargo(extra: &Path) -> String {
    let cargo = Path::new(env!("CARGO"));
    let cargo_dir = cargo.parent().expect("cargo directory");
    format!(
        "{}:{}:{}",
        extra.display(),
        cargo_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn fake_ratchet(directory: &Path, version: &str, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = directory.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nprintf 'ratchet {version}\\n'\n"))
        .expect("write fake ratchet");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

fn run(script: &str, path: &str, ratchet: Option<&Path>) -> Output {
    let mut command = Command::new("bash");
    command
        .arg(repo_root().join("scripts").join(script))
        .current_dir(repo_root())
        .env("PATH", path)
        .env_remove("RATCHET_BIN");
    if let Some(ratchet) = ratchet {
        command.env("RATCHET_BIN", ratchet);
    }
    command.output().expect("run gate script")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn short_fails_closed_when_ratchet_is_missing() {
    let temp = TempDir::new("missing-ratchet");
    let missing = temp.path().join("does-not-exist");
    let output = run("short.sh", &path_with_cargo(temp.path()), Some(&missing));
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(message.contains("RATCHET_BIN"), "{message}");
    assert!(message.contains("not an executable"), "{message}");
}

#[test]
fn the_ratchet_version_is_pinned_and_a_mismatch_fails() {
    let temp = TempDir::new("wrong-ratchet");
    let wrong = fake_ratchet(temp.path(), "9.9.9", "ratchet");
    let output = run("short.sh", &path_with_cargo(temp.path()), Some(&wrong));
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(message.contains("expected ratchet 0.5.0"), "{message}");
    assert!(message.contains("9.9.9"), "{message}");
}

#[test]
fn full_fails_closed_when_cargo_is_missing() {
    let output = run("full.sh", "/usr/bin:/bin", None);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("cargo is not on PATH"),
        "{}",
        stderr(&output)
    );
}
