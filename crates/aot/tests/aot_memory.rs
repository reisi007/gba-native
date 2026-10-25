//! Native-AOT integration for the single-transfer memory slice.
//!
//! Synthetic ARM fixture: aligned/unaligned word store/load, byte store/load,
//! halfword store, odd LDRSH, DISPCNT setup and a word `LDR pc`. Expected
//! registers/bus are hand-derived; the dynamic PC target is a deliberate
//! one-instruction reference fallback.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, DISPCNT_ADDR, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::{Cpu, USER_STACK};
use gba_native::display::{HEIGHT, Image, WIDTH};
use gba_native::interpret;
use gba_native::sha256;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-memory-aot-{tag}-{}-{sequence}",
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
const TARGET: u32 = 0x0800_0044;
const BUDGET: u64 = 18;
const RAM: u32 = EWRAM_BASE + 0x100;

fn ldr_literal(rd: u32, instruction_addr: u32, target: u32) -> u32 {
    let base = instruction_addr + 8;
    assert!(target >= base);
    let difference = target - base;
    assert_eq!(difference % 4, 0);
    assert!(difference < 4096);
    0xE59F_0000 | (rd << 12) | difference
}

fn str_imm(rd: u32, rn: u32, offset: u32) -> u32 {
    assert!(offset < 4096);
    0xE580_0000 | (rn << 16) | (rd << 12) | offset
}

fn ldr_imm(rd: u32, rn: u32, offset: u32) -> u32 {
    assert!(offset < 4096);
    0xE590_0000 | (rn << 16) | (rd << 12) | offset
}

fn strb_imm(rd: u32, rn: u32, offset: u32) -> u32 {
    assert!(offset < 4096);
    0xE5C0_0000 | (rn << 16) | (rd << 12) | offset
}

fn ldrb_imm(rd: u32, rn: u32, offset: u32) -> u32 {
    assert!(offset < 4096);
    0xE5D0_0000 | (rn << 16) | (rd << 12) | offset
}

fn strh_imm(rd: u32, rn: u32, offset: u32) -> u32 {
    assert!(offset < 256);
    0xE1C0_00B0 | (rn << 16) | (rd << 12) | ((offset & 0xF0) << 4) | (offset & 0xF)
}

fn ldrsh_imm(rd: u32, rn: u32, offset: u32) -> u32 {
    assert!(offset < 256);
    0xE1D0_00F0 | (rn << 16) | (rd << 12) | ((offset & 0xF0) << 4) | (offset & 0xF)
}

fn fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(0, 0x00, 0x80), // r0 = EWRAM+0x100
        ldr_literal(1, 0x04, 0x84), // r1 = 0x11223344
        str_imm(1, 0, 2),           // STR r1,[r0,#2] -> aligned word at +0x100
        ldr_imm(2, 0, 0),           // aligned word load
        ldr_imm(10, 0, 1),          // unaligned word load (ROR 8)
        ldr_literal(3, 0x14, 0x88), // r3 = 1
        strb_imm(3, 0, 5),          // byte at +0x105
        ldrb_imm(4, 0, 5),          // byte load
        ldr_literal(5, 0x20, 0x8C), // r5 = 0x8001
        strh_imm(5, 0, 8),          // half at +0x108
        ldrsh_imm(6, 0, 9),         // odd signed half -> signed byte 0x80
        ldr_literal(7, 0x2C, 0x90), // r7 = TARGET
        str_imm(7, 0, 12),          // store target in RAM
        ldr_literal(8, 0x34, 0x94), // r8 = DISPCNT
        ldr_literal(9, 0x38, 0x98), // r9 = 0x0403
        0xE188_90BB,                // STRH r9,[r8,r11] (r11=0): DISPCNT
        ldr_imm(15, 0, 12),         // LDR pc,[r0,#12] -> TARGET
        0xEAFF_FFFE,                // TARGET: B .
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    let literals = [
        RAM,
        0x1122_3344,
        1,
        0x0000_8001,
        TARGET,
        DISPCNT_ADDR,
        0x0000_0403,
    ];
    for (index, word) in literals.iter().enumerate() {
        let offset = 0x80 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    finish(rom, b"GBA-MEM\0")
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
    Cartridge::from_bytes(rom).expect("synthetic memory fixture")
}

