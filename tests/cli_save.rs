//! CLI contract for the host save library and the committed save-tester ROM.
//!
//! Synthetic fixture only; no retail ROM, save data, browser or network.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "gba-native-cli-save-{}-{}",
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
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

const FRESH_SAVE: &str = "39640902e8ceb5ebcce5769a9d891df7e0c3f1a9d8b0c3b0c175a34f3a823dca";
const LOADED_SAVE: &str = "bc470afabfeeece7e51c912dabc4178e507fd6535e1402b36370ddb4f6f1e938";

#[test]
fn multiple_slots_default_and_import_export_round_trip() {
    let temp = TempDir::new();
    let rom = repo_root().join("tests/roms/synthetic/save-tester.gba");
    let rom = rom.to_str().unwrap();
    let run = |slot: &str| {
        stdout(&cli(
            temp.path(),
            &[
                "run",
                rom,
                "--budget",
                "5000000",
                "--synthetic-backup-sram",
                "--save-dir",
                "output/saves",
                "--save-slot",
                slot,
            ],
        ))
    };
    let first = run("slot1");
    assert!(first.contains(&format!("save_sha256={FRESH_SAVE}")));
    assert!(first.contains("save_default=slot1"));
    let second = run("slot1");
    assert!(
        second.contains(&format!("save_sha256={LOADED_SAVE}")),
        "second run must read the persisted image: {second}"
    );
    let other = run("slot2");
    assert!(other.contains(&format!("save_sha256={FRESH_SAVE}")));

    let list = stdout(&cli(
        temp.path(),
        &["save", "list", rom, "--dir", "output/saves"],
    ));
    assert!(list.contains("slot=slot1 bytes=32768"));
    assert!(list.contains("slot=slot2 bytes=32768"));
    assert!(list.contains("default=1"));
    assert!(list.contains("slots=2"));

    let exported = stdout(&cli(
        temp.path(),
        &[
            "save",
            "export",
            rom,
            "slot2",
            "--dir",
            "output/saves",
            "--out",
            "output/export.sav",
        ],
    ));
    assert!(exported.contains(&format!("sha256={FRESH_SAVE}")));
    let imported = stdout(&cli(
        temp.path(),
        &[
            "save",
            "import",
            rom,
            "slot3",
            "--dir",
            "output/saves",
            "--file",
            "output/export.sav",
        ],
    ));
    assert!(imported.contains("slot=slot3"));
    assert!(imported.contains(&format!("sha256={FRESH_SAVE}")));

    let used = stdout(&cli(
        temp.path(),
        &["save", "use", rom, "slot3", "--dir", "output/saves"],
    ));
    assert!(used.contains("default=slot3"));
    let default = stdout(&cli(
        temp.path(),
        &["save", "default", rom, "--dir", "output/saves"],
    ));
    assert!(default.contains("default=slot3"));

    let deleted = stdout(&cli(
        temp.path(),
        &["save", "delete", rom, "slot1", "--dir", "output/saves"],
    ));
    assert!(deleted.contains("deleted=slot1"));
    assert!(deleted.contains("default=slot3"));
    let list = stdout(&cli(
        temp.path(),
        &["save", "list", rom, "--dir", "output/saves"],
    ));
    assert!(list.contains("slots=2"));
    assert!(!list.contains("slot=slot1 "));
}

#[test]
fn unknown_slot_and_bad_rom_fail_without_creating_state() {
    let temp = TempDir::new();
    let rom = repo_root().join("tests/roms/synthetic/save-tester.gba");
    let output = cli(
        temp.path(),
        &[
            "save",
            "export",
            rom.to_str().unwrap(),
            "missing",
            "--dir",
            "output/saves",
            "--out",
            "output/missing.sav",
        ],
    );
    assert!(!output.status.success());
    assert!(!temp.path().join("output/saves").exists());

    let output = cli(
        temp.path(),
        &[
            "run",
            rom.to_str().unwrap(),
            "--budget",
            "5000000",
            "--synthetic-backup-sram",
            "--save-dir",
            "output/saves",
            "--save-slot",
            "../escape",
        ],
    );
    assert!(!output.status.success());
}
