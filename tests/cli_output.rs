//! CLI-level regressions for the output/workspace ownership contract.
//!
//! These tests execute the real `gba-native` binary in an owned temporary
//! working directory. They do not call the path helpers directly, so they
//! exercise argument parsing, command dispatch and the generated workspace.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-cli-{tag}-{}-{sequence}",
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

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn expect_failure(output: &Output, tag: &str) -> String {
    let message = stderr(output);
    assert!(
        !output.status.success(),
        "{tag} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{message}",
        stdout(output)
    );
    message
}

fn path_arg(path: &Path) -> String {
    path.to_str()
        .expect("temporary paths are UTF-8")
        .to_string()
}

fn generated_dir(output: &Output) -> PathBuf {
    stdout(output)
        .lines()
        .find_map(|line| line.strip_prefix("generated_dir="))
        .map(PathBuf::from)
        .expect("proof prints generated_dir")
}

#[cfg(unix)]
#[test]
fn default_proof_allocates_fresh_workspaces_and_ignores_legacy_links() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new("proof-default");
    let cwd = temp.path().join("cwd");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&outside).expect("outside");

    // Reproduce the round-two exploit shape at the legacy default path. The
    // fixed command must not touch it at all.
    let legacy = cwd.join("output").join("aot-proof");
    std::fs::create_dir_all(&legacy).expect("legacy workspace");
    let outside_manifest = outside.join("Cargo.toml");
    let outside_main = outside.join("main.rs");
    let outside_src = outside.join("src");
    std::fs::create_dir_all(&outside_src).expect("outside src");
    let outside_src_main = outside_src.join("main.rs");
    std::fs::write(&outside_manifest, b"KEEP-MANIFEST").expect("manifest sentinel");
    std::fs::write(&outside_main, b"KEEP-MAIN").expect("main sentinel");
    std::fs::write(&outside_src_main, b"KEEP-SRC-MAIN").expect("src main sentinel");
    symlink(&outside_manifest, legacy.join("Cargo.toml")).expect("manifest symlink");
    symlink(&outside_main, legacy.join("main.rs")).expect("main symlink");
    symlink(&outside_src, legacy.join("src")).expect("src symlink");
    let outside_target = outside.join("target");
    std::fs::create_dir_all(&outside_target).expect("outside target");
    symlink(&outside_target, legacy.join("target")).expect("target symlink");

    let first = cli(&cwd, &["aot", "proof"]);
    assert!(
        first.status.success(),
        "first default proof failed\nstdout:\n{}\nstderr:\n{}",
        stdout(&first),
        stderr(&first)
    );
    assert!(stdout(&first).contains("proof=ok"));
    assert!(stdout(&first).contains("native_vs_reference=match"));
    let first_dir = generated_dir(&first);
    let output_root = std::fs::canonicalize(cwd.join("output")).expect("output root");
    assert_eq!(first_dir.parent().expect("workspace parent"), output_root);
    assert_ne!(first_dir, legacy);
    assert!(first_dir.join("fixture.gba").is_file());
    assert!(first_dir.join("harness").join("Cargo.toml").is_file());
    assert!(
        first_dir
            .join("harness")
            .join("src")
            .join("main.rs")
            .is_file()
    );

    // Repeated default runs must allocate a new workspace and leave the first
    // one and every pre-existing file intact.
    let second = cli(&cwd, &["aot", "proof"]);
    assert!(
        second.status.success(),
        "second default proof failed\nstdout:\n{}\nstderr:\n{}",
        stdout(&second),
        stderr(&second)
    );
    let second_dir = generated_dir(&second);
    assert_ne!(first_dir, second_dir);
    assert!(first_dir.join("fixture.gba").is_file());
    assert!(second_dir.join("fixture.gba").is_file());

    assert_eq!(
        std::fs::read(&outside_manifest).expect("manifest survives"),
        b"KEEP-MANIFEST"
    );
    assert_eq!(
        std::fs::read(&outside_main).expect("main survives"),
        b"KEEP-MAIN"
    );
    assert_eq!(
        std::fs::read(&outside_src_main).expect("src main survives"),
        b"KEEP-SRC-MAIN"
    );
    assert_eq!(
        std::fs::read_link(legacy.join("Cargo.toml")).expect("legacy link"),
        outside_manifest
    );
}

