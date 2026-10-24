//! Failure-path tests for the repository quality counter.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-quality-{tag}-{}-{sequence}",
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

fn write_lines(path: &Path, lines: usize) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    let mut text = String::new();
    for index in 0..lines {
        match index {
            0 => text.push_str("// a comment line counts\n"),
            1 => text.push('\n'),
            _ => text.push_str("fn counted() {}\n"),
        }
    }
    std::fs::write(path, text).expect("write fixture");
}

fn counter(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quality-counter"))
        .arg(arguments[0])
        .arg("--root")
        .arg(root)
        .args(&arguments[1..])
        .output()
        .expect("run quality counter")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn five_hundred_physical_lines_with_comments_and_blanks_pass() {
    let temp = TempDir::new("boundary");
    write_lines(&temp.path().join("src").join("exact.rs"), 500);
    let output = counter(temp.path(), &["oversized", "--max", "500", "--hard"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "0");
}

#[test]
fn five_hundred_one_physical_lines_fail_the_hard_gate() {
    let temp = TempDir::new("over");
    write_lines(&temp.path().join("src").join("over.rs"), 501);
    let output = counter(temp.path(), &["oversized", "--max", "500", "--hard"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), "1");
    assert!(stderr(&output).contains("over.rs has 501 physical lines"));
    assert!(stderr(&output).contains("1 file(s) exceed the hard limit"));
}

#[test]
fn a_trailing_newline_does_not_add_a_phantom_line() {
    let temp = TempDir::new("newline");
    let path = temp.path().join("exact.rs");
    let mut text = String::new();
    for _ in 0..500 {
        text.push_str("fn line() {}\n");
    }
    std::fs::write(&path, text).expect("write fixture");
    let output = counter(temp.path(), &["oversized", "--hard"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "0");
}

#[test]
fn a_nested_new_rust_file_is_part_of_the_population() {
    let temp = TempDir::new("nested");
    write_lines(&temp.path().join("deep").join("new.rs"), 501);
    let output = counter(temp.path(), &["oversized", "--max", "500", "--hard"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("deep/new.rs"));

    let population = counter(temp.path(), &["rust-files", "--format", "count"]);
    assert!(population.status.success());
    assert_eq!(stdout(&population), "1");
}

#[test]
fn generated_target_and_output_directories_are_not_maintained_sources() {
    let temp = TempDir::new("ignored");
    write_lines(&temp.path().join("build.rs"), 1);
    write_lines(&temp.path().join("src").join("kept.rs"), 1);
    write_lines(&temp.path().join("target").join("generated.rs"), 501);
    write_lines(&temp.path().join("output").join("emitted.rs"), 501);
    write_lines(&temp.path().join(".git").join("hook.rs"), 501);

    let population = counter(temp.path(), &["rust-files", "--format", "items"]);
    assert!(population.status.success());
    assert_eq!(stdout(&population), r#"["build.rs","src/kept.rs"]"#);

    let oversized = counter(temp.path(), &["oversized", "--hard"]);
    assert!(oversized.status.success(), "stderr: {}", stderr(&oversized));
}

#[test]
fn a_nested_output_directory_is_still_scanned() {
    let temp = TempDir::new("nested-output");
    write_lines(&temp.path().join("lib.rs"), 1);
    write_lines(
        &temp.path().join("src").join("output").join("oversized.rs"),
        501,
    );

    let oversized = counter(temp.path(), &["oversized", "--max", "500", "--hard"]);
    assert_eq!(oversized.status.code(), Some(1));
    assert_eq!(stdout(&oversized), "1");
    assert!(stderr(&oversized).contains("src/output/oversized.rs has 501 physical lines"));

    let population = counter(temp.path(), &["rust-files", "--format", "items"]);
    assert!(population.status.success());
    assert_eq!(
        stdout(&population),
        r#"["lib.rs","src/output/oversized.rs"]"#
    );
}

#[test]
fn a_nested_target_directory_is_still_scanned() {
    let temp = TempDir::new("nested-target");
    write_lines(&temp.path().join("lib.rs"), 1);
    write_lines(
        &temp
            .path()
            .join("tests")
            .join("target")
            .join("oversized.rs"),
        501,
    );

    let oversized = counter(temp.path(), &["oversized", "--max", "500", "--hard"]);
    assert_eq!(oversized.status.code(), Some(1));
    assert_eq!(stdout(&oversized), "1");
    assert!(stderr(&oversized).contains("tests/target/oversized.rs has 501 physical lines"));

    let population = counter(temp.path(), &["rust-files", "--format", "items"]);
    assert!(population.status.success());
    assert_eq!(
        stdout(&population),
        r#"["lib.rs","tests/target/oversized.rs"]"#
    );
}

#[test]
fn an_empty_selection_is_an_error_not_a_green() {
    let temp = TempDir::new("empty");
    let output = counter(temp.path(), &["oversized", "--hard"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(stderr(&output).contains("empty selection is never a pass"));
}

#[test]
fn an_unknown_command_and_a_bad_limit_are_usage_errors() {
    let temp = TempDir::new("usage");
    write_lines(&temp.path().join("src").join("a.rs"), 1);
    let unknown = counter(temp.path(), &["nonsense"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("unknown command"));
    let bad = counter(temp.path(), &["oversized", "--max", "five"]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(stderr(&bad).contains("invalid --max"));
}

#[cfg(unix)]
#[test]
fn an_unreadable_rust_file_fails_closed() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new("unreadable");
    let path = temp.path().join("src").join("locked.rs");
    write_lines(&path, 1);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    let output = counter(temp.path(), &["oversized"]);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("restore");
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("cannot read"));
}
