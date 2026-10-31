//! CLI contract for input scripts and host performance statistics.
//!
//! Uses the committed synthetic input-tester fixture. No retail ROM, browser or
//! network is involved.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "gba-native-cli-input-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::SeqCst)
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
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn cli(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gba-native"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run gba-native CLI")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn run_with_input_script_reports_expected_frame_and_performance() {
    let temp = TempDir::new();
    let rom = repo_root().join("tests/roms/synthetic/input-tester.gba");
    let script = repo_root().join("tests/roms/synthetic/input-tester.input");
    let output = cli(
        temp.path(),
        &[
            "run",
            rom.to_str().unwrap(),
            "--budget",
            "2000000",
            "--input",
            script.to_str().unwrap(),
            "--out",
            "output/run",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains("instructions=2000000"));
    assert!(text.contains("perf_guest_instructions_per_second="));
    assert!(text.contains("perf_realtime_factor="));
    assert!(text.contains("perf_faster_than_realtime="));
    assert!(text.contains("not one hardware cycle"));

    let run = temp.path().join("output/run");
    let report = std::fs::read_to_string(run.join("report.txt")).expect("report");
    assert!(
        report.contains(
            "image_sha256=5cbbd998a2bc5a026ebac07d91a0f058b769cb05dd14c09e30d52888ed80db8b"
        )
    );
    assert_eq!(
        std::fs::read(run.join("screen.ppm")).expect("screen"),
        std::fs::read(repo_root().join("tests/roms/synthetic/input-tester.expected.ppm"))
            .expect("expected")
    );
}

#[test]
fn missing_input_script_fails_before_creating_output() {
    let temp = TempDir::new();
    let rom = repo_root().join("tests/roms/synthetic/input-tester.gba");
    let output = cli(
        temp.path(),
        &[
            "run",
            rom.to_str().unwrap(),
            "--input",
            "output/does-not-exist.input",
            "--out",
            "output/should-not-exist",
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read input script"));
    assert!(!temp.path().join("output/should-not-exist").exists());
}

#[test]
fn invalid_input_script_fails_before_creating_output() {
    let temp = TempDir::new();
    std::fs::create_dir_all(temp.path().join("output")).expect("output");
    std::fs::write(temp.path().join("output/bad.input"), "0:NOTAKEY:10\n").expect("script");
    let rom = repo_root().join("tests/roms/synthetic/input-tester.gba");
    let output = cli(
        temp.path(),
        &[
            "run",
            rom.to_str().unwrap(),
            "--input",
            "output/bad.input",
            "--out",
            "output/should-not-exist",
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid input script"));
    assert!(!temp.path().join("output/should-not-exist").exists());
}

#[test]
fn fixture_command_writes_the_committed_synthetic_rom() {
    let temp = TempDir::new();
    let output = cli(
        temp.path(),
        &[
            "fixture",
            "input-tester",
            "--out",
            "output/input-tester.gba",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(
        text.contains("sha256=8a7667d43d9ec0333a5a7c0a032837919e101ca1c1893bdf5f95f51b27555539")
    );
    assert_eq!(
        std::fs::read(temp.path().join("output/input-tester.gba")).expect("generated"),
        std::fs::read(repo_root().join("tests/roms/synthetic/input-tester.gba"))
            .expect("committed")
    );
}
