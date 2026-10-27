//! CLI contract for the experimental `aot build` command: owned fresh output,
//! deterministic metadata, external ROM identity and failure paths that run no
//! target build.
//!
//! The success case uses the installed host triple, so it never attempts an
//! Apple cross-build on Linux.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::sha256;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-cli-build-{tag}-{}-{sequence}",
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

fn field(output: &Output, key: &str) -> String {
    stdout(output)
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("stdout is missing {key}"))
        .to_string()
}

fn parse_meta(text: &str) -> (Vec<String>, HashMap<String, String>) {
    let mut keys = Vec::new();
    let mut values = HashMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once('=').expect("metadata line is key=value");
        keys.push(key.to_string());
        assert!(
            values.insert(key.to_string(), value.to_string()).is_none(),
            "duplicate metadata key {key}"
        );
        assert!(
            !value.contains('/'),
            "metadata value must not be a path: {line}"
        );
    }
    (keys, values)
}

fn contains_extension(directory: &Path, extension: &str) -> bool {
    for entry in std::fs::read_dir(directory).expect("read generated directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            if contains_extension(&path, extension) {
                return true;
            }
        } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
            return true;
        }
    }
    false
}

#[test]
fn aot_build_writes_owned_artifact_and_deterministic_metadata() {
    let temp = TempDir::new("metadata");
    let cwd = temp.path().join("cwd");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let cart = gba_native::demo::demo_cartridge();
    let rom_bytes = cart.bytes().to_vec();
    let rom = cwd.join("synthetic.gba");
    std::fs::write(&rom, &rom_bytes).expect("write external ROM");
    let target = gba_native::aot::host_target_triple().expect("installed rustc host triple");

    let output = cli(
        &cwd,
        &["aot", "build", "synthetic.gba", "--target", &target],
    );
    assert!(
        output.status.success(),
        "aot build failed\nstdout:\n{}\nstderr:\n{}",
        stdout(&output),
        stderr(&output)
    );
    let text = stdout(&output);
    for expected in [
        "status=built".to_string(),
        format!("target={target}"),
        format!("rom_sha256={}", cart.sha256_hex()),
        format!("rom_bytes={}", cart.len()),
        "entry=0x08000000".to_string(),
        "entry_state=arm".to_string(),
        "budget=1000000".to_string(),
        "blocks=4".to_string(),
        "translated_instructions=17".to_string(),
        "embedded_rom_bytes=0".to_string(),
        "requires_external_rom=true".to_string(),
        "app=prototype-not-playable".to_string(),
    ] {
        assert!(text.contains(&expected), "stdout is missing {expected}");
    }
    assert!(
        text.contains("not a playable standalone game"),
        "the non-playability caveat is missing"
    );

    let work = PathBuf::from(field(&output, "generated_dir"));
    let output_root = std::fs::canonicalize(cwd.join("output")).expect("output root");
    assert_eq!(
        std::fs::canonicalize(&work)
            .expect("canonical workspace")
            .parent()
            .expect("workspace parent"),
        output_root
    );
    for path in [
        work.join("generated.rs"),
        work.join("artifact.meta"),
        work.join("build.meta"),
        work.join("native").join("Cargo.toml"),
        work.join("native").join("src").join("main.rs"),
    ] {
        assert!(path.is_file(), "missing generated file {}", path.display());
    }

    let executable = PathBuf::from(field(&output, "executable"));
    assert!(executable.is_file());
    assert_eq!(
        executable
            .parent()
            .expect("executable parent")
            .to_string_lossy()
            .replace('\\', "/"),
        work.join("native")
            .join("target")
            .join(&target)
            .join("release")
            .to_string_lossy()
            .replace('\\', "/")
    );
    assert!(
        executable
            .file_name()
            .expect("executable name")
            .to_string_lossy()
            .starts_with("gba-native-aot-harness")
    );

    let source = std::fs::read_to_string(work.join("generated.rs")).expect("generated source");
    assert!(source.contains("--rom"));
    assert!(source.contains("ROM_LEN"));
    assert!(!source.contains("ROM_BYTES"));
    assert!(!source.contains("GBA-NATIVE"));

    let artifact_meta = std::fs::read_to_string(work.join("artifact.meta")).expect("artifact meta");
    let expected_keys = [
        "schema",
        "target",
        "rom_sha256",
        "rom_bytes",
        "entry",
        "entry_state",
        "budget",
        "blocks",
        "translated_instructions",
        "resolved_bx",
        "dynamic_bx",
        "bl_calls",
        "bl_returns",
        "dynamic_bl",
        "pc_writes",
        "swis",
        "decode_stops",
        "external_targets",
        "block_limit_splits",
        "limit_reached",
        "source_sha256",
        "executable_sha256",
        "embedded_rom_bytes",
        "requires_external_rom",
    ];
    let (keys, fields) = parse_meta(&artifact_meta);
    assert_eq!(keys, expected_keys, "artifact metadata key order changed");
    assert_eq!(fields["schema"], "1");
    assert_eq!(fields["target"], target);
    assert_eq!(fields["rom_sha256"], cart.sha256_hex());
    assert_eq!(fields["rom_bytes"], cart.len().to_string());
    assert_eq!(fields["entry"], "0x08000000");
    assert_eq!(fields["entry_state"], "arm");
    assert_eq!(fields["budget"], "1000000");
    assert_eq!(fields["blocks"], "4");
    assert_eq!(fields["translated_instructions"], "17");
    assert_eq!(fields["embedded_rom_bytes"], "0");
    assert_eq!(fields["requires_external_rom"], "true");
    assert_eq!(
        fields["source_sha256"],
        sha256::hex(&sha256::digest(source.as_bytes())),
        "source hash must identify generated.rs"
    );
    assert_eq!(
        fields["executable_sha256"],
        sha256::hex(&sha256::digest(
            &std::fs::read(&executable).expect("read executable")
        )),
        "executable hash must identify the produced binary"
    );

    let build_meta = std::fs::read_to_string(work.join("build.meta")).expect("build meta");
    let expected_build_keys = [
        "schema",
        "target",
        "profile",
        "build_offline",
        "cargo_version",
        "rustc_version",
        "rustc_host",
    ];
    let (keys, fields) = parse_meta(&build_meta);
    assert_eq!(
        keys, expected_build_keys,
        "build metadata key order changed"
    );
    assert_eq!(fields["schema"], "1");
    assert_eq!(fields["target"], target);
    assert_eq!(fields["profile"], "release");
    assert_eq!(fields["build_offline"], "true");
    assert_ne!(fields["cargo_version"], "unknown");
    assert_ne!(fields["rustc_version"], "unknown");
    assert_eq!(fields["rustc_host"], target);
    assert!(!artifact_meta.contains(cwd.to_string_lossy().as_ref()));
    assert!(!build_meta.contains(cwd.to_string_lossy().as_ref()));

    // A second default run must allocate a different workspace and render the
    // same deterministic metadata. The executable hash is excluded because it
    // is a hash of build output, not of the metadata facts.
    let second = cli(
        &cwd,
        &["aot", "build", "synthetic.gba", "--target", &target],
    );
    assert!(
        second.status.success(),
        "second default aot build failed\nstdout:\n{}\nstderr:\n{}",
        stdout(&second),
        stderr(&second)
    );
    let second_work = PathBuf::from(field(&second, "generated_dir"));
    assert_ne!(
        work, second_work,
        "default workspaces must be unique per run"
    );
    assert!(work.join("artifact.meta").is_file());
    assert_eq!(
        build_meta,
        std::fs::read_to_string(second_work.join("build.meta")).expect("second build meta"),
        "build metadata must be deterministic"
    );
    let second_artifact_meta =
        std::fs::read_to_string(second_work.join("artifact.meta")).expect("second artifact meta");
    let stable_lines = |text: &str| {
        text.lines()
            .filter(|line| !line.starts_with("executable_sha256="))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        stable_lines(&artifact_meta),
        stable_lines(&second_artifact_meta),
        "artifact metadata facts must be deterministic"
    );

    // A fresh explicit --out must resolve below <cwd>/output/ and be accepted.
    let explicit = cli(
        &cwd,
        &[
            "aot",
            "build",
            "synthetic.gba",
            "--target",
            &target,
            "--out",
            "output/explicit-aot-build",
        ],
    );
    assert!(
        explicit.status.success(),
        "explicit fresh aot build failed\nstdout:\n{}\nstderr:\n{}",
        stdout(&explicit),
        stderr(&explicit)
    );
    let explicit_work = PathBuf::from(field(&explicit, "generated_dir"));
    assert_eq!(
        std::fs::canonicalize(&explicit_work).expect("canonical explicit workspace"),
        std::fs::canonicalize(cwd.join("output").join("explicit-aot-build"))
            .expect("canonical explicit path")
    );
    assert!(explicit_work.join("artifact.meta").is_file());
    assert!(explicit_work.join("build.meta").is_file());
    assert!(
        !contains_extension(&explicit_work, "gba"),
        "the explicit workspace must not contain a ROM copy"
    );

    // The original ROM stays external and unmodified, and no ROM copy is
    // written anywhere in the owned workspace.
    assert_eq!(std::fs::read(&rom).expect("ROM survives"), rom_bytes);
    assert!(
        !contains_extension(&work, "gba"),
        "the generated workspace must not contain a ROM copy"
    );
    assert!(
        !contains_extension(&second_work, "gba"),
        "the second workspace must not contain a ROM copy"
    );
}

