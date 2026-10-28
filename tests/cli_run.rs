//! Synthetic CLI regressions for optional post-budget run snapshots.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::{demo, sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-cli-run-{}-{sequence}",
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
fn bounded_run_without_out_keeps_existing_contract_and_writes_nothing() {
    let temp = TempDir::new();
    let cwd = temp.path().join("cwd");
    std::fs::create_dir(&cwd).expect("create cwd");
    let rom = demo::demo_cartridge();
    std::fs::write(cwd.join("synthetic.gba"), rom.bytes()).expect("write synthetic ROM");

    let output = cli(&cwd, &["run", "synthetic.gba", "--budget", "1"]);
    assert!(
        output.status.success(),
        "run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "status=bounded",
        "instructions=1",
        "pc=0x08000100",
        "thumb=0",
        "note=stopped only by the instruction budget; no boot or compatibility claim",
    ] {
        assert!(
            stdout.contains(expected),
            "run output is missing {expected}"
        );
    }
    assert!(
        !cwd.join("output").exists(),
        "no --out must create no files"
    );
}

#[test]
fn bounded_run_writes_bitmap_and_truthful_metadata_when_requested() {
    let temp = TempDir::new();
    let cwd = temp.path().join("cwd");
    std::fs::create_dir(&cwd).expect("create cwd");
    let rom = demo::demo_cartridge();
    std::fs::write(cwd.join("synthetic.gba"), rom.bytes()).expect("write synthetic ROM");

    let output = cli(
        &cwd,
        &[
            "run",
            "synthetic.gba",
            "--budget",
            "1",
            "--out",
            "output/run-001",
        ],
    );
    assert!(
        output.status.success(),
        "run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr(&output)
    );

    let directory = cwd.join("output/run-001");
    let ppm = std::fs::read(directory.join("screen.ppm")).expect("snapshot PPM");
    let header = b"P6\n240 160\n255\n";
    assert!(ppm.starts_with(header));
    assert_eq!(ppm.len(), header.len() + 240 * 160 * 3);
    let report = std::fs::read_to_string(directory.join("report.txt")).expect("run report");
    let pixel_hash = sha256::hex(&sha256::digest(&ppm[header.len()..]));
    let file_hash = sha256::hex(&sha256::digest(&ppm));
    for expected in [
        "capture_kind=post_budget_bitmap_snapshot".to_string(),
        format!("rom_bytes={}", rom.len()),
        format!("rom_sha256={}", rom.sha256_hex()),
        "requested_instruction_budget=1".to_string(),
        "used_instruction_budget=1".to_string(),
        "stop_reason=instruction_budget".to_string(),
        "final_pc=0x08000100".to_string(),
        "final_cpsr=0x000000DF".to_string(),
        "final_thumb=0".to_string(),
        "display_dispcnt=0x0000".to_string(),
        "display_mode=0".to_string(),
        "display_forced_blank=0".to_string(),
        "display_bg0_enabled=0".to_string(),
        "display_bg2_enabled=0".to_string(),
        "display_backdrop_rgb555=0x0000".to_string(),
        "image_file=screen.ppm".to_string(),
        "image_width=240".to_string(),
        "image_height=160".to_string(),
        format!("image_sha256={pixel_hash}"),
        format!("image_file_sha256={file_hash}"),
        "not a frame, boot, hardware-timed render, gameplay, or compatibility claim".to_string(),
    ] {
        assert!(report.contains(&expected), "report is missing {expected}");
    }
    assert_eq!(
        std::fs::read_dir(&directory)
            .expect("run output directory")
            .count(),
        2,
        "only the image and text report are written"
    );
}

