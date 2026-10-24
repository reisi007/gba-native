//! Substantive native-AOT integration test: emit Rust source from typed IR,
//! compile it with the installed Cargo/rustc, execute it, and compare registers,
//! flags, bus state, image and instruction budget against the IR reference.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::aot::{self, NativeHarness};
use gba_native::bus::{Bus, DISPCNT_ADDR};
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::demo;
use gba_native::display;
use gba_native::interpret;
use gba_native::sha256;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-aot-test-{tag}-{}-{sequence}",
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
    cpu.set_pc(demo::ENTRY);
    cpu.set_thumb(false);
    let mut bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut cpu, &mut bus, budget).expect("reference run");
    (cpu, bus, used)
}

#[test]
fn generated_native_demo_matches_reference_and_binds_rom() {
    let cart = demo::demo_cartridge();
    let artifact =
        aot::discover(&cart, demo::ENTRY, false, aot::Limits::default()).expect("discover demo");
    assert_eq!(artifact.stats.blocks, 4);
    assert_eq!(artifact.stats.instructions, 17);
    assert_eq!(artifact.stats.decode_stops, 0);
    assert!(artifact.stats.instructions < cart.len() / 4);

    let source =
        aot::emit_native_module(&cart, &artifact, demo::DEMO_BUDGET).expect("emit native source");
    assert!(source.contains("fn block_08000128_thumb"));
    assert!(source.contains("(0x08000128, true)"));
    assert!(source.contains("(0x08000100, false)"));
    assert!(source.contains("bus.write16(address & !1, cpu.reg(3) as u16)?;"));
    assert!(source.contains("gba_native::cpu::add_flags"));
    assert!(!source.contains("decode_arm("));
    assert!(!source.contains("gba_native::decode"));
    assert!(!source.contains("ROM_BYTES"));
    assert!(!source.contains("GBA-NATIVE"));
    assert!(source.contains("ROM_LEN"));
    assert!(source.contains("--rom"));

    let (cpu, bus, used) = reference(&cart, demo::DEMO_BUDGET);
    assert_eq!(used, demo::DEMO_BUDGET);
    let expected = demo::expected_demo_image();
    let reference_image = display::snapshot(&bus).expect("reference image");
    assert_eq!(reference_image, expected);

    let temp = TempDir::new("demo");
    let sentinel = temp.path().join("caller-owned.txt");
    std::fs::write(&sentinel, b"keep").expect("write caller sentinel");
    let bound_rom = temp.path().join("bound.gba");
    std::fs::write(&bound_rom, cart.bytes()).expect("write bound ROM");
    let harness =
        NativeHarness::build(&source, &temp.path().join("harness")).expect("build native harness");
    let no_rom = Command::new(harness.executable())
        .output()
        .expect("run harness without arguments");
    assert!(!no_rom.status.success());
    assert!(
        String::from_utf8_lossy(&no_rom.stderr).contains("usage: harness --rom PATH"),
        "harness must require an external ROM path"
    );
    let report = harness.run(&bound_rom).expect("run native harness");
    assert_eq!(report.budget_used, demo::DEMO_BUDGET);
    assert_eq!(report.budget_remaining, 0);
    assert_eq!(report.fallbacks, 0);
    assert!(report.matches_reference(&cpu, &bus, used).expect("compare"));
    assert_eq!(report.pc, 0x0800_0124);
    assert_eq!(report.cpsr, 0x6000_00DF);
    assert!(!report.thumb);
    assert_eq!(report.regs[2], 240 * 160);
    assert_eq!(report.image_width, 240);
    assert_eq!(report.image_height, 160);
    assert_eq!(report.image_sha256, expected.sha256_hex());
    assert_eq!(
        std::fs::read(&sentinel).expect("caller file survives"),
        b"keep",
        "harness build/run must not delete caller-owned files"
    );

    let wrong_rom = temp.path().join("wrong.gba");
    std::fs::write(&wrong_rom, demo::fallback_cartridge().bytes()).expect("write wrong ROM");
    let error = harness
        .run(&wrong_rom)
        .expect_err("wrong ROM must be rejected");
    assert!(
        format!("{error}").contains("ROM mismatch"),
        "unexpected error: {error}"
    );

    // Exact mid-block budget: 1 entry branch + 4 setup instructions. The native
    // block and the reference must stop at the same PC without overshooting.
    let mid_budget = 5;
    let mid_source =
        aot::emit_native_module(&cart, &artifact, mid_budget).expect("emit mid-budget source");
    let mid_dir = temp.path().join("mid-budget");
    let mid_harness = NativeHarness::build(&mid_source, &mid_dir).expect("build mid harness");
    let mid_rom = mid_dir.join("fixture.gba");
    std::fs::write(&mid_rom, cart.bytes()).expect("write mid fixture");
    let mid_report = mid_harness.run(&mid_rom).expect("run mid harness");
    let (mid_cpu, mid_bus, mid_used) = reference(&cart, mid_budget);
    assert_eq!(mid_used, mid_budget);
    assert_eq!(mid_cpu.pc(), 0x0800_0110);
    assert_eq!(mid_report.budget_used, mid_budget);
    assert_eq!(mid_report.pc, 0x0800_0110);
    assert!(
        mid_report
            .matches_reference(&mid_cpu, &mid_bus, mid_used)
            .expect("compare mid-budget state")
    );
}

