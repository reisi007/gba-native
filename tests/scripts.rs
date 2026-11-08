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
    // The version itself lives in scripts/lib.sh; the test asserts the
    // shape of the message, so a bump does not have to touch it.
    assert!(message.contains("expected ratchet "), "{message}");
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
fn browser_tool_bootstrap_self_test_runs_in_a_clean_checkout() {
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
fn rune_browser_screenshots_use_the_shared_webgpu_launch_preset() {
    use std::os::unix::fs::PermissionsExt;

    let root = repo_root();
    let helper = root.join("scripts/web-gpu-launch-args.sh");
    let metadata = std::fs::metadata(&helper).expect("shared launch-args helper exists");
    assert!(metadata.is_file());
    assert_ne!(metadata.permissions().mode() & 0o111, 0);

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
            .iter()
            .copied()
            .chain([
                "--enable-features=Vulkan",
                "--use-angle=swiftshader",
                "--use-vulkan=swiftshader",
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
        assert_eq!(args, common.map(str::to_owned), "platform {platform}");
    }

    let screenshots = std::fs::read_to_string(root.join("scripts/rune-web-screenshots.sh"))
        .expect("Rune browser screenshot runner");
    assert!(screenshots.contains("$ROOT/scripts/web-gpu-launch-args.sh"));
    assert!(screenshots.contains("RUNE_E2E_LAUNCH_ARGS=\"${RUNE_E2E_LAUNCH_ARGS:-$LAUNCH_ARGS}\""));
}

fn android_ndk_host_tag() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else {
        "linux-x86_64"
    }
}

fn fake_android_ndk(root: &Path, api: u32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin = root
        .join("sdk/ndk/29.0.0/toolchains/llvm/prebuilt")
        .join(android_ndk_host_tag())
        .join("bin");
    std::fs::create_dir_all(&bin).expect("create fake NDK bin");
    for name in [
        "aarch64-linux-android",
        "armv7a-linux-androideabi",
        "x86_64-linux-android",
    ] {
        let path = bin.join(format!("{name}{api}-clang"));
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write fake clang");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let readelf = bin.join("llvm-readelf");
    std::fs::write(&readelf, "#!/bin/sh\nexit 0\n").expect("write fake readelf");
    std::fs::set_permissions(&readelf, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    root.join("sdk/ndk/29.0.0")
}

fn fake_rustup_without_targets(directory: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = directory.join("rustup");
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write fake rustup");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

#[test]
fn android_gate_fails_closed_without_an_ndk() {
    let temp = TempDir::new("android-missing-ndk");
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).expect("create home");
    let output = Command::new("bash")
        .arg(repo_root().join("scripts/android.sh"))
        .current_dir(repo_root())
        .env("PATH", path_with_cargo(temp.path()))
        .env("HOME", &home)
        .env("ANDROID_HOME", &home)
        .env_remove("ANDROID_NDK_HOME")
        .env_remove("ANDROID_NDK_ROOT")
        .env_remove("ANDROID_SDK_ROOT")
        .output()
        .expect("run android gate");
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(message.contains("no Android NDK found"), "{message}");
    assert!(message.contains("ANDROID_NDK_HOME"), "{message}");
}

#[test]
fn android_gate_fails_closed_when_rust_android_targets_are_missing() {
    let temp = TempDir::new("android-missing-target");
    let home = temp.path().join("home");
    let tools = temp.path().join("tools");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(&tools).expect("create tools");
    let ndk = fake_android_ndk(temp.path(), 21);
    fake_rustup_without_targets(&tools);
    let output = Command::new("bash")
        .arg(repo_root().join("scripts/android.sh"))
        .args(["--abi", "aarch64-linux-android"])
        .current_dir(repo_root())
        .env(
            "PATH",
            format!("{}:{}", tools.display(), path_with_cargo(temp.path())),
        )
        .env("HOME", &home)
        .env("ANDROID_NDK_HOME", &ndk)
        .env_remove("ANDROID_NDK_ROOT")
        .env_remove("ANDROID_HOME")
        .env_remove("ANDROID_SDK_ROOT")
        .output()
        .expect("run android gate");
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(message.contains("not installed"), "{message}");
    assert!(message.contains("aarch64-linux-android"), "{message}");
    assert!(message.contains("rustup target add"), "{message}");
}
