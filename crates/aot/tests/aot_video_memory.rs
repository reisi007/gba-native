//! Native-AOT coverage for the explicit prototype video-memory alias policies.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, DISPCNT_ADDR, OAM_BASE, VRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::interpret;
use gba_native::sha256;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
const ENTRY: u32 = 0x0800_0000;
const BUDGET: u64 = 18;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-video-memory-aot-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("create unique temp dir");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ldr_literal(rd: u32, instruction_addr: u32, target: u32) -> u32 {
    let offset = target
        .checked_sub(instruction_addr + 8)
        .expect("forward literal");
    assert_eq!(offset & 3, 0);
    assert!(offset < 4096);
    0xE59F_0000 | (rd << 12) | offset
}

fn str_imm(rd: u32, rn: u32) -> u32 {
    0xE580_0000 | (rn << 16) | (rd << 12)
}

fn ldr_imm(rd: u32, rn: u32) -> u32 {
    0xE590_0000 | (rn << 16) | (rd << 12)
}

fn strh_imm(rd: u32, rn: u32) -> u32 {
    0xE1C0_00B0 | (rn << 16) | (rd << 12)
}

fn fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(0, 0x00, 0x60),  // r0 = VRAM_BASE
        ldr_literal(1, 0x04, 0x64),  // r1 = test word
        ldr_literal(2, 0x08, 0x68),  // r2 = DISPCNT
        0xE3A0_3000,                 // MOV r3,#0 (tiled mode)
        strh_imm(3, 2),              // STRH r3,[r2]
        str_imm(1, 0),               // STR r1,[r0]
        ldr_literal(4, 0x18, 0x6C),  // r4 = VRAM_BASE+0x10000
        str_imm(1, 4),               // STR r1,[r4]
        ldr_literal(5, 0x20, 0x70),  // r5 = VRAM_BASE+0x20000
        ldr_imm(6, 5),               // LDR r6,[r5] -> 128 KiB alias
        ldr_literal(7, 0x28, 0x74),  // r7 = VRAM_BASE+0x18000
        ldr_imm(8, 7),               // LDR r8,[r7] -> tiled 32 KiB mirror
        ldr_imm(9, 0),               // LDR r9,[r0]
        ldr_literal(10, 0x34, 0x78), // r10 = OAM_BASE
        str_imm(1, 10),              // STR r1,[r10]
        ldr_literal(11, 0x3C, 0x7C), // r11 = OAM_BASE+0x400
        ldr_imm(12, 11),             // LDR r12,[r11] -> narrow OAM mirror
        ldr_imm(14, 10),             // LDR r14,[r10]
        0xEAFF_FFFE,                 // B .
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    for (index, word) in [
        VRAM_BASE,
        0x1122_3344,
        DISPCNT_ADDR,
        VRAM_BASE + 0x10000,
        VRAM_BASE + 0x20000,
        VRAM_BASE + 0x18000,
        OAM_BASE,
        OAM_BASE + 0x400,
    ]
    .iter()
    .enumerate()
    {
        let offset = 0x60 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0xA0..0xA8].copy_from_slice(b"GBA-VRAM");
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let checksum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(checksum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).expect("synthetic VRAM fixture")
}

#[test]
fn native_video_memory_aliases_match_independent_expectations() {
    let cart = fixture();
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default())
        .expect("discover VRAM alias fixture");
    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit VRAM fixture");
    let mut expected_cpu = Cpu::direct_entry();
    expected_cpu.initialize_bios_stacks();
    expected_cpu.set_pc(ENTRY);
    let mut expected_bus = Bus::new(cart.clone());
    assert_eq!(
        interpret::run_budget(&mut expected_cpu, &mut expected_bus, BUDGET),
        Ok(BUDGET)
    );
    assert_eq!(expected_cpu.reg(6), 0x1122_3344);
    assert_eq!(expected_cpu.reg(8), 0x1122_3344);
    assert_eq!(expected_cpu.reg(9), 0x1122_3344);
    assert_eq!(expected_cpu.reg(12), 0x1122_3344);
    assert_eq!(expected_cpu.reg(14), 0x1122_3344);

    let mut independent_bus = Bus::new(cart.clone());
    independent_bus
        .write32(VRAM_BASE, 0x1122_3344)
        .expect("base VRAM store");
    independent_bus
        .write32(VRAM_BASE + 0x10000, 0x1122_3344)
        .expect("tiled VRAM store");
    independent_bus
        .write32(OAM_BASE, 0x1122_3344)
        .expect("OAM store");
    assert_eq!(independent_bus.read32(VRAM_BASE + 0x20000), Ok(0x1122_3344));
    assert_eq!(independent_bus.read32(VRAM_BASE + 0x18000), Ok(0x1122_3344));
    assert_eq!(independent_bus.read32(OAM_BASE + 0x400), Ok(0x1122_3344));

    let temp = TempDir::new();
    let rom_path = temp.0.join("fixture.gba");
    std::fs::write(&rom_path, cart.bytes()).expect("write fixture ROM");
    let harness =
        NativeHarness::build(&source, &temp.0.join("harness")).expect("build VRAM harness");
    let report = harness.run(&rom_path).expect("run VRAM harness");
    assert_eq!(report.regs[6], 0x1122_3344);
    assert_eq!(report.regs[8], 0x1122_3344);
    assert_eq!(report.regs[9], 0x1122_3344);
    assert_eq!(report.regs[12], 0x1122_3344);
    assert_eq!(report.regs[14], 0x1122_3344);
    assert_eq!(report.budget_used, BUDGET);
    assert_eq!(report.fallbacks, 0);
    assert!(
        report
            .matches_reference(&expected_cpu, &expected_bus, BUDGET)
            .expect("parity")
    );
    assert_eq!(
        report.bus_sha256,
        sha256::hex(&independent_bus.state_digest())
    );
}
