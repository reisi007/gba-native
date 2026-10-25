//! Native-AOT integration for ARM LDM/STM and Thumb PUSH/POP/LDM/STM.
//!
//! Both fixtures are synthetic. Expected registers and bus contents are
//! hand-derived from the encodings; accesses are unrolled in the generated
//! native source and each guest instruction consumes exactly one budget unit.

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
            "gba-native-block-aot-{tag}-{}-{sequence}",
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

fn strh_imm(rd: u32, rn: u32, offset: u32) -> u32 {
    assert!(offset < 256);
    0xE1C0_00B0 | (rn << 16) | (rd << 12) | ((offset & 0xF0) << 4) | (offset & 0xF)
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
    Cartridge::from_bytes(rom).expect("synthetic block fixture")
}

fn arm_fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(0, 0x00, 0x40), // r0 = EWRAM+0x200
        ldr_literal(1, 0x04, 0x44), // r1 = 0x11
        ldr_literal(2, 0x08, 0x48), // r2 = 0x22
        ldr_literal(3, 0x0C, 0x4C), // r3 = 0x33
        0xE8A0_000E,                // STMIA r0!,{r1,r2,r3}
        0xE3A0_1000,                // MOV r1,#0
        0xE3A0_2000,                // MOV r2,#0
        0xE3A0_3000,                // MOV r3,#0
        0xE910_0070,                // LDMDB r0,{r4,r5,r6}
        ldr_literal(7, 0x24, 0x50), // r7 = DISPCNT
        ldr_literal(8, 0x28, 0x54), // r8 = 0x0403
        strh_imm(8, 7, 0),          // DISPCNT
        0xEAFF_FFFE,                // B .
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    for (index, word) in [EWRAM_BASE + 0x200, 0x11, 0x22, 0x33, DISPCNT_ADDR, 0x0403]
        .iter()
        .enumerate()
    {
        let offset = 0x40 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    finish(rom, b"GBA-BLK\0")
}

fn thumb_fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let halfwords: [u16; 19] = [
        0x2011, // MOV r0,#0x11
        0x2122, // MOV r1,#0x22
        0xB403, // PUSH {r0,r1}
        0x2000, // MOV r0,#0
        0x2100, // MOV r1,#0
        0xBC03, // POP {r0,r1}
        0x4D10, // LDR r5,[pc,#0x40] -> literal at 0x50
        0x2044, // MOV r0,#0x44
        0x2155, // MOV r1,#0x55
        0xC503, // STMIA r5!,{r0,r1}
        0x2000, // MOV r0,#0
        0x2100, // MOV r1,#0
        0x4E0E, // LDR r6,[pc,#0x38] -> literal at 0x54
        0xCE0C, // LDMIA r6!,{r2,r3}
        0x2433, // MOV r4,#0x33
        0x4F0E, // LDR r7,[pc,#0x38] -> literal at 0x58
        0x480E, // LDR r0,[pc,#0x38] -> literal at 0x5C (temporary)
        0x8038, // STRH r0,[r7]
        0xE7FE, // B .
    ];
    for (index, halfword) in halfwords.iter().enumerate() {
        rom[index * 2..index * 2 + 2].copy_from_slice(&halfword.to_le_bytes());
    }
    for (index, word) in [EWRAM_BASE + 0x300, EWRAM_BASE + 0x300, DISPCNT_ADDR, 0x0403]
        .iter()
        .enumerate()
    {
        let offset = 0x50 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    finish(rom, b"GBA-TBL\0")
}

fn build_and_run(source: &str, cart: &Cartridge, tag: &str) -> (aot::NativeReport, Cpu, Bus) {
    let temp = TempDir::new(tag);
    let path = temp.path().join("fixture.gba");
    std::fs::write(&path, cart.bytes()).expect("write fixture");
    let harness = NativeHarness::build(source, &temp.path().join("harness"))
        .expect("build generated harness");
    let report = harness.run(&path).expect("run generated harness");
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    (report, cpu, Bus::new(cart.clone()))
}