#[cfg(unix)]
#[test]
fn explicit_existing_out_is_refused_without_following_links() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new("proof-existing");
    let cwd = temp.path().join("cwd");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(cwd.join("output").join("existing")).expect("existing out");
    std::fs::create_dir_all(outside.join("src")).expect("outside src");
    let outside_manifest = outside.join("Cargo.toml");
    let outside_main = outside.join("main.rs");
    let outside_src_main = outside.join("src").join("main.rs");
    std::fs::write(&outside_manifest, b"KEEP-MANIFEST").expect("manifest sentinel");
    std::fs::write(&outside_main, b"KEEP-MAIN").expect("main sentinel");
    std::fs::write(&outside_src_main, b"KEEP-SRC-MAIN").expect("src main sentinel");
    let existing = cwd.join("output").join("existing");
    symlink(&outside_manifest, existing.join("Cargo.toml")).expect("manifest symlink");
    symlink(&outside_main, existing.join("main.rs")).expect("main symlink");
    symlink(outside.join("src"), existing.join("src")).expect("src symlink");
    symlink(&outside, existing.join("target")).expect("target symlink");

    let output = cli(&cwd, &["aot", "proof", "--out", "output/existing"]);
    let message = expect_failure(&output, "proof with an existing --out directory");
    assert!(message.contains("existing directory"), "{message}");
    assert_eq!(
        std::fs::read(&outside_manifest).expect("manifest survives"),
        b"KEEP-MANIFEST"
    );
    assert_eq!(
        std::fs::read(&outside_main).expect("main survives"),
        b"KEEP-MAIN"
    );
    assert_eq!(
        std::fs::read(&outside_src_main).expect("src main survives"),
        b"KEEP-SRC-MAIN"
    );
}

#[test]
fn explicit_existing_file_out_is_not_reported_as_a_directory() {
    let temp = TempDir::new("proof-existing-file");
    let cwd = temp.path().join("cwd");
    let existing = cwd.join("output").join("existing-file");
    std::fs::create_dir_all(existing.parent().expect("output parent")).expect("output dir");
    std::fs::write(&existing, b"KEEP-FILE").expect("file sentinel");

    let output = cli(&cwd, &["aot", "proof", "--out", "output/existing-file"]);
    let message = expect_failure(&output, "proof with an existing --out file");
    assert!(message.contains("existing path"), "{message}");
    assert!(!message.contains("existing directory"), "{message}");
    assert_eq!(
        std::fs::read(existing).expect("file survives"),
        b"KEEP-FILE"
    );
}

#[test]
fn proof_out_outside_output_is_refused_before_creating_anything() {
    let temp = TempDir::new("proof-outside");
    let cwd = temp.path().join("cwd");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&outside).expect("outside");

    let relative = outside.join("fresh-proof");
    let output = cli(&cwd, &["aot", "proof", "--out", "../outside/fresh-proof"]);
    let message = expect_failure(&output, "proof with a relative outside --out");
    assert!(message.contains("outside"), "{message}");
    assert!(!relative.exists());
    assert!(
        !cwd.join("output").exists(),
        "rejection must not create output/"
    );

    let absolute = outside.join("absolute-proof");
    let absolute_arg = path_arg(&absolute);
    let output = cli(&cwd, &["aot", "proof", "--out", &absolute_arg]);
    let message = expect_failure(&output, "proof with an absolute outside --out");
    assert!(message.contains("outside"), "{message}");
    assert!(!absolute.exists());
    assert!(!cwd.join("output").exists());

    let escaped = cwd.join("escaped-proof");
    let output = cli(&cwd, &["aot", "proof", "--out", "output/../escaped-proof"]);
    expect_failure(&output, "proof with a traversal --out");
    assert!(!escaped.exists());
}

