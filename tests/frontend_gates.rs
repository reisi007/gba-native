#![cfg(unix)]
//! Contracts of the optional `scripts/frontend.sh` gate.
//!
//! These tests use a fake `cargo`/`rustc` and a fake Ratchet path so the root
//! test suite never fetches or builds the GPU frontend dependencies. The real
//! Cargo commands are exercised by running `scripts/frontend.sh` itself; this
//! file pins the failure propagation, root resolution and target-directory
//! contract that a fake toolchain can prove without touching a GPU.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-frontend-gates-{tag}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .expect("canonical repository root")
}

fn executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write fake tool");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .expect("make fake tool executable");
}

/// Resolve a program from the parent process PATH *before* the test replaces the
/// child environment, so `bash` itself can be started by absolute path.
fn host_program(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").expect("host PATH is set");
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("host PATH has no {name}");
}

/// A minimal owned tool directory that contains only the shell and `dirname`
/// the gate needs for root resolution. In particular it contains no
/// `cargo`/`rustc`, so a distro-provided Cargo in `/usr/bin` or `/bin` cannot
/// accidentally satisfy the missing-tool path.
fn minimal_tools(temp: &TempDir) -> PathBuf {
    let bin = temp.path().join("minimal-bound-path");
    fs::create_dir_all(&bin).expect("create minimal bin");
    for tool in ["bash", "dirname"] {
        std::os::unix::fs::symlink(host_program(tool), bin.join(tool)).expect("link minimal tool");
    }
    bin
}

/// A cargo that records cwd, target directory and arguments, and can fail the
/// unit-test command on request. It otherwise succeeds for every command.
const FAKE_CARGO: &str = "\
#!/bin/sh
{
  printf 'cwd=%s\\n' \"$PWD\"
  printf 'target=%s\\n' \"${CARGO_TARGET_DIR:-<unset>}\"
  printf 'args=%s\\n' \"$*\"
} >> \"${FRONTEND_TEST_LOG:?FRONTEND_TEST_LOG must be set}\"
case \" $* \" in
  *\" test \"*)
    if [ \"${FRONTEND_TEST_FAIL_TESTS:-}\" = 1 ]; then
      printf 'simulated cargo test failure\\n' >&2
      exit 1
    fi
    ;;
esac
exit 0
";

fn fake_toolchain(temp: &TempDir) -> PathBuf {
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).expect("create fake bin");
    executable(&bin.join("cargo"), FAKE_CARGO);
    executable(&bin.join("rustc"), "#!/bin/sh\nexit 0\n");
    bin
}

fn interesting_path(bin: &Path) -> String {
    format!("{}:/usr/bin:/bin", bin.display())
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn log(path: &Path) -> String {
    fs::read_to_string(path).expect("read fake cargo log")
}

#[test]
fn frontend_gate_resolves_root_and_owns_its_target_from_any_cwd() {
    let temp = TempDir::new("success");
    let bin = fake_toolchain(&temp);
    let log_path = temp.path().join("cargo.log");
    let elsewhere = temp.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("create unrelated cwd");

    let output = Command::new(host_program("bash"))
        .arg(repo_root().join("scripts/frontend.sh"))
        .current_dir(&elsewhere)
        // An inherited target directory must never win: a plain cargo run in
        // apps/desktop would generate *.rs below apps/desktop/target, which the
        // root counter scans. Ratchet is intentionally absent.
        .env("PATH", interesting_path(&bin))
        .env("CARGO_TARGET_DIR", "apps/desktop/target")
        .env("FRONTEND_TEST_LOG", &log_path)
        .env_remove("RATCHET_BIN")
        .output()
        .expect("run frontend gate");

    assert!(output.status.success(), "{}", stderr(&output));
    let recorded = log(&log_path);
    assert!(recorded.contains("target=target/frontend"), "{recorded}");
    assert!(
        !recorded.contains("target=apps/desktop/target"),
        "{recorded}"
    );

    let root = repo_root();
    for line in recorded.lines().filter(|line| line.starts_with("cwd=")) {
        let cwd = Path::new(&line["cwd=".len()..])
            .canonicalize()
            .expect("canonical recorded cwd");
        assert_eq!(
            cwd, root,
            "gate must run from the repository root: {recorded}"
        );
    }
    for step in [
        "-- oversized",
        "args=fmt",
        "args=clippy",
        "args=test",
        "args=build",
        "args=run",
        "-- --help",
    ] {
        assert!(recorded.contains(step), "missing {step} in {recorded}");
    }
    assert!(stderr(&output).contains("optional frontend gate passed"));
}

#[test]
fn a_failing_frontend_test_command_stops_the_gate() {
    let temp = TempDir::new("test-failure");
    let bin = fake_toolchain(&temp);
    let log_path = temp.path().join("cargo.log");

    let output = Command::new(host_program("bash"))
        .arg(repo_root().join("scripts/frontend.sh"))
        .current_dir(repo_root())
        .env("PATH", interesting_path(&bin))
        .env("FRONTEND_TEST_LOG", &log_path)
        .env("FRONTEND_TEST_FAIL_TESTS", "1")
        .env_remove("RATCHET_BIN")
        .output()
        .expect("run frontend gate");

    assert!(!output.status.success(), "the gate must fail closed");
    let message = stderr(&output);
    assert!(message.contains("frontend cargo test"), "{message}");
    assert!(
        message.contains("simulated cargo test failure"),
        "{message}"
    );
    let recorded = log(&log_path);
    assert!(recorded.contains("args=test"), "{recorded}");
    assert!(
        !recorded.contains("args=build"),
        "build must not run after a failed test command: {recorded}"
    );
}

#[test]
fn frontend_gate_fails_closed_without_cargo() {
    let temp = TempDir::new("missing-cargo");
    let bin = minimal_tools(&temp);
    // A Cargo outside the minimal child PATH must still not satisfy the gate.
    let outside = temp.path().join("outside-path");
    fs::create_dir_all(&outside).expect("create outside tool dir");
    executable(&outside.join("cargo"), "#!/bin/sh\nexit 0\n");
    executable(&outside.join("rustc"), "#!/bin/sh\nexit 0\n");

    let mut tools: Vec<String> = fs::read_dir(&bin)
        .expect("read minimal bin")
        .map(|entry| {
            entry
                .expect("minimal bin entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    tools.sort();
    assert_eq!(
        tools,
        ["bash", "dirname"],
        "the minimal PATH must contain no Cargo/Rust wrapper"
    );

    let output = Command::new(host_program("bash"))
        .arg(repo_root().join("scripts/frontend.sh"))
        .current_dir(temp.path())
        .env("PATH", &bin)
        .env("HOME", temp.path())
        .env_remove("RATCHET_BIN")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("run frontend gate");

    assert_eq!(
        output.status.code(),
        Some(1),
        "the gate must exit 1 with its own diagnostic, not fail to start bash: {}",
        stderr(&output)
    );
    let message = stderr(&output);
    assert!(
        message.contains("cargo is not on PATH"),
        "expected the script's missing-Cargo diagnostic, got: {message}"
    );
    assert!(
        !message.contains("No such file or directory"),
        "the shell or dirname must already be resolvable: {message}"
    );
}

#[test]
fn frontend_gate_script_is_present_and_executable() {
    let path = repo_root().join("scripts/frontend.sh");
    let metadata = fs::metadata(&path).expect("frontend gate exists");
    assert!(metadata.is_file());
    assert_ne!(
        metadata.permissions().mode() & 0o111,
        0,
        "must be executable"
    );
}
