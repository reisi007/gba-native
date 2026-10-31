//! Native-AOT integration for trace-seeded sealed work-RAM blocks.
//!
//! The fixture copies a two-instruction Thumb routine into IWRAM and branches
//! to it. A trace seed binds the observed instruction word; the generated
//! dispatcher re-reads that word and only runs the native variant when it still
//! matches. A deliberate mismatch must fall back to a fresh interpreter decode.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::Bus;
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::interpret;
use gba_native_aot::aot::{self, NativeHarness};

const BUDGET: u64 = 12;
const ENTRY: u32 = 0x0800_0100;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-trace-aot-{tag}-{}-{sequence}",
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

fn ldr_literal(rd: u32, instruction_addr: u32, target: u32) -> u32 {
    let base = instruction_addr + 8;
    assert!(target >= base);
    let difference = target - base;
    assert_eq!(difference % 4, 0);
    assert!(difference < 4096);
    0xE59F_0000 | (rd << 12) | difference
}

fn finish(mut rom: Vec<u8>) -> Cartridge {
    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA9].copy_from_slice(b"TRACE-AOT");
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).expect("synthetic trace fixture")
}

/// ARM boot code stores `code` at 0x03000000 and branches there in Thumb state.
fn fixture(code: u32) -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(0, 0x100, 0x120), // r0 = IWRAM
        ldr_literal(1, 0x104, 0x124), // r1 = Thumb code pair
        ldr_literal(2, 0x108, 0x128), // r2 = IWRAM|1
        0xE580_1000,                  // STR r1,[r0]
        0xE12F_FF12,                  // BX r2
        0xEAFF_FFFE,                  // unreachable B .
    ];
    for (index, word) in words.iter().enumerate() {
        rom[0x100 + index * 4..0x104 + index * 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x120..0x124].copy_from_slice(&0x0300_0000u32.to_le_bytes());
    rom[0x124..0x128].copy_from_slice(&code.to_le_bytes());
    rom[0x128..0x12C].copy_from_slice(&0x0300_0001u32.to_le_bytes());
    finish(rom)
}

fn reference(cart: &Cartridge) -> (Cpu, Bus) {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(ENTRY);
    cpu.set_thumb(false);
    let mut bus = Bus::new(cart.clone());
    interpret::run_budget(&mut cpu, &mut bus, BUDGET).expect("reference run");
    (cpu, bus)
}

fn seeds(cart: &Cartridge, code_raw: u16) -> Vec<aot::TraceSeed> {
    let mut seeds = Vec::new();
    for addr in [
        0x0800_0100u32,
        0x0800_0104,
        0x0800_0108,
        0x0800_010C,
        0x0800_0110,
    ] {
        let raw = cart.read_u32(addr).expect("ROM word");
        seeds.push(aot::TraceSeed {
            addr,
            thumb: false,
            raw,
        });
    }
    seeds.push(aot::TraceSeed {
        addr: 0x0300_0000,
        thumb: true,
        raw: u32::from(code_raw),
    });
    seeds.push(aot::TraceSeed {
        addr: 0x0300_0002,
        thumb: true,
        raw: 0xE7FE,
    });
    seeds
}

#[test]
fn trace_seeded_ram_block_runs_native_and_matches_reference() {
    let cart = fixture(0xE7FE_2007);
    let seeds = seeds(&cart, 0x2007);
    let artifact = aot::trace_artifact(&cart, ENTRY, false, &seeds).expect("trace artifact");
    assert_eq!(artifact.stats.blocks, 7);
    assert_eq!(artifact.stats.sealed_blocks, 2);

    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit source");
    assert!(source.contains("fn block_03000000_thumb_00002007"));
    assert!(source.contains("match raw"));
    assert!(source.contains("0x2007u16 =>"));
    assert!(!source.contains("ROM_BYTES"));

    let temp = TempDir::new("match");
    let rom_path = temp.path().join("fixture.gba");
    std::fs::write(&rom_path, cart.bytes()).expect("write fixture");
    let harness =
        NativeHarness::build(&source, &temp.path().join("harness")).expect("build harness");
    let report = harness.run(&rom_path).expect("run native harness");
    let (cpu, bus) = reference(&cart);
    assert_eq!(report.fallbacks, 0);
    assert_eq!(report.budget_used, BUDGET);
    assert!(
        report
            .matches_reference(&cpu, &bus, BUDGET)
            .expect("compare")
    );
    assert_eq!(report.regs[0], 7);

    let parsed = aot::parse_trace_seeds("ROM 08000100 0 E59F0018 1\nRAM 03000000 1 00002007 3\n")
        .expect("parse seeds");
    assert_eq!(parsed.len(), 2);
}

#[test]
fn guarded_ram_block_falls_back_when_live_word_changes() {
    // The trace observed 0x2007, but the guest stores 0x2009. The dispatcher
    // must not run the stale native variant; the interpreter executes 0x2009.
    let cart = fixture(0xE7FE_2009);
    let seeds = seeds(&cart, 0x2007);
    let artifact = aot::trace_artifact(&cart, ENTRY, false, &seeds).expect("trace artifact");
    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit source");
    let temp = TempDir::new("mismatch");
    let rom_path = temp.path().join("fixture.gba");
    std::fs::write(&rom_path, cart.bytes()).expect("write fixture");
    let harness =
        NativeHarness::build(&source, &temp.path().join("harness")).expect("build harness");
    let report = harness.run(&rom_path).expect("run native harness");
    let (cpu, bus) = reference(&cart);
    assert!(report.fallbacks >= 1, "stale guard must fall back");
    assert!(
        report
            .matches_reference(&cpu, &bus, BUDGET)
            .expect("compare")
    );
    assert_eq!(report.regs[0], 9);
}

#[test]
fn trace_stats_count_swi_and_sealed_blocks() {
    let cart = fixture(0xDF00);
    let seeds = vec![
        aot::TraceSeed {
            addr: 0x0300_0000,
            thumb: true,
            raw: 0xDF00,
        },
        aot::TraceSeed {
            addr: 0x0300_0002,
            thumb: true,
            raw: 0xE7FE,
        },
    ];
    let artifact = aot::trace_artifact(&cart, ENTRY, false, &seeds).expect("trace artifact");
    assert_eq!(artifact.stats.blocks, 2);
    assert_eq!(artifact.stats.sealed_blocks, 2);
    assert_eq!(artifact.stats.swis, 1);
}

#[test]
fn stale_rom_seed_is_rejected() {
    let cart = fixture(0xE7FE_2007);
    let mut seeds = seeds(&cart, 0x2007);
    seeds[0].raw ^= 1;
    let error = aot::trace_artifact(&cart, ENTRY, false, &seeds).expect_err("stale trace");
    assert!(error.to_string().contains("does not match the cartridge"));
}
