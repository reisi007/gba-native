//! Substantive contracts for the three-package workspace split.
//!
//! These tests exercise the actual Cargo metadata, lockfile and test inventory
//! in an owned, fresh target directory with an empty owned `CARGO_HOME`, so a
//! cached registry cannot accidentally satisfy a dependency. They also execute
//! the real bare CLI commands from an unrelated working directory. The optional
//! GPU frontend is checked statically here and exercised by `scripts/frontend.sh`;
//! it is never resolved by the default root graph.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::{aot, bios, bus, cart, cpu, decode, demo, display, error, interpret, ir, render};
use gba_native::{semantics, sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-workspace-{tag}-{}-{sequence}",
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

#[test]
fn root_facade_preserves_legacy_paths_and_error_identity() {
    // Every legacy module is reachable through the root facade, and core/AOT
    // error values are one identity rather than separate incompatible types.
    fn accepts_core_error(_: gba_native_core::Error) {}
    accepts_core_error(error::Error::Aot("facade".to_string()));
    accepts_core_error(error::Error::Native("facade".to_string()));

    let cartridge: cart::Cartridge = demo::demo_cartridge();
    let artifact = aot::discover(&cartridge, demo::ENTRY, false, aot::Limits::default())
        .expect("facade discovery");
    assert_eq!(artifact.stats.blocks, 4);
    assert_eq!(artifact.stats.instructions, 17);
    let source =
        aot::emit_native_module(&cartridge, &artifact, demo::DEMO_BUDGET).expect("facade emission");
    assert!(source.contains("gba_native::cpu::add_flags"));
    assert!(!source.contains("ROM_BYTES"));

    let mut machine = cpu::Cpu::direct_entry();
    machine.set_pc(demo::ENTRY);
    let mut memory = bus::Bus::new(cartridge);
    let used =
        interpret::run_budget(&mut machine, &mut memory, demo::DEMO_BUDGET).expect("reference run");
    assert_eq!(used, demo::DEMO_BUDGET);
    let image = display::snapshot(&memory).expect("facade image");
    assert_eq!(image.sha256_hex(), demo::expected_demo_image().sha256_hex());
    let scaled = render::scale(&image, 480, 320, render::Backend::Nearest).expect("facade scale");
    assert!(!render::to_ppm(&scaled).is_empty());

    let raw = 0xE1A0_0000;
    assert_eq!(
        decode::decode_arm(demo::ENTRY, raw)
            .expect("facade decode")
            .op,
        op_of(raw)
    );
    assert_eq!(semantics::aligned_pc(demo::ENTRY + 1, false), demo::ENTRY);
    let rebuilt = cart::Cartridge::from_bytes(demo::demo_cartridge().bytes().to_vec())
        .expect("facade cartridge");
    assert_eq!(sha256::hex(&rebuilt.sha256()), rebuilt.sha256_hex());
    let _ = bios::SERVICE_DIV;
    let _: ir::Psr = ir::Psr::Cpsr;
}

fn op_of(raw: u32) -> ir::Op {
    decode::decode_arm(0x0800_0000, raw).expect("decode").op
}

#[test]
fn offline_default_graph_inventory_and_bare_cli_contract() {
    let temp = TempDir::new("offline");
    let target = temp.path().join("target");
    let home = temp.path().join("cargo-home");
    std::fs::create_dir(&home).expect("empty owned CARGO_HOME");
    let root = repo_root();

    let metadata = cargo(
        &root,
        &target,
        &home,
        &[
            "metadata",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
        ],
        &root,
    );
    assert_success(&metadata, "cargo metadata");
    let metadata = String::from_utf8_lossy(&metadata.stdout);
    for package in ["gba-native", "gba-native-core", "gba-native-aot"] {
        assert!(
            metadata.contains(&format!("\"name\":\"{package}\"")),
            "metadata is missing package {package}"
        );
    }
    let default_start = metadata
        .find("\"workspace_default_members\":[")
        .expect("metadata has workspace_default_members");
    let defaults = &metadata[default_start..];
    let defaults = &defaults[..defaults.find(']').expect("default members array ends")];
    for member in ["/crates/core#", "/crates/aot#", "gba-native#0.1.0"] {
        assert!(
            defaults.contains(member),
            "default-members are missing {member}: {defaults}"
        );
    }
    assert!(
        !defaults.contains("apps/desktop"),
        "the default workspace must exclude apps/desktop"
    );

    let lock = std::fs::read_to_string(root.join("Cargo.lock")).expect("root lockfile");
    assert_eq!(
        lock.matches("[[package]]").count(),
        3,
        "root lock must contain exactly the three internal packages"
    );
    assert!(lock.contains("name = \"gba-native-core\""));
    assert!(lock.contains("name = \"gba-native-aot\""));
    assert!(
        !lock.contains("source = "),
        "no registry source in root lock"
    );
    assert!(
        !lock.contains("checksum = "),
        "no registry checksum in root lock"
    );

    let tree = cargo(
        &root,
        &target,
        &home,
        &["tree", "--offline", "--locked"],
        &root,
    );
    assert_success(&tree, "cargo tree");
    let tree = String::from_utf8_lossy(&tree.stdout);
    for package in ["gba-native-core", "gba-native-aot"] {
        assert!(tree.contains(package), "tree is missing {package}");
    }
    for frontend in ["pixels", "winit", "wgpu", "apps/desktop"] {
        assert!(
            !tree.contains(frontend),
            "default tree must not resolve {frontend}"
        );
    }

    let listed = cargo(
        &root,
        &target,
        &home,
        &["test", "--offline", "--locked", "--", "--list"],
        &root,
    );
    assert_success(&listed, "cargo test -- --list");
    let total = listed_count(&listed);
    assert!(
        total >= 200,
        "the workspace lost baseline tests: listed {total}, expected at least 200"
    );

    // Baseline inventory, labelled separately from this round's new contracts.
    // Each moved integration target and unit group keeps its exact old count.
    let core_manifest = root.join("crates/core/Cargo.toml");
    let aot_manifest = root.join("crates/aot/Cargo.toml");
    let root_manifest = root.join("Cargo.toml");
    let core_targets = [
        ("arm_alu", 6),
        ("arm_control", 9),
        ("arm_halfword", 5),
        ("arm_shift", 4),
        ("bios_arithmetic", 6),
        ("bios_dispatch", 2),
        ("bios_memory_copy", 13),
        ("bios_reset", 4),
        ("bus", 6),
        ("cartridge", 6),
        ("cpu", 12),
        ("demo", 3),
        ("display", 5),
        ("isa_golden", 4),
        ("psr_banking", 6),
        ("psr_field_mask", 4),
        ("psr_transfer", 10),
        ("thumb_alu", 7),
        ("thumb_control", 5),
    ];
    let aot_targets = [
        ("aot_alu", 2),
        ("aot_control", 4),
        ("aot_halfword", 1),
        ("aot_native", 5),
        ("aot_psr", 3),
        ("aot_thumb_alu", 1),
    ];
    let root_targets = [
        ("cli_output", 5),
        ("frontend_gates", 4),
        ("quality_counter", 10),
        ("scripts", 3),
    ];
    let mut baseline = 0;
    for (name, expected) in core_targets {
        baseline += assert_target_count(
            &root,
            &target,
            &home,
            &core_manifest,
            "--test",
            name,
            expected,
        );
    }
    for (name, expected) in aot_targets {
        baseline += assert_target_count(
            &root,
            &target,
            &home,
            &aot_manifest,
            "--test",
            name,
            expected,
        );
    }
    for (name, expected) in root_targets {
        baseline += assert_target_count(
            &root,
            &target,
            &home,
            &root_manifest,
            "--test",
            name,
            expected,
        );
    }
    baseline += assert_target_count(&root, &target, &home, &core_manifest, "--lib", "", 30);
    baseline += assert_target_count(&root, &target, &home, &aot_manifest, "--lib", "", 10);
    baseline += assert_target_count(
        &root,
        &target,
        &home,
        &root_manifest,
        "--bin",
        "gba-native",
        5,
    );
    assert_eq!(baseline, 200, "baseline test inventory changed");

    // Bare/default-run CLI from the repository root, then manifest invocations
    // from an unrelated working directory that owns all generated output.
    let help = cargo(
        &root,
        &target,
        &home,
        &["run", "--quiet", "--offline", "--locked", "--", "--help"],
        &root,
    );
    assert_success(&help, "bare default-run --help");
    assert!(String::from_utf8_lossy(&help.stdout).contains("gba-native demo"));

    let elsewhere = temp.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("unrelated cwd");
    let demo_run = cargo(
        &elsewhere,
        &target,
        &home,
        &[
            "run",
            "--quiet",
            "--offline",
            "--locked",
            "--manifest-path",
            root.join("Cargo.toml").to_str().expect("UTF-8 root"),
            "--",
            "demo",
            "--out",
            "output/demo.ppm",
        ],
        &elsewhere,
    );
    assert_success(&demo_run, "demo from unrelated cwd");
    let demo_text = String::from_utf8_lossy(&demo_run.stdout);
    assert!(
        demo_text.contains(
            "image_sha256=b02968d620ee66d7175c29bbde12bc9ff075290f3e8086b26e9d4829b9ccb154"
        )
    );
    assert!(elsewhere.join("output/demo.ppm").is_file());

    let proof = cargo(
        &elsewhere,
        &target,
        &home,
        &[
            "run",
            "--quiet",
            "--offline",
            "--locked",
            "--manifest-path",
            root.join("Cargo.toml").to_str().expect("UTF-8 root"),
            "--",
            "aot",
            "proof",
            "--out",
            "output/proof",
        ],
        &elsewhere,
    );
    assert_success(&proof, "aot proof from unrelated cwd");
    let proof_text = String::from_utf8_lossy(&proof.stdout);
    for expected in [
        "blocks=4",
        "translated_instructions=17",
        "budget_used=230411",
        "fallbacks=0",
        "native_vs_reference=match",
        "proof=ok",
    ] {
        assert!(proof_text.contains(expected), "proof is missing {expected}");
    }

    let counter = cargo(
        &elsewhere,
        &target,
        &home,
        &[
            "run",
            "--quiet",
            "--offline",
            "--locked",
            "--manifest-path",
            root.join("Cargo.toml").to_str().expect("UTF-8 root"),
            "--bin",
            "quality-counter",
            "--",
            "oversized",
            "--root",
            root.to_str().expect("UTF-8 root"),
            "--max",
            "500",
            "--hard",
        ],
        &elsewhere,
    );
    assert_success(&counter, "quality counter from unrelated cwd");
}

fn cargo(dir: &Path, target: &Path, home: &Path, args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO"))
        .current_dir(cwd)
        .args(args)
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_HOME", home)
        .output()
        .unwrap_or_else(|error| panic!("cannot run cargo in {}: {error}", dir.display()))
}

fn assert_success(output: &Output, tag: &str) {
    assert!(
        output.status.success(),
        "{tag} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn listed_count(output: &Output) -> usize {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.ends_with(": test"))
        .count()
}

fn assert_target_count(
    root: &Path,
    target: &Path,
    home: &Path,
    manifest: &Path,
    selector: &str,
    name: &str,
    expected: usize,
) -> usize {
    let mut args = vec![
        "test".to_string(),
        "--offline".to_string(),
        "--locked".to_string(),
        "--manifest-path".to_string(),
        manifest.to_str().expect("UTF-8 manifest").to_string(),
        selector.to_string(),
    ];
    if !name.is_empty() {
        args.push(name.to_string());
    }
    args.push("--".to_string());
    args.push("--list".to_string());
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = cargo(root, target, home, &borrowed, root);
    assert_success(&output, &format!("{selector} {name} --list"));
    let count = listed_count(&output);
    assert_eq!(
        count, expected,
        "test family {selector} {name} changed from {expected} to {count}"
    );
    count
}

#[test]
fn desktop_manifest_and_lock_are_core_only_static_contract() {
    let root = repo_root();
    let manifest =
        std::fs::read_to_string(root.join("apps/desktop/Cargo.toml")).expect("desktop manifest");
    assert!(
        manifest.contains(
            "gba-native = { package = \"gba-native-core\", path = \"../../crates/core\" }"
        )
    );
    assert!(!manifest.contains("gba-native-aot"));

    let lock = std::fs::read_to_string(root.join("apps/desktop/Cargo.lock")).expect("desktop lock");
    assert!(lock.contains("name = \"gba-native-core\""));
    assert!(!lock.contains("name = \"gba-native-aot\""));
    assert!(
        !lock.contains("name = \"gba-native\"\n"),
        "the frontend must not depend on the root compatibility facade"
    );
    assert!(lock.contains("name = \"pixels\"\nversion = \"0.17.2\""));
    assert!(lock.contains("name = \"winit\"\nversion = \"0.30.13\""));
    assert!(lock.contains("name = \"wgpu\"\nversion = \"29.0.4\""));
}