#[test]
fn memory_and_block_destinations_invalidate_cached_bx_constants() {
    // A same-block literal LDR caches r0; the following memory load must
    // invalidate it so BX r0 remains dynamic.
    let mut rom = vec![0u8; 0x200];
    for (index, word) in [
        ldr_literal(0, 0x00, 0x20),
        0xE591_0000u32,
        0xE12F_FF10,
        0xEAFF_FFFE,
    ]
    .iter()
    .enumerate()
    {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x20..0x24].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
    let cart = finish(rom, b"GBA-MBX\0");
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default())
        .expect("discover memory-invalidated BX");
    assert_eq!(artifact.stats.resolved_bx, 0);
    assert_eq!(artifact.stats.dynamic_bx, 1);
    assert!(
        artifact
            .blocks
            .iter()
            .all(|block| block.addr != 0x0800_0020),
        "a memory load must not leave a stale BX constant"
    );

    // The same must hold when a block load writes the register.
    let mut rom = vec![0u8; 0x200];
    for (index, word) in [
        ldr_literal(0, 0x00, 0x20),
        0xE891_0001u32,
        0xE12F_FF10,
        0xEAFF_FFFE,
    ]
    .iter()
    .enumerate()
    {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x20..0x24].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
    let cart = finish(rom, b"GBA-LBX\0");
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default())
        .expect("discover block-invalidated BX");
    assert_eq!(artifact.stats.resolved_bx, 0);
    assert_eq!(artifact.stats.dynamic_bx, 1);
    assert!(
        artifact
            .blocks
            .iter()
            .all(|block| block.addr != 0x0800_0020)
    );

    // PUSH writes SP, so a cached SP constant must not resolve a later BX.
    let mut rom = vec![0u8; 0x200];
    for (index, word) in [
        0xE3A0_D020u32, // MOV sp,#0x20
        0xE92D_0001,    // PUSH {r0} (STMDB sp!,{r0})
        0xE12F_FF1D,    // BX sp
        0xEAFF_FFFE,    // B .
    ]
    .iter()
    .enumerate()
    {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x20..0x24].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
    let cart = finish(rom, b"GBA-SBX\0");
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default())
        .expect("discover SP-invalidated BX");
    assert_eq!(artifact.stats.resolved_bx, 0);
    assert_eq!(artifact.stats.dynamic_bx, 1);
    assert!(
        artifact
            .blocks
            .iter()
            .all(|block| block.addr != 0x0800_0020)
    );

    // Thumb POP and LDMIA also write their destination registers.
    for writer in [0xBC01u16, 0xC901] {
        let mut rom = vec![0u8; 0x200];
        for (index, halfword) in [0x4802u16, writer, 0x4700, 0xE7FE].iter().enumerate() {
            rom[index * 2..index * 2 + 2].copy_from_slice(&halfword.to_le_bytes());
        }
        rom[0x0C..0x10].copy_from_slice(&0x0800_0010u32.to_le_bytes());
        rom[0x10..0x12].copy_from_slice(&0xE7FEu16.to_le_bytes());
        let cart = finish(rom, b"GBA-TBX\0");
        let artifact = aot::discover(&cart, ENTRY, true, aot::Limits::default())
            .expect("discover Thumb-invalidated BX");
        assert_eq!(artifact.stats.resolved_bx, 0, "{writer:04X}");
        assert_eq!(artifact.stats.dynamic_bx, 1, "{writer:04X}");
        assert!(
            artifact
                .blocks
                .iter()
                .all(|block| block.addr != 0x0800_0010),
            "{writer:04X}"
        );
    }
}

