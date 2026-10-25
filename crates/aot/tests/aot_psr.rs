//! Native-AOT integration for ARM PSR transfers and banked registers.
//!
//! The fixture is entirely synthetic. It switches SYSTEM -> IRQ -> SYSTEM ->
//! IRQ -> SYSTEM through MSR, writes/reads SPSR_irq, reads banked SP/LR on both
//! sides and ends in a self-branch. Expected register values are hand-derived
//! from the instruction sequence, not from the shared decoder/interpreter.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, DISPCNT_ADDR};
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::interpret;
use gba_native::sha256;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-psr-aot-{tag}-{}-{sequence}",
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
const FINAL_PC: u32 = 0x0800_006C;
const BUDGET: u64 = 29;

fn ldr_literal(rd: u32, instruction_addr: u32, target: u32) -> u32 {
    let base = instruction_addr + 8;
    assert!(target >= base);
    let difference = target - base;
    assert_eq!(difference % 4, 0);
    assert!(difference < 4096);
    0xE59F_0000 | (rd << 12) | difference
}

fn mrs_cpsr(rd: u32) -> u32 {
    0xE10F_0000 | (rd << 12)
}

fn mrs_spsr(rd: u32) -> u32 {
    0xE14F_0000 | (rd << 12)
}

fn msr_cpsr_c(rm: u32) -> u32 {
    0xE121_F000 | rm
}

fn msr_spsr_f(rm: u32) -> u32 {
    0xE168_F000 | rm
}

fn psr_fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(0, 0x000, 0x070), // DISPCNT
        ldr_literal(1, 0x004, 0x074), // mode 3 + BG2
        0xE1C0_10B0,                  // STRH r1,[r0]
        ldr_literal(0, 0x00C, 0x078), // 0x12 (IRQ)
        msr_cpsr_c(0),                // switch IRQ
        ldr_literal(1, 0x014, 0x07C), // 0x03007000
        0xE1A0_D001,                  // MOV r13,r1
        ldr_literal(1, 0x01C, 0x080), // 0x12340000
        0xE1A0_E001,                  // MOV r14,r1
        ldr_literal(1, 0x024, 0x084), // 0x00000BAD
        0xE1A0_8001,                  // MOV r8,r1
        mrs_cpsr(2),                  // r2 = IRQ-mode CPSR
        ldr_literal(3, 0x030, 0x088), // 0xF0000000
        msr_spsr_f(3),                // SPSR_irq flags
        mrs_spsr(4),                  // r4 = SPSR_irq
        ldr_literal(0, 0x03C, 0x08C), // 0x1F (SYSTEM)
        msr_cpsr_c(0),                // switch SYSTEM
        0xE1A0_500D,                  // MOV r5,r13 (SYSTEM SP)
        0xE1A0_600E,                  // MOV r6,r14 (SYSTEM LR)
        0xE1A0_7008,                  // MOV r7,r8
        ldr_literal(0, 0x050, 0x090), // 0x12 (IRQ again)
        msr_cpsr_c(0),
        0xE1A0_900D,                  // MOV r9,r13 (IRQ SP)
        0xE1A0_A00E,                  // MOV r10,r14 (IRQ LR)
        mrs_spsr(11),                 // r11 = SPSR_irq again
        ldr_literal(0, 0x064, 0x094), // 0x1F (SYSTEM)
        msr_cpsr_c(0),
        0xEAFF_FFFE, // B .
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    let literals = [
        DISPCNT_ADDR,
        0x0000_0403,
        0x0000_0012,
        0x0300_7000,
        0x1234_0000,
        0x0000_0BAD,
        0xF000_0000,
        0x0000_001F,
        0x0000_0012,
        0x0000_001F,
    ];
    for (index, word) in literals.iter().enumerate() {
        let offset = 0x070 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA0 + 8].copy_from_slice(b"GBA-PSR\0");
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).expect("synthetic PSR fixture")
}

fn reference(cart: &Cartridge, budget: u64) -> (Cpu, Bus, u64) {
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(ENTRY);
    cpu.set_thumb(false);
    let mut bus = Bus::new(cart.clone());
    let mut used = 0;
    while used < budget {
        if let Err(error) = interpret::step(&mut cpu, &mut bus) {
            panic!("reference failed at pc=0x{:08X}: {error}", cpu.pc());
        }
        used += 1;
    }
    (cpu, bus, used)
}