#[test]
fn aot_build_failures_create_no_output_and_launch_no_build() {
    let temp = TempDir::new("failures");
    let cwd = temp.path().join("cwd");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&cwd).expect("cwd");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::write(
        cwd.join("synthetic.gba"),
        gba_native::demo::demo_cartridge().bytes(),
    )
    .expect("write external ROM");
    let target = gba_native::aot::host_target_triple().expect("installed rustc host triple");
    let output_root = cwd.join("output");

    let output = cli(&cwd, &["aot", "build", "synthetic.gba"]);
    let message = expect_failure(&output, "missing --target");
    assert!(message.contains("--target"), "{message}");
    assert!(!output_root.exists());

    let output = cli(&cwd, &["aot", "build", "synthetic.gba", "--target", ""]);
    let message = expect_failure(&output, "empty --target");
    assert!(message.contains("target"), "{message}");
    assert!(!output_root.exists());

    let output = cli(
        &cwd,
        &[
            "aot",
            "build",
            "synthetic.gba",
            "--target",
            "aarch64/apple-darwin",
        ],
    );
    let message = expect_failure(&output, "invalid --target");
    assert!(message.contains("target"), "{message}");
    assert!(!output_root.exists());

    let output = cli(&cwd, &["aot", "build", "missing.gba", "--target", &target]);
    expect_failure(&output, "missing ROM");
    assert!(!output_root.exists());

    let output = cli(
        &cwd,
        &[
            "aot",
            "build",
            "synthetic.gba",
            "--target",
            &target,
            "--budget",
            "nope",
        ],
    );
    let message = expect_failure(&output, "invalid budget");
    assert!(message.contains("invalid budget"), "{message}");
    assert!(!output_root.exists());

    let output = cli(
        &cwd,
        &[
            "aot",
            "build",
            "synthetic.gba",
            "--target",
            &target,
            "--bogus",
        ],
    );
    let message = expect_failure(&output, "unknown option");
    assert!(message.contains("unknown aot build option"), "{message}");
    assert!(!output_root.exists());

    let outside_out = outside.join("fresh-build");
    let output = cli(
        &cwd,
        &[
            "aot",
            "build",
            "synthetic.gba",
            "--target",
            &target,
            "--out",
            "../outside/fresh-build",
        ],
    );
    let message = expect_failure(&output, "outside --out");
    assert!(message.contains("outside"), "{message}");
    assert!(!outside_out.exists());
    assert!(!output_root.exists());

    let output = cli(
        &cwd,
        &[
            "aot",
            "build",
            "synthetic.gba",
            "--target",
            &target,
            "--out",
            "output/../escaped-build",
        ],
    );
    expect_failure(&output, "traversal --out");
    assert!(!cwd.join("escaped-build").exists());
    assert!(!output_root.exists());

    let output = cli(
        &cwd,
        &["aot", "build", "synthetic.gba", "--target", "wasm32-wasip1"],
    );
    let message = expect_failure(&output, "unsupported wasm target");
    assert!(message.contains("wasm32-wasip1"), "{message}");
    assert!(message.contains("wasm32-unknown-unknown"), "{message}");
    assert!(!output_root.exists());

    let existing = output_root.join("existing");
    std::fs::create_dir_all(&existing).expect("existing --out");
    let output = cli(
        &cwd,
        &[
            "aot",
            "build",
            "synthetic.gba",
            "--target",
            &target,
            "--out",
            "output/existing",
        ],
    );
    let message = expect_failure(&output, "existing --out");
    assert!(message.contains("existing directory"), "{message}");
    assert!(!existing.join("Cargo.toml").exists());
    assert!(!existing.join("native").exists());
}