#[test]
fn demo_and_emit_restrict_generated_paths_to_output() {
    let temp = TempDir::new("generated-paths");
    let cwd = temp.path().join("cwd");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&outside).expect("outside");

    let outside_image = outside.join("demo.ppm");
    std::fs::write(&outside_image, b"KEEP-IMAGE").expect("image sentinel");
    let output = cli(&cwd, &["demo", "--out", "../outside/demo.ppm"]);
    let message = expect_failure(&output, "demo with an existing outside file");
    assert!(message.contains("outside"), "{message}");
    assert_eq!(
        std::fs::read(&outside_image).expect("image survives"),
        b"KEEP-IMAGE"
    );

    let fresh_image = outside.join("fresh.ppm");
    let output = cli(&cwd, &["demo", "--out", "../outside/fresh.ppm"]);
    expect_failure(&output, "demo with a fresh outside file");
    assert!(!fresh_image.exists());

    let absolute_image = outside.join("absolute.ppm");
    let absolute_arg = path_arg(&absolute_image);
    let output = cli(&cwd, &["demo", "--out", &absolute_arg]);
    expect_failure(&output, "demo with an absolute outside file");
    assert!(!absolute_image.exists());

    let escaped = cwd.join("escaped.ppm");
    let output = cli(&cwd, &["demo", "--out", "output/../escaped.ppm"]);
    expect_failure(&output, "demo with a traversal path");
    assert!(!escaped.exists());

    let inside = cwd.join("output").join("ok").join("demo.ppm");
    let output = cli(&cwd, &["demo", "--out", "output/ok/demo.ppm"]);
    assert!(
        output.status.success(),
        "allowed relative demo failed\nstderr:\n{}",
        stderr(&output)
    );
    assert!(inside.is_file());

    let absolute_inside = cwd.join("output").join("abs").join("demo.ppm");
    let absolute_inside_arg = path_arg(&absolute_inside);
    let output = cli(&cwd, &["demo", "--out", &absolute_inside_arg]);
    assert!(
        output.status.success(),
        "allowed absolute demo failed\nstderr:\n{}",
        stderr(&output)
    );
    assert!(absolute_inside.is_file());

    let rom = cwd.join("synthetic.gba");
    std::fs::write(&rom, gba_native::demo::demo_cartridge().bytes()).expect("synthetic ROM");
    let outside_source = outside.join("generated.rs");
    std::fs::write(&outside_source, b"KEEP-SOURCE").expect("source sentinel");
    let output = cli(
        &cwd,
        &["aot", "emit", "synthetic.gba", "../outside/generated.rs"],
    );
    let message = expect_failure(&output, "emit with an existing outside file");
    assert!(message.contains("outside"), "{message}");
    assert_eq!(
        std::fs::read(&outside_source).expect("source survives"),
        b"KEEP-SOURCE"
    );

    let fresh_source = outside.join("fresh.rs");
    let fresh_source_arg = path_arg(&fresh_source);
    let output = cli(&cwd, &["aot", "emit", "synthetic.gba", &fresh_source_arg]);
    expect_failure(&output, "emit with an absolute outside file");
    assert!(!fresh_source.exists());

    let output = cli(
        &cwd,
        &["aot", "emit", "synthetic.gba", "output/generated.rs"],
    );
    assert!(
        output.status.success(),
        "allowed emit failed\nstderr:\n{}",
        stderr(&output)
    );
    let generated = cwd.join("output").join("generated.rs");
    let source = std::fs::read_to_string(&generated).expect("generated source");
    assert!(source.contains("ROM_LEN"));
    assert!(!source.contains("ROM_BYTES"));
}

