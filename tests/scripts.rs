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

#[test]
fn web_bootstrap_self_test_runs_before_clean_checkout_provisioning() {
    let output = Command::new("bash")
        .arg(repo_root().join("tests/bootstrap-web-tools-clean-checkout.sh"))
        .current_dir(repo_root())
        .output()
        .expect("run clean-checkout bootstrap regression");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn browser_gpu_launch_args_are_platform_specific_and_shared_by_both_paths() {
    let root = repo_root();
    let helper = root.join("scripts/web-gpu-launch-args.sh");
    let metadata = std::fs::metadata(&helper).expect("WebGPU launch-args helper exists");
    assert!(metadata.is_file());
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(
        metadata.permissions().mode() & 0o111,
        0,
        "launch-args helper must be executable"
    );

    let common = [
        "--enable-unsafe-webgpu",
        "--use-webgpu-adapter=swiftshader",
        "--enable-dawn-features=allow_unsafe_apis",
        "--disable-dawn-features=use_dxc",
        "--enable-webgpu-developer-features",
        "--use-gpu-in-tests",
        "--enable-accelerated-2d-canvas",
    ];
    let linux = Command::new(&helper)
        .arg("Linux")
        .output()
        .expect("run Linux launch-args contract");
    assert!(linux.status.success());
    let linux_args: Vec<_> = String::from_utf8(linux.stdout)
        .expect("UTF-8 launch args")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        linux_args,
        common
            .into_iter()
            .chain([
                "--enable-features=Vulkan",
                "--use-angle=vulkan",
                "--use-vulkan=swiftshader",
                "--disable-vulkan-surface",
            ])
            .map(str::to_owned)
            .collect::<Vec<_>>()
    );

    for platform in ["Darwin", "MINGW64_NT-10.0"] {
        let output = Command::new(&helper)
            .arg(platform)
            .output()
            .expect("run non-Linux launch-args contract");
        assert!(output.status.success());
        let args: Vec<_> = String::from_utf8(output.stdout)
            .expect("UTF-8 launch args")
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        assert_eq!(args, common, "platform {platform}");
    }

    for script in ["web-e2e.sh", "web-screenshots.sh"] {
        let contents = std::fs::read_to_string(root.join("scripts").join(script))
            .expect("read browser launch script");
        assert!(
            contents.contains("$ROOT/scripts/web-gpu-launch-args.sh"),
            "{script} must use the shared platform-aware launch preset"
        );
        assert!(
            contents.contains("${WEB_E2E_LAUNCH_ARGS:-$DEFAULT_LAUNCH_ARGS}"),
            "{script} must preserve non-empty user override semantics"
        );
        assert!(
            contents.contains("WEB_E2E_LAUNCH_ARGS=\"$LAUNCH_ARGS\""),
            "{script} must pass its final launch args to Playwright"
        );
    }
}
