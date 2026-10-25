//! Dedicated native RAM self-modifying regression.
//!
//! The guest ROM writes a two-instruction ARM routine (`MOV r0,#1` / `BX lr`)
//! to EWRAM, calls it, returns to a ROM label, patches the RAM routine to
//! `MOV r0,#2`, and calls it again. Discovery must translate only ROM blocks
//! (including the ROM return labels reached through false conditional targets)
//! and the native harness must fall back to a fresh reference fetch/decode for
//! each of the four RAM instructions. There is no RAM translation and no stale
//! decoded RAM word.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, DISPCNT_ADDR, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::{Cpu, USER_STACK};
use gba_native::demo;
use gba_native::interpret;
use gba_native::sha256;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-ram-aot-{tag}-{}-{sequence}",
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
const RET1: u32 = 0x0800_002C;
const RET2: u32 = 0x0800_0044;
const FINISH: u32 = 0x0800_0054;
const RAM: u32 = EWRAM_BASE;
const BUDGET: u64 = 26;

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

fn bne(instr_addr: u32, target: u32) -> u32 {
    let difference = target as i64 - (instr_addr as i64 + 8);
    assert_eq!(difference % 4, 0);
    let imm24 = ((difference >> 2) as u32) & 0x00FF_FFFF;
    0x1A00_0000 | imm24
}

fn bx(rm: u32) -> u32 {
    0xE12F_FF10 | rm
}

fn fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(1, 0x00, 0xC0),  // r1 = RAM
        ldr_literal(2, 0x04, 0xC4),  // r2 = RAM (BX target)
        ldr_literal(3, 0x08, 0xC8),  // r3 = MOV r0,#1
        ldr_literal(4, 0x0C, 0xCC),  // r4 = BX lr
        str_imm(3, 1, 0),            // RAM[0] = MOV r0,#1
        str_imm(4, 1, 4),            // RAM[1] = BX lr
        ldr_literal(14, 0x18, 0xD0), // lr = RET1
        0xE3A0_0000,                 // MOV r0,#0
        0xE350_0000,                 // CMP r0,#0
        bne(0x24, RET1),             // false, still discovers RET1
        bx(2),                       // enter RAM
        0xE350_0001,                 // RET1: CMP r0,#1
        ldr_literal(3, 0x30, 0xD4),  // r3 = MOV r0,#2
        str_imm(3, 1, 0),            // patch RAM[0]
        ldr_literal(14, 0x38, 0xD8), // lr = RET2
        bne(0x3C, RET2),             // false, still discovers RET2
        bx(2),                       // revisit RAM
        0xE350_0002,                 // RET2: CMP r0,#2
        ldr_literal(4, 0x48, 0xDC),  // r4 = DISPCNT
        ldr_literal(5, 0x4C, 0xE0),  // r5 = 0x0403
        0xE1C4_50B0,                 // STRH r5,[r4]
        0xEAFF_FFFE,                 // B .
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    let literals = [
        RAM,
        RAM,
        0xE3A0_0001,
        0xE12F_FF1E,
        RET1,
        0xE3A0_0002,
        RET2,
        DISPCNT_ADDR,
        0x0000_0403,
    ];
    for (index, word) in literals.iter().enumerate() {
        let offset = 0xC0 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA8].copy_from_slice(b"GBA-RAM\0");
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).expect("synthetic RAM fixture")
}