fn ldr_literal(rd: u32, instruction_addr: u32, target: u32) -> u32 {
    let base = instruction_addr + 8;
    assert!(target >= base);
    let difference = target - base;
    assert_eq!(difference % 4, 0);
    assert!(difference < 4096);
    0xE59F_0000 | (rd << 12) | difference
}

/// Independently generated synthetic fixture whose immediates previously fell
/// into the extra-halfword/multiply classification. It also sets DISPCNT mode 3
/// through a documented STRH so the generated harness can snapshot an image.
fn immediate_fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        0xE3A0_00B0, // MOV r0,#0xB0
        0xE3A0_1009, // MOV r1,#9
        0xE280_201B, // ADD r2,r0,#0x1B
        0xE242_301B, // SUB r3,r2,#0x1B
        0xE290_400B, // ADDS r4,r0,#0x0B
        0xE253_500D, // SUBS r5,r3,#0x0D
        0xE350_00B0, // CMP r0,#0xB0
        ldr_literal(1, 0x1C, 0x40),
        ldr_literal(2, 0x20, 0x44),
        0xE1C1_20B0, // STRH r2,[r1]
        0xEAFF_FFFE, // B .
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x40..0x44].copy_from_slice(&DISPCNT_ADDR.to_le_bytes());
    rom[0x44..0x48].copy_from_slice(&0x0000_0403u32.to_le_bytes());

    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA0 + 8].copy_from_slice(b"GBA-IMM\0");
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).expect("synthetic immediate fixture")
}

#[test]
fn native_immediate_family_matches_isa_expected_state_and_only_explicit_store() {
    let cart = immediate_fixture();
    let artifact = aot::discover(&cart, 0x0800_0000, false, aot::Limits::default())
        .expect("discover immediate fixture");
    assert!(artifact.stats.instructions >= 11);
    assert_eq!(artifact.stats.decode_stops, 0);

    let budget = 11;
    let source = aot::emit_native_module(&cart, &artifact, budget).expect("emit immediate source");
    assert!(!source.contains("ROM_BYTES"));
    assert!(!source.contains("GBA-IMM"));

    let temp = TempDir::new("immediate");
    let fixture = temp.path().join("fixture.gba");
    std::fs::write(&fixture, cart.bytes()).expect("write immediate fixture");
    let harness = NativeHarness::build(&source, &temp.path().join("harness"))
        .expect("build immediate harness");
    let report = harness.run(&fixture).expect("run immediate harness");

    // Values and flags are derived from the encodings by hand, not by the
    // shared decoder/interpreter.
    assert_eq!(report.regs[0], 0xB0);
    assert_eq!(report.regs[1], DISPCNT_ADDR);
    assert_eq!(report.regs[2], 0x0403);
    assert_eq!(report.regs[3], 0xB0);
    assert_eq!(report.regs[4], 0xBB);
    assert_eq!(report.regs[5], 0xA3);
    assert_eq!(report.cpsr, 0x6000_00DF); // final CMP r0,#0xB0: Z=1,C=1
    assert_eq!(report.pc, 0x0800_0028);
    assert_eq!(report.budget_used, budget);
    assert_eq!(report.fallbacks, 0);

    let mut expected_bus = Bus::new(cart.clone());
    expected_bus
        .write16(DISPCNT_ADDR, 0x0403)
        .expect("expected DISPCNT write");
    assert_eq!(
        report.bus_sha256,
        sha256::hex(&expected_bus.state_digest()),
        "only the explicit DISPCNT store may change bus state"
    );
    assert_eq!(report.vram_sha256, sha256::hex(&expected_bus.vram_digest()));
}