#[test]
fn native_memory_fixture_uses_direct_widths_and_matches_hand_expected_state() {
    let cart = fixture();
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default())
        .expect("discover memory fixture");
    assert_eq!(artifact.stats.blocks, 1);
    assert_eq!(artifact.stats.instructions, 17);
    assert_eq!(artifact.stats.pc_writes, 1);
    assert_eq!(artifact.stats.decode_stops, 0);

    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit memory source");
    let translated = source.split("fn main()").next().expect("block section");
    assert!(!translated.contains("interpret::step"));
    assert!(!translated.contains("decode_arm("));
    assert!(!source.contains("ROM_BYTES"));
    assert!(!source.contains("GBA-MEM"));
    assert!(source.contains("bus.write32(address & !3, source)?;"));
    assert!(source.contains("bus.write8(address, source as u8)?;"));
    assert!(source.contains("bus.write16(address & !1, source as u16)?;"));
    assert!(source.contains("cpu.set_reg(15, value & !3);"));

    let mut expected_cpu = Cpu::direct_entry();
    expected_cpu.initialize_bios_stacks();
    expected_cpu.set_pc(ENTRY);
    expected_cpu.set_thumb(false);
    let mut expected_bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut expected_cpu, &mut expected_bus, BUDGET)
        .expect("reference memory run");
    assert_eq!(used, BUDGET);
    assert_eq!(expected_cpu.pc(), TARGET);
    assert_eq!(expected_cpu.reg(0), RAM);
    assert_eq!(expected_cpu.reg(1), 0x1122_3344);
    assert_eq!(expected_cpu.reg(2), 0x1122_3344);
    assert_eq!(expected_cpu.reg(3), 1);
    assert_eq!(expected_cpu.reg(4), 1);
    assert_eq!(expected_cpu.reg(5), 0x0000_8001);
    assert_eq!(expected_cpu.reg(6), 0xFFFF_FF80);
    assert_eq!(expected_cpu.reg(7), TARGET);
    assert_eq!(expected_cpu.reg(8), DISPCNT_ADDR);
    assert_eq!(expected_cpu.reg(9), 0x0403);
    assert_eq!(expected_cpu.reg(10), 0x4411_2233);
    assert_eq!(expected_cpu.reg(13), USER_STACK);
    assert_eq!(expected_cpu.cpsr(), 0x0000_00DF);
    assert!(!expected_cpu.thumb());
    assert_eq!(expected_bus.read32(RAM), Ok(0x1122_3344));
    assert_eq!(expected_bus.read32(RAM + 4), Ok(0x0000_0100));
    assert_eq!(expected_bus.read16(RAM + 8), Ok(0x8001));
    assert_eq!(expected_bus.read32(RAM + 12), Ok(TARGET));
    assert_eq!(expected_bus.dispcnt(), 0x0403);

    let temp = TempDir::new("fixture");
    let path = temp.path().join("fixture.gba");
    std::fs::write(&path, cart.bytes()).expect("write fixture");
    let harness =
        NativeHarness::build(&source, &temp.path().join("harness")).expect("build memory harness");
    let report = harness.run(&path).expect("run memory harness");

    assert_eq!(report.budget_used, BUDGET);
    assert_eq!(report.budget_remaining, 0);
    assert_eq!(
        report.fallbacks, 1,
        "the dynamic LDR PC target falls back once"
    );
    assert_eq!(report.pc, TARGET);
    assert_eq!(report.cpsr, 0x0000_00DF);
    assert!(!report.thumb);
    assert_eq!(
        report.regs,
        [
            RAM,          // r0
            0x1122_3344,  // r1
            0x1122_3344,  // r2
            1,            // r3
            1,            // r4
            0x0000_8001,  // r5
            0xFFFF_FF80,  // r6: LDRSH odd signed byte
            TARGET,       // r7
            DISPCNT_ADDR, // r8
            0x0403,       // r9
            0x4411_2233,  // r10: unaligned word load ROR 8
            0,            // r11
            0,            // r12
            USER_STACK,   // r13
            0,            // r14
            TARGET,       // r15
        ]
    );
    assert!(
        report
            .matches_reference(&expected_cpu, &expected_bus, BUDGET)
            .expect("compare native/reference")
    );

    // Independent expected bus: only the explicit stores plus DISPCNT.
    let mut bus = Bus::new(cart.clone());
    bus.write32(RAM, 0x1122_3344).expect("word store");
    bus.write8(RAM + 5, 1).expect("byte store");
    bus.write16(RAM + 8, 0x8001).expect("half store");
    bus.write32(RAM + 12, TARGET).expect("target store");
    bus.write16(DISPCNT_ADDR, 0x0403).expect("DISPCNT");
    assert_eq!(report.bus_sha256, sha256::hex(&bus.state_digest()));
    assert_eq!(report.vram_sha256, sha256::hex(&bus.vram_digest()));
    assert_eq!(report.rom_sha256, cart.sha256_hex());

    // Independent image expectation: mode-3 VRAM is all zero, so the complete
    // 240x160 packed RGB image is black.
    let expected_image =
        Image::new(WIDTH, HEIGHT, vec![0; (WIDTH * HEIGHT) as usize]).expect("expected image");
    assert_eq!(report.image_width, WIDTH);
    assert_eq!(report.image_height, HEIGHT);
    assert_eq!(report.image_sha256, expected_image.sha256_hex());
}
