//! Native-AOT integration for a negative-offset ARM `STRH`.
//!
//! The fixture is entirely synthetic. It loads an EWRAM base and a stored value
//! through literal `LDR`, performs `STRH r1,[r0,#-0x20]`, initialises DISPCNT,
//! reads the stored halfword back with `LDRH r4,[r0,#-0x20]` and ends in a
//! self-branch. Expected registers and the expected bus effect are hand-derived
//! from that sequence, independent of the shared decoder/interpreter.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::aot::{self, NativeHarness};
use gba_native::bus::{Bus, DISPCNT_ADDR, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::{Cpu, USER_STACK};
use gba_native::interpret;
use gba_native::sha256;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-halfword-aot-{tag}-{}-{sequence}",
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

const ENTRY: u32 = 0x0800_0000;
const FINAL_PC: u32 = 0x0800_001C;
const BUDGET: u64 = 8;
const RAM_BASE: u32 = EWRAM_BASE + 0x100;
const TARGET: u32 = RAM_BASE - 0x20;

fn ldr_literal(rd: u32, instruction_addr: u32, target: u32) -> u32 {
    let base = instruction_addr + 8;
    assert!(target >= base);
    let difference = target - base;
    assert_eq!(difference % 4, 0);
    assert!(difference < 4096);
    0xE59F_0000 | (rd << 12) | difference
}

fn fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(0, 0x00, 0x40), // r0 = EWRAM base 0x02000100
        ldr_literal(1, 0x04, 0x44), // r1 = 0x0000BEEF
        0xE140_12B0,                // STRH r1,[r0,#-0x20]
        ldr_literal(2, 0x0C, 0x48), // r2 = DISPCNT
        ldr_literal(3, 0x10, 0x4C), // r3 = mode 3 + BG2
        0xE1C2_30B0,                // STRH r3,[r2]
        0xE150_42B0,                // LDRH r4,[r0,#-0x20]
        0xEAFF_FFFE,                // B .
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    let literals = [RAM_BASE, 0x0000_BEEF, DISPCNT_ADDR, 0x0000_0403];
    for (index, word) in literals.iter().enumerate() {
        let offset = 0x40 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA8].copy_from_slice(b"GBA-HWD\0");
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).expect("synthetic halfword fixture")
}

fn reference(cart: &Cartridge) -> (Cpu, Bus) {
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(ENTRY);
    cpu.set_thumb(false);
    let mut bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut cpu, &mut bus, BUDGET).expect("reference execution");
    assert_eq!(used, BUDGET);
    (cpu, bus)
}

#[test]
fn negative_strh_is_translated_directly_and_matches_hand_expected_bus() {
    let cart = fixture();
    let artifact =
        aot::discover(&cart, ENTRY, false, aot::Limits::default()).expect("discover fixture");
    // The straight-line entry block plus the self-branch continuation, which
    // re-queues the terminal B as its own block (8 unique + 1 translated B).
    assert_eq!(artifact.stats.blocks, 2);
    assert_eq!(artifact.stats.instructions, 9);
    assert_eq!(artifact.stats.decode_stops, 0);
    assert_eq!(artifact.stats.dynamic_bx, 0);

    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit fixture");
    let translated = source.split("fn main()").next().expect("block section");
    assert!(!translated.contains("interpret::step"));
    assert!(!translated.contains("decode_arm("));
    assert!(source.contains("// 0x08000008 arm 0xE14012B0"));
    assert!(source.contains("wrapping_add(0xFFFFFFE0u32)"));
    assert!(source.contains("bus.write16(address & !1, cpu.reg(1) as u16)?;"));
    assert!(!source.contains("ROM_BYTES"));
    assert!(!source.contains("GBA-HWD"));

    let (cpu, bus) = reference(&cart);
    assert_eq!(cpu.pc(), FINAL_PC);
    assert_eq!(cpu.cpsr(), 0x0000_00DF);
    assert_eq!(bus.read16(TARGET), Ok(0xBEEF));
    assert_eq!(
        bus.read16(RAM_BASE + 0x20),
        Ok(0),
        "positive alias unwritten"
    );
    assert_eq!(bus.dispcnt(), 0x0403);

    let temp = TempDir::new("fixture");
    let fixture_path = temp.path().join("fixture.gba");
    std::fs::write(&fixture_path, cart.bytes()).expect("write fixture");
    let harness = NativeHarness::build(&source, &temp.path().join("harness"))
        .expect("build halfword harness");
    let report = harness.run(&fixture_path).expect("run halfword harness");

    assert_eq!(report.budget_used, BUDGET);
    assert_eq!(report.budget_remaining, 0);
    assert_eq!(report.fallbacks, 0, "negative STRH must not fall back");
    assert_eq!(report.pc, FINAL_PC);
    assert_eq!(report.cpsr, 0x0000_00DF);
    assert!(!report.thumb);
    assert_eq!(
        report.regs,
        [
            RAM_BASE,     // r0: EWRAM base from literal LDR
            0x0000_BEEF,  // r1: stored value
            DISPCNT_ADDR, // r2
            0x0000_0403,  // r3: DISPCNT mode 3 + BG2
            0x0000_BEEF,  // r4: LDRH readback from the negative offset
            0,            // r5
            0,            // r6
            0,            // r7
            0,            // r8
            0,            // r9
            0,            // r10
            0,            // r11
            0,            // r12
            USER_STACK,   // r13
            0,            // r14
            FINAL_PC,
        ]
    );

    assert_eq!(report.pc, cpu.pc());
    assert_eq!(report.cpsr, cpu.cpsr());
    assert_eq!(report.cpu_sha256, sha256::hex(&cpu.state_digest()));
    assert!(
        report
            .matches_reference(&cpu, &bus, BUDGET)
            .expect("compare native and reference")
    );

    // Independently constructed expected bus: exactly the negative STRH target
    // and the explicit DISPCNT store, nothing at the positive-offset alias.
    let mut expected_bus = Bus::new(cart.clone());
    expected_bus
        .write16(TARGET, 0xBEEF)
        .expect("expected halfword store");
    expected_bus
        .write16(DISPCNT_ADDR, 0x0403)
        .expect("expected DISPCNT store");
    assert_eq!(expected_bus.read16(RAM_BASE + 0x20), Ok(0));
    assert_eq!(report.bus_sha256, sha256::hex(&expected_bus.state_digest()));
    assert_eq!(report.vram_sha256, sha256::hex(&expected_bus.vram_digest()));
    assert_eq!(
        bus.state_digest(),
        expected_bus.state_digest(),
        "reference bus matches the independent expected bus"
    );

    let no_rom = Command::new(harness.executable())
        .output()
        .expect("run without arguments");
    assert!(!no_rom.status.success());
}