#[test]
fn native_runtime_uses_fresh_reference_fallback_for_dynamic_target() {
    let cart = demo::fallback_cartridge();
    let artifact = aot::discover(&cart, demo::ENTRY, false, aot::Limits::default())
        .expect("discover fallback fixture");
    assert_eq!(artifact.stats.blocks, 5);
    assert_eq!(artifact.stats.instructions, 12);
    assert!(artifact.stats.dynamic_bx >= 1);
    assert!(
        artifact
            .blocks
            .iter()
            .all(|block| block.addr != demo::FALLBACK_TARGET)
    );

    let source = aot::emit_native_module(&cart, &artifact, demo::FALLBACK_BUDGET)
        .expect("emit fallback source");
    assert!(!source.contains("block_08000128_arm"));
    assert!(source.contains("interpret::step(&mut cpu, &mut bus)?;"));

    let (cpu, bus, used) = reference(&cart, demo::FALLBACK_BUDGET);
    assert_eq!(used, demo::FALLBACK_BUDGET);
    assert_eq!(cpu.pc(), 0x0800_0134);

    let temp = TempDir::new("fallback");
    let fixture = temp.path().join("fixture.gba");
    std::fs::write(&fixture, cart.bytes()).expect("write fallback fixture");
    let harness = NativeHarness::build(&source, &temp.path().join("harness"))
        .expect("build fallback harness");
    let report = harness.run(&fixture).expect("run fallback harness");
    assert_eq!(report.budget_used, demo::FALLBACK_BUDGET);
    assert_eq!(report.fallbacks, 2);
    assert!(report.matches_reference(&cpu, &bus, used).expect("compare"));
    assert_eq!(report.pc, 0x0800_0134);
    assert_eq!(report.regs[2], 1);
    assert_eq!(report.regs[4], 0x55);
}

#[test]
fn harness_refuses_preexisting_directory_instead_of_reusing_it() {
    let temp = TempDir::new("harness-contract");
    let existing = temp.path().join("existing");
    std::fs::create_dir(&existing).expect("existing dir");
    let sentinel = existing.join("caller.txt");
    std::fs::write(&sentinel, b"keep").expect("sentinel");

    let error = NativeHarness::build("fn main() {}", &existing)
        .err()
        .expect("must refuse");
    assert!(format!("{error}").contains("already exists"), "{error}");
    assert_eq!(
        std::fs::read(&sentinel).expect("sentinel survives"),
        b"keep"
    );
}

#[cfg(unix)]
#[test]
fn harness_refuses_preexisting_symlinked_crate_children() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new("harness-link");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(outside.join("src")).expect("outside dirs");
    let outside_main = outside.join("src").join("main.rs");
    std::fs::write(&outside_main, b"keep").expect("outside sentinel");

    let existing = temp.path().join("existing");
    std::fs::create_dir(&existing).expect("existing dir");
    symlink(outside.join("src"), existing.join("src")).expect("src symlink");

    let error = NativeHarness::build("fn main() {}", &existing)
        .err()
        .expect("must refuse");
    assert!(format!("{error}").contains("already exists"), "{error}");
    assert_eq!(std::fs::read(&outside_main).expect("outside main"), b"keep");
    assert!(!outside.join("Cargo.toml").exists());
}
