//! Synthetic CLI regressions for the explicit backup-capacity and affine opt-ins.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-cli-options-{}-{sequence}",
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

fn cli(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gba-native"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run gba-native CLI")
}

fn synthetic_rom(path: &Path, instructions: &[(usize, u32)]) {
    let mut bytes = vec![0; 0x200];
    for &(offset, instruction) in instructions {
        bytes[offset..offset + 4].copy_from_slice(&instruction.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write synthetic ROM");
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn run_synthetic_backup_sram64k_is_explicit_and_reported_once() {
    let temp = TempDir::new();
    let cwd = temp.path().join("cwd");
    std::fs::create_dir(&cwd).expect("create cwd");
    synthetic_rom(&cwd.join("plain.gba"), &[(0, 0xE3A0_0000)]);

    let output = cli(
        &cwd,
        &[
            "run",
            "plain.gba",
            "--budget",
            "1",
            "--synthetic-backup-sram64k",
            "--out",
            "output/backup64",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("backup_policy=sram-64k-volatile"),
        "stdout is missing the selected backup policy"
    );
    assert_eq!(
        stdout.matches("backup_assumption=").count(),
        1,
        "the backup assumption must be printed exactly once"
    );
    assert!(stdout.contains("64 KiB"), "{stdout}");
    let report = std::fs::read_to_string(cwd.join("output/backup64/report.txt")).expect("report");
    assert!(
        report.contains("backup_policy=sram-64k-volatile"),
        "{report}"
    );
}

#[test]
fn run_rejects_both_backup_capacity_flags() {
    let temp = TempDir::new();
    let cwd = temp.path().join("cwd");
    std::fs::create_dir(&cwd).expect("create cwd");
    synthetic_rom(&cwd.join("plain.gba"), &[(0, 0xE3A0_0000)]);

    let output = cli(
        &cwd,
        &[
            "run",
            "plain.gba",
            "--budget",
            "1",
            "--synthetic-backup-sram",
            "--synthetic-backup-sram64k",
        ],
    );
    assert!(!output.status.success(), "contradictory flags must fail");
    assert!(
        stderr(&output).contains("choose only one"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn run_synthetic_bios_affine_is_explicit_and_reported_once() {
    let temp = TempDir::new();
    let cwd = temp.path().join("cwd");
    std::fs::create_dir(&cwd).expect("create cwd");
    synthetic_rom(&cwd.join("plain.gba"), &[(0, 0xE3A0_0000)]);

    let output = cli(
        &cwd,
        &[
            "run",
            "plain.gba",
            "--budget",
            "1",
            "--synthetic-bios-affine",
            "--out",
            "output/affine",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("affine_policy=documented"),
        "stdout is missing the selected affine policy"
    );
    assert_eq!(
        stdout.matches("affine_assumption=").count(),
        1,
        "the affine assumption must be printed exactly once"
    );
    let report = std::fs::read_to_string(cwd.join("output/affine/report.txt")).expect("report");
    assert!(report.contains("affine_policy=documented"), "{report}");
    assert!(report.contains("affine_note="), "{report}");
}