#[test]
fn run_and_snapshot_errors_create_no_output_directory() {
    let temp = TempDir::new();
    let cwd = temp.path().join("cwd");
    std::fs::create_dir(&cwd).expect("create cwd");

    // The documented HLE rejects unmodeled reset groups; run must not bypass it.
    synthetic_rom(
        &cwd.join("reset-error.gba"),
        &[(0, 0xE3A0_00FF), (4, 0xEF01_0000)],
    );
    let reset = cli(
        &cwd,
        &[
            "run",
            "reset-error.gba",
            "--budget",
            "2",
            "--out",
            "output/reset-error",
        ],
    );
    assert!(
        !reset.status.success(),
        "unsupported reset must remain an error"
    );
    assert!(stderr(&reset).contains("not modeled"), "{}", stderr(&reset));
    assert!(!cwd.join("output/reset-error").exists());

    // This fixture completes its budget but sets unsupported display mode 1.
    synthetic_rom(
        &cwd.join("display-error.gba"),
        &[
            (0, 0xE59F_0008), // LDR r0, [PC, #8] -> literal at 0x10
            (4, 0xE59F_1008), // LDR r1, [PC, #8] -> literal at 0x14
            (8, 0xE1C0_10B0), // STRH r1, [r0]
            (0x10, 0x0400_0000),
            (0x14, 1),
        ],
    );
    let display = cli(
        &cwd,
        &[
            "run",
            "display-error.gba",
            "--budget",
            "3",
            "--out",
            "output/display-error",
        ],
    );
    assert!(
        !display.status.success(),
        "unsupported snapshot must remain an error"
    );
    assert!(
        stderr(&display).contains("display mode 1"),
        "{}",
        stderr(&display)
    );
    assert!(!cwd.join("output/display-error").exists());
    assert!(
        !cwd.join("output").exists(),
        "failures must not create output/"
    );
}

#[test]
fn run_refuses_preexisting_output_directory_without_replacing_files() {
    let temp = TempDir::new();
    let cwd = temp.path().join("cwd");
    let existing = cwd.join("output/existing");
    std::fs::create_dir_all(&existing).expect("existing output directory");
    std::fs::write(existing.join("report.txt"), b"KEEP").expect("sentinel report");
    std::fs::write(cwd.join("synthetic.gba"), demo::demo_cartridge().bytes())
        .expect("write synthetic ROM");

    let output = cli(
        &cwd,
        &[
            "run",
            "synthetic.gba",
            "--budget",
            "1",
            "--out",
            "output/existing",
        ],
    );
    assert!(!output.status.success(), "existing output must be refused");
    assert!(stderr(&output).contains("existing directory"));
    assert_eq!(
        std::fs::read(existing.join("report.txt")).expect("sentinel survives"),
        b"KEEP"
    );
    assert!(!existing.join("screen.ppm").exists());
}

#[test]
fn run_validates_output_arguments_and_redirects() {
    let temp = TempDir::new();
    let cwd = temp.path().join("cwd");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&cwd).expect("create cwd");
    std::fs::create_dir(&outside).expect("create outside");
    std::fs::write(cwd.join("synthetic.gba"), demo::demo_cartridge().bytes())
        .expect("write synthetic ROM");

    for args in [
        vec!["run", "synthetic.gba", "--out"],
        vec!["run", "synthetic.gba", "--unknown"],
        vec!["run", "synthetic.gba", "--budget", "not-a-number"],
    ] {
        let output = cli(&cwd, &args);
        assert!(
            !output.status.success(),
            "invalid args unexpectedly succeeded"
        );
        assert!(!cwd.join("output").exists());
    }

    let escaped = cwd.join("escaped-run");
    let traversal = cli(
        &cwd,
        &[
            "run",
            "synthetic.gba",
            "--budget",
            "1",
            "--out",
            "output/../escaped-run",
        ],
    );
    assert!(!traversal.status.success());
    assert!(stderr(&traversal).contains("outside"));
    assert!(!escaped.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        std::fs::create_dir(cwd.join("output")).expect("output root");
        symlink(&outside, cwd.join("output/link")).expect("output redirect");
        let redirected = cli(
            &cwd,
            &[
                "run",
                "synthetic.gba",
                "--budget",
                "1",
                "--out",
                "output/link/run",
            ],
        );
        assert!(!redirected.status.success());
        assert!(stderr(&redirected).contains("symbolic link"));
        assert!(!outside.join("run").exists());
    }
}