#[test]
fn native_ram_self_modification_falls_back_four_times_and_sees_the_patch() {
    let cart = fixture();
    let artifact =
        aot::discover(&cart, ENTRY, false, aot::Limits::default()).expect("discover RAM fixture");

    let block_addrs: Vec<u32> = artifact.blocks.iter().map(|block| block.addr).collect();
    assert!(block_addrs.contains(&ENTRY));
    assert!(block_addrs.contains(&RET1));
    assert!(block_addrs.contains(&RET2));
    assert!(
        artifact
            .blocks
            .iter()
            .all(|block| cart.rom_offset(block.addr).is_some()),
        "no RAM block may be translated: {block_addrs:X?}"
    );
    assert_eq!(artifact.stats.dynamic_bx, 2);
    assert_eq!(artifact.stats.decode_stops, 0);

    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit RAM fixture");
    assert!(source.contains("fn block_0800002c_arm"));
    assert!(source.contains("fn block_08000044_arm"));
    assert!(!source.contains("block_02000000_arm"));
    assert!(!source.contains("ROM_BYTES"));
    assert!(!source.contains("GBA-RAM"));
    assert!(source.contains("interpret::step(&mut cpu, &mut bus)?;"));
    assert!(source.contains("bus.write32(address & !3, source)?;"));

    let mut expected_cpu = Cpu::direct_entry();
    expected_cpu.set_pc(ENTRY);
    expected_cpu.set_thumb(false);
    let mut expected_bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut expected_cpu, &mut expected_bus, BUDGET)
        .expect("reference RAM run");
    assert_eq!(used, BUDGET);
    assert_eq!(expected_cpu.pc(), FINISH);
    assert_eq!(expected_cpu.reg(0), 2);
    assert_eq!(expected_cpu.reg(1), RAM);
    assert_eq!(expected_cpu.reg(2), RAM);
    assert_eq!(expected_cpu.reg(3), 0xE3A0_0002);
    assert_eq!(expected_cpu.reg(4), DISPCNT_ADDR);
    assert_eq!(expected_cpu.reg(5), 0x0403);
    assert_eq!(expected_cpu.reg(14), RET2);
    assert_eq!(expected_cpu.reg(13), USER_STACK);
    assert_eq!(expected_cpu.cpsr(), 0x6000_00DF); // Z=1, C=1 from CMP r0,#2
    assert!(!expected_cpu.thumb());
    assert_eq!(expected_bus.read32(RAM), Ok(0xE3A0_0002));
    assert_eq!(expected_bus.read32(RAM + 4), Ok(0xE12F_FF1E));
    assert_eq!(expected_bus.dispcnt(), 0x0403);

    let temp = TempDir::new("fixture");
    let path = temp.path().join("fixture.gba");
    std::fs::write(&path, cart.bytes()).expect("write fixture");
    let harness =
        NativeHarness::build(&source, &temp.path().join("harness")).expect("build RAM harness");
    let report = harness.run(&path).expect("run RAM harness");

    assert_eq!(report.budget_used, BUDGET);
    assert_eq!(report.budget_remaining, 0);
    assert_eq!(report.fallbacks, 4, "two RAM instructions on each visit");
    assert_eq!(report.pc, FINISH);
    assert_eq!(report.cpsr, 0x6000_00DF);
    assert!(!report.thumb);
    assert_eq!(report.regs[0], 2);
    assert_eq!(report.regs[1], RAM);
    assert_eq!(report.regs[2], RAM);
    assert_eq!(report.regs[3], 0xE3A0_0002);
    assert_eq!(report.regs[4], DISPCNT_ADDR);
    assert_eq!(report.regs[5], 0x0403);
    assert_eq!(report.regs[13], USER_STACK);
    assert_eq!(report.regs[14], RET2);
    assert_eq!(report.regs[15], FINISH);
    assert!(
        report
            .matches_reference(&expected_cpu, &expected_bus, BUDGET)
            .expect("compare native/reference")
    );

    // Independent expected bus: only the two RAM words and DISPCNT were
    // written. The second RAM word is unchanged by the patch.
    let mut expected_state = Bus::new(cart.clone());
    expected_state
        .write32(RAM, 0xE3A0_0002)
        .expect("patched RAM word");
    expected_state
        .write32(RAM + 4, 0xE12F_FF1E)
        .expect("RAM return word");
    expected_state
        .write16(DISPCNT_ADDR, 0x0403)
        .expect("DISPCNT");
    assert_eq!(
        report.bus_sha256,
        sha256::hex(&expected_state.state_digest())
    );
    assert_eq!(
        report.vram_sha256,
        sha256::hex(&expected_state.vram_digest())
    );
    assert_eq!(report.rom_sha256, cart.sha256_hex());

    // External input and digest binding are unchanged.
    let no_rom = Command::new(harness.executable())
        .output()
        .expect("run without arguments");
    assert!(!no_rom.status.success());
    assert!(String::from_utf8_lossy(&no_rom.stderr).contains("usage: harness --rom PATH"));

    let wrong = temp.path().join("wrong.gba");
    std::fs::write(&wrong, demo::fallback_cartridge().bytes()).expect("write wrong fixture");
    let error = harness.run(&wrong).expect_err("wrong ROM must be rejected");
    assert!(format!("{error}").contains("ROM mismatch"), "{error}");
}
