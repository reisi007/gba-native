//! Native-AOT parity for unaligned ARM/Thumb block and stack bases.
//!
//! Design policy: low base bits are ignored for the transfer address
//! (`base & !3`), while writeback is derived from the original base. Both
//! fixtures use synthetic ROMs and hand-derived expectations; the generated
//! harness must match the reference interpreter exactly.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::interpret;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-unaligned-block-aot-{tag}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("create unique temp dir");
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

fn finish(mut rom: Vec<u8>, title: &[u8]) -> Cartridge {
    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA0 + title.len()].copy_from_slice(title);
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).expect("synthetic unaligned-block fixture")
}

/// ARM: STMIA r0!,{r1,r2} with r0 = EWRAM+0x102 writes 0x100/0x104 and leaves
/// r0 = 0x10A. LDMDA r0!,{r3,r4} then reads from 0x104/0x108 and leaves
/// r0 = 0x102 (original 0x10A minus 8).
fn arm_fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(0, 0x00, 0x40), // r0 = EWRAM+0x102
        ldr_literal(1, 0x04, 0x44), // r1 = 0x11
        ldr_literal(2, 0x08, 0x48), // r2 = 0x22
        0xE8A0_0006,                // STMIA r0!,{r1,r2}
        0xE3A0_1000,                // MOV r1,#0
        0xE3A0_2000,                // MOV r2,#0
        0xE830_0018,                // LDMDA r0!,{r3,r4}
        0xEAFF_FFFE,                // B .
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    for (index, word) in [EWRAM_BASE + 0x102, 0x11, 0x22].iter().enumerate() {
        let offset = 0x40 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    finish(rom, b"GBA-UBK\0")
}

/// Thumb: two independently unaligned bases. STMIA r0!,{r1,r2} writes
/// 0x100/0x104 and leaves r0 = 0x10A; LDMIA r5!,{r3,r4} with r5 =
/// EWRAM+0x106 reads 0x104/0x108 and leaves r5 = 0x10E.
fn thumb_fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let halfwords: [u16; 7] = [
        0x480F, // LDR r0,[pc,#0x3C] -> literal at 0x40
        0x4D10, // LDR r5,[pc,#0x40] -> literal at 0x44
        0x2111, // MOV r1,#0x11
        0x2222, // MOV r2,#0x22
        0xC006, // STMIA r0!,{r1,r2}
        0xCD18, // LDMIA r5!,{r3,r4}
        0xE7FE, // B .
    ];
    for (index, halfword) in halfwords.iter().enumerate() {
        rom[index * 2..index * 2 + 2].copy_from_slice(&halfword.to_le_bytes());
    }
    for (index, word) in [EWRAM_BASE + 0x102, EWRAM_BASE + 0x106].iter().enumerate() {
        let offset = 0x40 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    finish(rom, b"GBA-UBT\0")
}

fn build_and_run(source: &str, cart: &Cartridge, tag: &str) -> aot::NativeReport {
    let temp = TempDir::new(tag);
    let path = temp.path().join("fixture.gba");
    std::fs::write(&path, cart.bytes()).expect("write fixture");
    let harness = NativeHarness::build(source, &temp.path().join("harness"))
        .expect("build generated harness");
    harness.run(&path).expect("run generated harness")
}

fn reference(cart: &Cartridge, thumb: bool, budget: u64) -> (Cpu, Bus) {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(0x0800_0000);
    cpu.set_thumb(thumb);
    let mut bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut cpu, &mut bus, budget).expect("reference run");
    assert_eq!(used, budget);
    (cpu, bus)
}

#[test]
fn native_arm_unaligned_block_base_matches_the_reference() {
    let cart = arm_fixture();
    let artifact = aot::discover(&cart, 0x0800_0000, false, aot::Limits::default())
        .expect("discover ARM unaligned fixture");
    let budget = 7;
    let source = aot::emit_native_module(&cart, &artifact, budget).expect("emit ARM unaligned");
    assert!(source.contains("aligned_block_base(base)"));
    assert!(!source.contains("require_aligned_block_base"));

    let (expected_cpu, expected_bus) = reference(&cart, false, budget);
    assert_eq!(expected_cpu.pc(), 0x0800_001C);
    assert_eq!(expected_cpu.reg(0), EWRAM_BASE + 0x102);
    assert_eq!(expected_cpu.reg(3), 0x22);
    assert_eq!(expected_cpu.reg(4), 0);
    assert_eq!(expected_bus.read32(EWRAM_BASE + 0x100), Ok(0x11));
    assert_eq!(expected_bus.read32(EWRAM_BASE + 0x104), Ok(0x22));

    let report = build_and_run(&source, &cart, "arm");
    assert_eq!(report.budget_used, budget);
    assert_eq!(report.fallbacks, 0);
    assert_eq!(report.pc, 0x0800_001C);
    assert_eq!(report.regs[0], EWRAM_BASE + 0x102);
    assert_eq!(report.regs[3], 0x22);
    assert!(
        report
            .matches_reference(&expected_cpu, &expected_bus, budget)
            .expect("compare native/reference")
    );
}

#[test]
fn native_thumb_unaligned_block_base_matches_the_reference() {
    let cart = thumb_fixture();
    let artifact = aot::discover(&cart, 0x0800_0000, true, aot::Limits::default())
        .expect("discover Thumb unaligned fixture");
    let budget = 6;
    let source = aot::emit_native_module(&cart, &artifact, budget).expect("emit Thumb unaligned");
    assert!(source.contains("aligned_block_base(base)"));
    assert!(!source.contains("require_aligned_block_base"));

    let (expected_cpu, expected_bus) = reference(&cart, true, budget);
    assert_eq!(expected_cpu.pc(), 0x0800_000C);
    assert!(expected_cpu.thumb());
    assert_eq!(expected_cpu.reg(0), EWRAM_BASE + 0x10A);
    assert_eq!(expected_cpu.reg(3), 0x22);
    assert_eq!(expected_cpu.reg(4), 0);
    assert_eq!(expected_cpu.reg(5), EWRAM_BASE + 0x10E);
    assert_eq!(expected_bus.read32(EWRAM_BASE + 0x100), Ok(0x11));
    assert_eq!(expected_bus.read32(EWRAM_BASE + 0x104), Ok(0x22));

    let report = build_and_run(&source, &cart, "thumb");
    assert_eq!(report.budget_used, budget);
    assert_eq!(report.fallbacks, 0);
    assert_eq!(report.pc, 0x0800_000C);
    assert_eq!(report.regs[0], EWRAM_BASE + 0x10A);
    assert_eq!(report.regs[3], 0x22);
    assert_eq!(report.regs[5], EWRAM_BASE + 0x10E);
    assert!(
        report
            .matches_reference(&expected_cpu, &expected_bus, budget)
            .expect("compare native/reference")
    );
}