#[test]
fn discovery_ends_pc_writing_memory_blocks_and_queues_conditional_fallthrough() {
    // ARM: LDREQ pc,[r0] ends the block and queues its conditional fallthrough.
    let mut rom = vec![0u8; 0x200];
    for (index, word) in [0x0590_F000u32, 0xE3A0_1001, 0xEAFF_FFFE]
        .iter()
        .enumerate()
    {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    let cart = finish(rom, b"GBA-CPC\0");
    let artifact = aot::discover(&cart, 0x0800_0000, false, aot::Limits::default())
        .expect("discover conditional PC load");
    let addrs: Vec<u32> = artifact.blocks.iter().map(|block| block.addr).collect();
    assert!(addrs.contains(&0x0800_0000));
    assert!(
        addrs.contains(&0x0800_0004),
        "conditional PC load must queue its fallthrough: {addrs:X?}"
    );

    // ARM: an unconditional LDM with PC also ends the block and has no
    // statically known successor.
    let mut rom = vec![0u8; 0x200];
    for (index, word) in [0xE890_8000u32, 0xE3A0_1001, 0xEAFF_FFFE]
        .iter()
        .enumerate()
    {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    let cart = finish(rom, b"GBA-LPC\0");
    let artifact =
        aot::discover(&cart, 0x0800_0000, false, aot::Limits::default()).expect("discover LDM PC");
    assert_eq!(artifact.blocks.len(), 1);
    assert_eq!(artifact.blocks[0].addr, 0x0800_0000);
    assert!(artifact.blocks[0].instructions[0].is_control_flow());

    // Thumb POP {PC} is likewise terminal without a discovered fallthrough.
    let mut rom = vec![0u8; 0x200];
    for (index, halfword) in [0xBD00u16, 0x2001, 0xE7FE].iter().enumerate() {
        rom[index * 2..index * 2 + 2].copy_from_slice(&halfword.to_le_bytes());
    }
    let cart = finish(rom, b"GBA-PPC\0");
    let artifact = aot::discover(&cart, 0x0800_0000, true, aot::Limits::default())
        .expect("discover Thumb POP PC");
    assert_eq!(artifact.blocks.len(), 1);
    assert_eq!(artifact.blocks[0].addr, 0x0800_0000);
    assert!(artifact.blocks[0].instructions[0].is_control_flow());
}

#[test]
fn native_arm_ldm_stm_fixture_matches_hand_expected_state() {
    let cart = arm_fixture();
    let artifact = aot::discover(&cart, 0x0800_0000, false, aot::Limits::default())
        .expect("discover ARM block fixture");
    assert_eq!(artifact.stats.instructions, 14);
    assert_eq!(artifact.stats.blocks, 2);
    assert_eq!(artifact.stats.decode_stops, 0);

    let budget = 13;
    let source = aot::emit_native_module(&cart, &artifact, budget).expect("emit ARM block");
    let translated = source.split("fn main()").next().expect("block section");
    assert!(!translated.contains("interpret::step"));
    assert!(!source.contains("ROM_BYTES"));
    assert!(source.contains("bus.write32(start.wrapping_add(0x0u32), cpu.reg(1))?;"));
    assert!(source.contains("bus.read32(start.wrapping_add(0x0u32))?;"));
    assert!(source.contains("cpu.set_reg(0, new_base);"));

    let mut expected_cpu = Cpu::direct_entry();
    expected_cpu.initialize_bios_stacks();
    expected_cpu.set_pc(0x0800_0000);
    expected_cpu.set_thumb(false);
    let mut expected_bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut expected_cpu, &mut expected_bus, budget)
        .expect("reference ARM block run");
    assert_eq!(used, budget);
    assert_eq!(expected_cpu.pc(), 0x0800_0030);
    assert_eq!(expected_cpu.reg(0), EWRAM_BASE + 0x20C);
    assert_eq!(expected_cpu.reg(1), 0);
    assert_eq!(expected_cpu.reg(2), 0);
    assert_eq!(expected_cpu.reg(3), 0);
    assert_eq!(expected_cpu.reg(4), 0x11);
    assert_eq!(expected_cpu.reg(5), 0x22);
    assert_eq!(expected_cpu.reg(6), 0x33);
    assert_eq!(expected_cpu.reg(7), DISPCNT_ADDR);
    assert_eq!(expected_cpu.reg(8), 0x0403);
    assert_eq!(expected_cpu.cpsr(), 0x0000_00DF);
    assert_eq!(expected_bus.read32(EWRAM_BASE + 0x200), Ok(0x11));
    assert_eq!(expected_bus.read32(EWRAM_BASE + 0x204), Ok(0x22));
    assert_eq!(expected_bus.read32(EWRAM_BASE + 0x208), Ok(0x33));
    assert_eq!(expected_bus.dispcnt(), 0x0403);

    let (report, _, _) = build_and_run(&source, &cart, "arm");
    assert_eq!(report.budget_used, budget);
    assert_eq!(report.fallbacks, 0);
    assert_eq!(report.pc, 0x0800_0030);
    assert_eq!(report.regs[0], EWRAM_BASE + 0x20C);
    assert_eq!(report.regs[4], 0x11);
    assert_eq!(report.regs[5], 0x22);
    assert_eq!(report.regs[6], 0x33);
    assert!(
        report
            .matches_reference(&expected_cpu, &expected_bus, budget)
            .expect("compare native/reference")
    );

    let mut bus = Bus::new(cart.clone());
    bus.write32(EWRAM_BASE + 0x200, 0x11).expect("store 1");
    bus.write32(EWRAM_BASE + 0x204, 0x22).expect("store 2");
    bus.write32(EWRAM_BASE + 0x208, 0x33).expect("store 3");
    bus.write16(DISPCNT_ADDR, 0x0403).expect("DISPCNT");
    assert_eq!(report.bus_sha256, sha256::hex(&bus.state_digest()));
}

#[test]
fn native_thumb_stack_and_block_fixture_matches_hand_expected_state() {
    let cart = thumb_fixture();
    let artifact = aot::discover(&cart, 0x0800_0000, true, aot::Limits::default())
        .expect("discover Thumb block fixture");
    assert_eq!(artifact.stats.instructions, 20);
    assert_eq!(artifact.stats.blocks, 2);
    assert_eq!(artifact.stats.decode_stops, 0);

    let budget = 19;
    let source = aot::emit_native_module(&cart, &artifact, budget).expect("emit Thumb block");
    let translated = source.split("fn main()").next().expect("block section");
    assert!(!translated.contains("interpret::step"));
    assert!(!source.contains("ROM_BYTES"));
    assert!(source.contains("cpu.reg(13)"));
    assert!(source.contains("cpu.set_reg(13, start);"));
    assert!(source.contains("bus.write32(base.wrapping_add(0x0u32)"));

    let mut expected_cpu = Cpu::direct_entry();
    expected_cpu.initialize_bios_stacks();
    expected_cpu.set_pc(0x0800_0000);
    expected_cpu.set_thumb(true);
    let mut expected_bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut expected_cpu, &mut expected_bus, budget)
        .expect("reference Thumb block run");
    assert_eq!(used, budget);
    assert_eq!(expected_cpu.pc(), 0x0800_0024);
    assert!(expected_cpu.thumb());
    assert_eq!(expected_cpu.reg(0), 0x0403);
    assert_eq!(expected_cpu.reg(1), 0);
    assert_eq!(expected_cpu.reg(2), 0x44);
    assert_eq!(expected_cpu.reg(3), 0x55);
    assert_eq!(expected_cpu.reg(4), 0x33);
    assert_eq!(expected_cpu.reg(5), EWRAM_BASE + 0x308);
    assert_eq!(expected_cpu.reg(6), EWRAM_BASE + 0x308);
    assert_eq!(expected_cpu.reg(7), DISPCNT_ADDR);
    assert_eq!(expected_cpu.reg(8), 0);
    assert_eq!(expected_cpu.reg(13), USER_STACK);
    assert_eq!(expected_cpu.cpsr(), 0x0000_00FF); // Thumb bit set, NZCV clear
    assert_eq!(expected_bus.read32(USER_STACK - 8), Ok(0x11));
    assert_eq!(expected_bus.read32(USER_STACK - 4), Ok(0x22));
    assert_eq!(expected_bus.read32(EWRAM_BASE + 0x300), Ok(0x44));
    assert_eq!(expected_bus.read32(EWRAM_BASE + 0x304), Ok(0x55));
    assert_eq!(expected_bus.dispcnt(), 0x0403);

    let (report, _, _) = build_and_run(&source, &cart, "thumb");
    assert_eq!(report.budget_used, budget);
    assert_eq!(report.fallbacks, 0);
    assert_eq!(report.pc, 0x0800_0024);
    assert!(report.thumb);
    assert_eq!(report.regs[2], 0x44);
    assert_eq!(report.regs[3], 0x55);
    assert_eq!(report.regs[5], EWRAM_BASE + 0x308);
    assert_eq!(report.regs[13], USER_STACK);
    assert!(
        report
            .matches_reference(&expected_cpu, &expected_bus, budget)
            .expect("compare native/reference")
    );

    let mut bus = Bus::new(cart.clone());
    bus.write32(USER_STACK - 8, 0x11).expect("push 1");
    bus.write32(USER_STACK - 4, 0x22).expect("push 2");
    bus.write32(EWRAM_BASE + 0x300, 0x44).expect("stm 1");
    bus.write32(EWRAM_BASE + 0x304, 0x55).expect("stm 2");
    bus.write16(DISPCNT_ADDR, 0x0403).expect("DISPCNT");
    assert_eq!(report.bus_sha256, sha256::hex(&bus.state_digest()));

    let expected_image =
        Image::new(WIDTH, HEIGHT, vec![0; (WIDTH * HEIGHT) as usize]).expect("expected image");
    assert_eq!(report.image_width, WIDTH);
    assert_eq!(report.image_height, HEIGHT);
    assert_eq!(report.image_sha256, expected_image.sha256_hex());
}
