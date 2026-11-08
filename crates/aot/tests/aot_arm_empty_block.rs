//! Native ARMv4 empty-list LDM/STM behavior on a synthetic cartridge.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::display::{HEIGHT, Image, WIDTH};
use gba_native::interpret;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-aot-arm-empty-block-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("create owned temporary directory");
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
    0xE59F_0000 | (rd << 12) | difference
}

fn fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(0, 0x00, 0x40), // r0 = EWRAM+0x200
        0xE8A0_0000,                // STMIA r0!,{}: PC+12 transfer, +0x40 writeback
        ldr_literal(1, 0x08, 0x44), // r1 = EWRAM+0x204
        0xE931_0000,                // LDMDB r1!,{}: PC at base-4, -0x40 writeback
        0xEAFF_FFFE,                // B .; the bounded run ends before this
    ];
    for (index, word) in words.iter().enumerate() {
        let offset = index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x40..0x44].copy_from_slice(&(EWRAM_BASE + 0x200).to_le_bytes());
    rom[0x44..0x48].copy_from_slice(&(EWRAM_BASE + 0x204).to_le_bytes());

    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA0 + b"ARM-EMPTY".len()].copy_from_slice(b"ARM-EMPTY");
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).expect("synthetic ARM empty-list fixture")
}

#[test]
fn native_arm_empty_stmia_and_ldmdb_match_independent_state() {
    let cart = fixture();
    let artifact = aot::discover(&cart, 0x0800_0000, false, aot::Limits::default())
        .expect("discover empty-list fixture");
    assert_eq!(artifact.stats.instructions, 4);
    assert_eq!(artifact.stats.blocks, 1);

    let budget = 4;
    let source = aot::emit_native_module(&cart, &artifact, budget)
        .expect("emit ARM empty-list native blocks");
    let translated = source
        .split("pub fn run_program")
        .next()
        .expect("generated block section");
    assert!(!translated.contains("interpret::step"));
    assert!(translated.contains("block_start(transfer, 1u32, false, true)"));
    assert!(
        translated.contains("block_start(transfer, 1u32, true, false)"),
        "generated block should preserve LDMDB transfer address:\n{translated}"
    );
    assert!(translated.contains("block_written_back(base, 16u32, true)"));
    assert!(translated.contains("block_written_back(base, 16u32, false)"));
    assert!(translated.contains("bus.write32(start.wrapping_add(0x0u32), 0x08000010u32)?;"));

    let mut expected_cpu = Cpu::direct_entry();
    expected_cpu.initialize_bios_stacks();
    expected_cpu.set_pc(0x0800_0000);
    expected_cpu.set_thumb(false);
    let mut expected_bus = Bus::new(cart.clone());
    assert_eq!(
        interpret::run_budget(&mut expected_cpu, &mut expected_bus, budget),
        Ok(budget)
    );
    assert_eq!(expected_cpu.pc(), 0x0800_0010);
    assert_eq!(expected_cpu.reg(0), EWRAM_BASE + 0x240);
    assert_eq!(expected_cpu.reg(1), EWRAM_BASE + 0x1C4);
    assert_eq!(expected_bus.read32(EWRAM_BASE + 0x200), Ok(0x0800_0010));

    let temp = TempDir::new();
    let rom_path = temp.path().join("fixture.gba");
    std::fs::write(&rom_path, cart.bytes()).expect("write owned fixture");
    let harness = NativeHarness::build(&source, &temp.path().join("harness"))
        .expect("build generated native harness");
    let report = harness.run(&rom_path).expect("run generated harness");
    assert_eq!(report.budget_used, budget);
    assert_eq!(report.native_instructions, budget);
    assert_eq!(report.fallbacks, 0);
    assert_eq!(report.pc, expected_cpu.pc());
    assert_eq!(report.regs[0], expected_cpu.reg(0));
    assert_eq!(report.regs[1], expected_cpu.reg(1));
    assert!(
        report
            .matches_reference(&expected_cpu, &expected_bus, budget)
            .expect("native/reference parity")
    );

    let expected_image =
        Image::new(WIDTH, HEIGHT, vec![0; (WIDTH * HEIGHT) as usize]).expect("expected image");
    assert_eq!(report.image_sha256, expected_image.sha256_hex());
}