#[cfg(unix)]
#[test]
fn cli_refuses_symlinks_and_dangling_links_inside_output() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new("cli-symlinks");
    let cwd = temp.path().join("cwd");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(cwd.join("output")).expect("output");
    std::fs::create_dir_all(&outside).expect("outside");
    symlink(&outside, cwd.join("output").join("link")).expect("directory symlink");

    let output = cli(&cwd, &["demo", "--out", "output/link/nested/demo.ppm"]);
    let message = expect_failure(&output, "demo through a symlinked directory");
    assert!(message.contains("symbolic link"), "{message}");
    assert!(!outside.join("nested").exists());

    let output = cli(&cwd, &["aot", "proof", "--out", "output/link/proof"]);
    let message = expect_failure(&output, "proof through a symlinked directory");
    assert!(message.contains("symbolic link"), "{message}");
    assert!(!outside.join("proof").exists());

    let rom = cwd.join("synthetic.gba");
    std::fs::write(&rom, gba_native::demo::demo_cartridge().bytes()).expect("synthetic ROM");
    let output = cli(
        &cwd,
        &["aot", "emit", "synthetic.gba", "output/link/generated.rs"],
    );
    let message = expect_failure(&output, "emit through a symlinked directory");
    assert!(message.contains("symbolic link"), "{message}");
    assert!(!outside.join("generated.rs").exists());

    let dangling_target = outside.join("escape.ppm");
    symlink(&dangling_target, cwd.join("output").join("dangling")).expect("dangling symlink");
    let output = cli(&cwd, &["demo", "--out", "output/dangling"]);
    let message = expect_failure(&output, "demo through a dangling link");
    assert!(message.contains("symbolic link"), "{message}");
    assert!(!dangling_target.exists());
}

#[cfg(unix)]
#[test]
fn canonical_alias_resolving_inside_output_is_allowed() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new("output-alias");
    let cwd = temp.path().join("cwd");
    let destination = cwd.join("output").join("real");
    std::fs::create_dir_all(&destination).expect("output destination");
    symlink(&destination, cwd.join("output-alias")).expect("alias into output");

    let output = cli(&cwd, &["demo", "--out", "output-alias/demo.ppm"]);
    assert!(
        output.status.success(),
        "canonical in-output alias failed\nstderr:\n{}",
        stderr(&output)
    );
    assert!(destination.join("demo.ppm").is_file());
}

#[cfg(unix)]
#[test]
fn cli_atomically_replaces_hard_link_without_modifying_external_sentinel() {
    let temp = TempDir::new("hard-link");
    let cwd = temp.path().join("cwd");
    let output = cwd.join("output");
    let outside_file = temp.path().join("shared.ppm");
    let in_output = output.join("shared.ppm");
    std::fs::create_dir_all(&output).expect("output dir");
    std::fs::write(&outside_file, b"KEEP-OUTSIDE").expect("outside sentinel");
    std::fs::hard_link(&outside_file, &in_output).expect("hard link");

    let output = cli(&cwd, &["demo", "--out", "output/shared.ppm"]);
    assert!(
        output.status.success(),
        "demo failed to atomically replace the output file\nstderr:\n{}",
        stderr(&output)
    );
    assert_eq!(
        std::fs::read(&outside_file).expect("outside link survives"),
        b"KEEP-OUTSIDE"
    );
    let replaced = std::fs::read(in_output).expect("output file replaced");
    assert!(replaced.starts_with(b"P6\n"));
    assert_ne!(replaced, b"KEEP-OUTSIDE");
}

#[cfg(unix)]
#[test]
fn cli_rejects_fifo_without_blocking() {
    use std::time::{Duration, Instant};

    let temp = TempDir::new("fifo");
    let cwd = temp.path().join("cwd");
    let output_dir = cwd.join("output");
    std::fs::create_dir_all(&output_dir).expect("output dir");
    let fifo = output_dir.join("blocked.ppm");
    let created = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("run mkfifo");
    assert!(created.success(), "mkfifo failed for {}", fifo.display());

    let mut child = Command::new(env!("CARGO_BIN_EXE_gba-native"))
        .current_dir(&cwd)
        .args(["demo", "--out", "output/blocked.ppm"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn CLI");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().expect("poll CLI").is_some() {
            let output = child.wait_with_output().expect("collect CLI output");
            let message = stderr(&output);
            assert!(!output.status.success(), "FIFO replacement succeeded");
            assert!(message.contains("non-regular file"), "{message}");
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("CLI blocked while attempting to replace a FIFO");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
