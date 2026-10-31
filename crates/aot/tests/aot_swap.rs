//! Synthetic native-AOT coverage for documented ARMv4T SWP/SWPB semantics.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::{Cpu, USER_STACK};
use gba_native::interpret;
use gba_native::sha256;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-swap-aot-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("create unique temp directory");
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
const RAM: u32 = EWRAM_BASE + 0x100;
const BUDGET: u64 = 8;

fn ldr_literal(rd: u32, instruction_addr: u32, target: u32) -> u32 {
    let displacement = target - (instruction_addr + 8);
    assert!(displacement < 4096);
    0xE59F_0000 | (rd << 12) | displacement
}

fn swap(byte: bool, rn: u32, rd: u32, rm: u32) -> u32 {
    0xE100_0090 | (u32::from(byte) << 22) | (rn << 16) | (rd << 12) | rm
}

fn finish(mut rom: Vec<u8>) -> Cartridge {
    rom[0xA0..0xAC].copy_from_slice(b"GBA-SWAP\0\0\0\0");
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).expect("synthetic swap cartridge")
}

fn fixture() -> Cartridge {
    let mut rom = vec![0; 0x200];
    let words = [
        ldr_literal(0, 0x00, 0x40), // r0 = RAM
        ldr_literal(1, 0x04, 0x44), // r1 = initial word
        0xE580_1000,                // STR r1,[r0]
        ldr_literal(2, 0x0C, 0x48), // r2 = source word
        0xE280_3001,                // ADD r3,r0,#1
        swap(false, 3, 4, 2),       // unaligned word swap
        swap(true, 3, 5, 2),        // byte swap at the same unaligned address
        0xEAFF_FFF7,                // B ENTRY
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    for (index, word) in [RAM, 0x1122_3344u32, 0xAABB_CCDD].iter().enumerate() {
        let offset = 0x40 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    finish(rom)
}

#[test]
fn swap_destination_invalidates_cached_bx_constants() {
    let mut rom = vec![0; 0x200];
    let words = [
        ldr_literal(0, 0x00, 0x40),
        ldr_literal(1, 0x04, 0x44),
        0xE3A0_2000, // MOV r2,#0
        swap(false, 1, 0, 2),
        0xE12F_FF10, // BX r0 must stay dynamic after SWP writes r0
        0xEAFF_FFFE,
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x30..0x34].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
    rom[0x40..0x44].copy_from_slice(&0x0800_0030u32.to_le_bytes());
    rom[0x44..0x48].copy_from_slice(&RAM.to_le_bytes());
    let cart = finish(rom);

    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default())
        .expect("discover SWP destination write");

    assert_eq!(artifact.stats.resolved_bx, 0);
    assert_eq!(artifact.stats.dynamic_bx, 1);
    assert!(
        artifact
            .blocks
            .iter()
            .all(|block| block.addr != ENTRY + 0x30)
    );
}

#[test]
fn native_swap_fixture_matches_independent_memory_and_register_expectations() {
    let cart = fixture();
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default())
        .expect("discover SWP/SWPB fixture");
    assert_eq!(artifact.stats.blocks, 1);
    assert_eq!(artifact.stats.instructions, BUDGET as usize);
    assert_eq!(artifact.stats.decode_stops, 0);

    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit SWP/SWPB");
    let translated = source
        .split("pub fn run_program")
        .next()
        .expect("translated blocks");
    assert!(!translated.contains("interpret::step"));
    assert!(translated.contains("bus.write32(address & !3, source)?;"));
    assert!(translated.contains("bus.write8(address, source as u8)?;"));

    let mut expected_cpu = Cpu::direct_entry();
    expected_cpu.initialize_bios_stacks();
    expected_cpu.set_pc(ENTRY);
    expected_cpu.set_thumb(false);
    let mut expected_bus = Bus::new(cart.clone());
    assert_eq!(
        interpret::run_budget(&mut expected_cpu, &mut expected_bus, BUDGET),
        Ok(BUDGET)
    );
    assert_eq!(expected_cpu.pc(), ENTRY);
    assert_eq!(expected_cpu.reg(0), RAM);
    assert_eq!(expected_cpu.reg(1), 0x1122_3344);
    assert_eq!(expected_cpu.reg(2), 0xAABB_CCDD);
    assert_eq!(expected_cpu.reg(3), RAM + 1);
    assert_eq!(expected_cpu.reg(4), 0x4411_2233);
    assert_eq!(expected_cpu.reg(5), 0xCC);
    assert_eq!(expected_cpu.reg(13), USER_STACK);
    assert_eq!(expected_bus.read32(RAM), Ok(0xAABB_DDDD));

    let mut independent_bus = Bus::new(cart.clone());
    independent_bus
        .write32(RAM, 0xAABB_DDDD)
        .expect("independent expected swap result");

    let temp = TempDir::new();
    let rom_path = temp.path().join("fixture.gba");
    std::fs::write(&rom_path, cart.bytes()).expect("write synthetic fixture");
    let harness = NativeHarness::build(&source, &temp.path().join("harness"))
        .expect("build native swap harness");
    let report = harness.run(&rom_path).expect("run native swap harness");

    assert_eq!(report.budget_used, BUDGET);
    assert_eq!(report.fallbacks, 0);
    assert_eq!(report.pc, ENTRY);
    assert_eq!(report.regs[4], 0x4411_2233);
    assert_eq!(report.regs[5], 0xCC);
    assert!(
        report
            .matches_reference(&expected_cpu, &expected_bus, BUDGET)
            .expect("native/reference comparison")
    );
    assert_eq!(
        report.bus_sha256,
        sha256::hex(&independent_bus.state_digest())
    );
}
