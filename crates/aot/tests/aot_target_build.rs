//! Target-profile native build integration: compile the generated harness in
//! release mode for the installed host triple, execute it with an external ROM,
//! and check the existing digest/length binding. This is host-toolchain
//! evidence, not a cross-compilation claim.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::Bus;
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::demo;
use gba_native::interpret;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-aot-target-{tag}-{}-{sequence}",
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

fn reference(cart: &Cartridge, budget: u64) -> (Cpu, Bus, u64) {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(demo::ENTRY);
    cpu.set_thumb(false);
    let mut bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut cpu, &mut bus, budget).expect("reference run");
    (cpu, bus, used)
}

#[test]
fn target_profile_build_runs_synthetic_rom_and_preserves_digest_binding() {
    let target = aot::host_target_triple().expect("installed rustc host triple");
    let cart = demo::demo_cartridge();
    let artifact =
        aot::discover(&cart, demo::ENTRY, false, aot::Limits::default()).expect("discover demo");
    let source =
        aot::emit_native_module(&cart, &artifact, demo::DEMO_BUDGET).expect("emit native source");
    assert!(!source.contains("ROM_BYTES"));
    assert!(!source.contains("GBA-NATIVE"));
    assert!(source.contains("--rom"));

    let temp = TempDir::new("run");
    let harness = NativeHarness::build_for_target(&source, &temp.path().join("harness"), &target)
        .expect("build target-profile harness");
    assert!(
        harness.executable().is_file(),
        "target executable is missing: {}",
        harness.executable().display()
    );
    let components = format!("/{target}/release/");
    assert!(
        harness
            .executable()
            .to_string_lossy()
            .replace('\\', "/")
            .contains(&components),
        "executable path is not in the target release directory: {}",
        harness.executable().display()
    );

    let no_rom = Command::new(harness.executable())
        .output()
        .expect("run target harness without arguments");
    assert!(!no_rom.status.success());
    assert!(
        String::from_utf8_lossy(&no_rom.stderr).contains("usage: harness --rom PATH"),
        "target executable must still require an external ROM path"
    );

    let bound_rom = temp.path().join("bound.gba");
    std::fs::write(&bound_rom, cart.bytes()).expect("write bound ROM");
    let (cpu, bus, used) = reference(&cart, demo::DEMO_BUDGET);
    let report = harness.run(&bound_rom).expect("run target harness");
    assert_eq!(report.budget_used, demo::DEMO_BUDGET);
    assert_eq!(report.fallbacks, 0);
    assert!(report.matches_reference(&cpu, &bus, used).expect("compare"));

    let wrong_rom = temp.path().join("wrong.gba");
    std::fs::write(&wrong_rom, demo::fallback_cartridge().bytes()).expect("write wrong ROM");
    let error = harness
        .run(&wrong_rom)
        .expect_err("wrong ROM must be rejected");
    assert!(
        format!("{error}").contains("ROM mismatch"),
        "unexpected error: {error}"
    );
}

#[test]
fn target_validation_keeps_valid_short_triples() {
    for target in ["wasm32-wasip1", "aarch64-apple-darwin"] {
        assert!(
            aot::validate_target(target).is_ok(),
            "{target} must remain a valid target triple"
        );
    }
}

#[test]
fn target_build_rejects_invalid_targets_before_creating_the_crate() {
    let temp = TempDir::new("invalid-target");
    let directory = temp.path().join("harness");
    for target in [
        "",
        " ",
        " aarch64-apple-darwin",
        "aarch64/apple-darwin",
        "-target",
        "target.json",
        "aarch64-apple-darwin.json",
        "aarch64--darwin",
    ] {
        let error = NativeHarness::build_for_target("fn main() {}", &directory, target)
            .err()
            .expect("invalid target must be refused");
        assert!(
            format!("{error}").contains("target"),
            "unexpected error for {target:?}: {error}"
        );
        assert!(
            !directory.exists(),
            "invalid target {target:?} created a crate directory"
        );
    }
}

#[test]
fn target_build_refuses_a_preexisting_directory() {
    let target = aot::host_target_triple().expect("installed rustc host triple");
    let temp = TempDir::new("existing");
    let existing = temp.path().join("existing");
    std::fs::create_dir(&existing).expect("existing directory");
    let sentinel = existing.join("caller.txt");
    std::fs::write(&sentinel, b"keep").expect("sentinel");

    let error = NativeHarness::build_for_target("fn main() {}", &existing, &target)
        .err()
        .expect("existing directory must be refused");
    assert!(format!("{error}").contains("already exists"), "{error}");
    assert_eq!(
        std::fs::read(&sentinel).expect("sentinel survives"),
        b"keep"
    );
    assert!(!existing.join("Cargo.toml").exists());
}