#[test]
fn native_psr_banking_fixture_matches_hand_expected_state() {
    let cart = psr_fixture();
    let artifact =
        aot::discover(&cart, ENTRY, false, aot::Limits::default()).expect("discover PSR fixture");
    assert_eq!(artifact.stats.blocks, 2);
    assert_eq!(artifact.stats.instructions, 29);
    assert_eq!(artifact.stats.decode_stops, 0);
    assert_eq!(artifact.stats.dynamic_bx, 0);

    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit PSR source");
    let translated = source.split("fn main()").next().expect("block section");
    assert!(!translated.contains("interpret::step"));
    assert!(!translated.contains("decode_arm("));
    assert!(source.contains("cpu.write_psr(gba_native::ir::Psr::Cpsr"));
    assert!(source.contains("cpu.write_psr(gba_native::ir::Psr::Spsr"));
    assert!(source.contains("cpu.read_psr(gba_native::ir::Psr::Cpsr)?"));
    assert!(source.contains("cpu.read_psr(gba_native::ir::Psr::Spsr)?"));
    assert!(!source.contains("ROM_BYTES"));
    assert!(!source.contains("GBA-PSR"));

    let (cpu, bus, used) = reference(&cart, BUDGET);
    assert_eq!(used, BUDGET);
    assert_eq!(cpu.pc(), FINAL_PC);
    assert_eq!(cpu.cpsr(), 0x0000_001F);

    let temp = TempDir::new("fixture");
    let fixture = temp.path().join("fixture.gba");
    std::fs::write(&fixture, cart.bytes()).expect("write fixture");
    let harness =
        NativeHarness::build(&source, &temp.path().join("harness")).expect("build PSR harness");
    let report = harness.run(&fixture).expect("run PSR harness");

    assert_eq!(report.budget_used, BUDGET);
    assert_eq!(report.budget_remaining, 0);
    assert_eq!(report.fallbacks, 0);
    assert_eq!(report.pc, FINAL_PC);
    assert_eq!(report.cpsr, 0x0000_001F);
    assert!(!report.thumb);
    assert_eq!(
        report.regs,
        [
            0x0000_001F, // r0: final SYSTEM control value
            0x0000_0BAD, // r1
            0x0000_0012, // r2: IRQ-mode CPSR read while in IRQ
            0xF000_0000, // r3
            0xF000_0000, // r4: SPSR_irq read
            0x0300_7F00, // r5: SYSTEM SP after returning
            0x0000_0000, // r6: SYSTEM LR after returning
            0x0000_0BAD, // r7: shared r8 snapshot
            0x0000_0BAD, // r8: written in IRQ, shared with SYSTEM
            0x0300_7000, // r9: IRQ SP read on re-entry
            0x1234_0000, // r10: IRQ LR read on re-entry
            0xF000_0000, // r11: SPSR_irq persisted across the round trip
            0x0000_0000, // r12
            0x0300_7F00, // r13: SYSTEM bank restored
            0x0000_0000, // r14: SYSTEM bank restored
            FINAL_PC,
        ]
    );
    assert_eq!(report.pc, cpu.pc());
    assert_eq!(report.cpsr, cpu.cpsr());
    assert_eq!(report.cpu_sha256, sha256::hex(&cpu.state_digest()));
    assert!(report.matches_reference(&cpu, &bus, used).expect("compare"));

    let mut expected_bus = Bus::new(cart.clone());
    expected_bus
        .write16(DISPCNT_ADDR, 0x0403)
        .expect("expected DISPCNT write");
    assert_eq!(report.bus_sha256, sha256::hex(&expected_bus.state_digest()));
    assert_eq!(report.vram_sha256, sha256::hex(&expected_bus.vram_digest()));

    let no_rom = Command::new(harness.executable())
        .output()
        .expect("run without arguments");
    assert!(!no_rom.status.success());
}

#[test]
fn discovery_invalidates_banked_constants_on_cpsr_control_write() {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(8, 0x000, 0x040), // r8 = target (banked after a mode change)
        0xE121_F000,                  // MSR CPSR_c,r0
        0xE12F_FF18,                  // BX r8
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x10..0x14].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
    rom[0x40..0x44].copy_from_slice(&0x0800_0010u32.to_le_bytes());
    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA0 + 8].copy_from_slice(b"GBA-PSR\0");
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    let cart = Cartridge::from_bytes(rom).expect("synthetic invalidation fixture");

    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default())
        .expect("discover invalidation fixture");
    assert_eq!(artifact.stats.resolved_bx, 0);
    assert_eq!(artifact.stats.dynamic_bx, 1);
    assert!(
        artifact
            .blocks
            .iter()
            .all(|block| block.addr != 0x0800_0010)
    );
}

#[test]
fn discovery_keeps_bx_constants_across_flags_only_msr() {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(8, 0x000, 0x040), // r8 = target
        0xE128_F000,                  // MSR CPSR_f,r0
        0xE12F_FF18,                  // BX r8
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x10..0x14].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
    rom[0x40..0x44].copy_from_slice(&0x0800_0010u32.to_le_bytes());
    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA0 + 8].copy_from_slice(b"GBA-PSR\0");
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    let cart = Cartridge::from_bytes(rom).expect("synthetic flags fixture");

    let artifact =
        aot::discover(&cart, ENTRY, false, aot::Limits::default()).expect("discover flags fixture");
    assert_eq!(artifact.stats.resolved_bx, 1);
    assert!(
        artifact
            .blocks
            .iter()
            .any(|block| block.addr == 0x0800_0010)
    );
}
