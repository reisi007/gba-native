//! Native-AOT fallback keeps the reference instruction-boundary events.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::display;
use gba_native::interpret;
use gba_native::ir::Psr;
use gba_native::sha256;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-aot-fallback-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("create unique temporary directory");
        Self(path)
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
    let offset = target - base;
    assert!(offset < 4096);
    0xE59F_0000 | (rd << 12) | offset
}

fn fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(0, 0x00, 0x40), // r0 = EWRAM+0x100
        ldr_literal(1, 0x04, 0x44), // r1 = dynamic fallback target
        0xE580_1000,                // STR r1,[r0]
        ldr_literal(2, 0x0C, 0x48), // r2 = EWRAM+0x100
        0xE592_0000,                // LDR r0,[r2] (runtime, not discoverable)
        0xE12F_FF10,                // BX r0 (dynamic target)
        0xEAFF_FFFE,                // B . at ROM+0x18 if reached linearly
        0xEAFF_FFFE,                // B . at ROM+0x1C if reached linearly
        0xEAFF_FFFE,                // B . at ROM+0x20: fallback target
    ];
    for (index, word) in words.iter().enumerate() {
        let offset = index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x40..0x44].copy_from_slice(&(EWRAM_BASE + 0x100).to_le_bytes());
    rom[0x44..0x48].copy_from_slice(&0x0800_0020u32.to_le_bytes());
    rom[0x48..0x4C].copy_from_slice(&(EWRAM_BASE + 0x100).to_le_bytes());
    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA8].copy_from_slice(b"GBA-FBK\0");
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).expect("synthetic fallback fixture")
}

fn irq_fixture() -> Cartridge {
    const HANDLER: u32 = 0x0800_0100;
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(0, 0x00, 0xC0), // r0 = IRQ handler pointer
        ldr_literal(1, 0x04, 0xC4), // r1 = handler
        0xE580_1000,                // STR r1,[r0]
        ldr_literal(0, 0x0C, 0xC8), // r0 = IE
        0xE3A0_1001,                // MOV r1,#1 (VBlank)
        0xE1C0_10B0,                // STRH r1,[r0]
        ldr_literal(0, 0x18, 0xCC), // r0 = IME
        0xE1C0_10B0,                // STRH r1,[r0]
        0xE321_F05F,                // MSR CPSR_c,#0x5F (unmask IRQ)
        0xEAFF_FFFE,                // B . until the modeled VBlank IRQ
    ];
    for (index, word) in words.iter().enumerate() {
        let offset = index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    for (index, word) in [0x0300_7FFCu32, HANDLER, 0x0400_0200, 0x0400_0208]
        .iter()
        .enumerate()
    {
        let offset = 0xC0 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x100..0x104].copy_from_slice(&0xE282_2001u32.to_le_bytes()); // ADD r2,r2,#1
    rom[0x104..0x108].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes()); // B .
    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA8].copy_from_slice(b"GBA-IRQ\0");
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).expect("synthetic IRQ fixture")
}

#[test]
fn native_unknown_target_ticks_guest_instructions_like_the_reference() {
    let cart = fixture();
    let artifact = aot::discover(&cart, 0x0800_0000, false, aot::Limits::default())
        .expect("discover dynamic fallback fixture");
    assert_eq!(artifact.stats.instructions, 6);
    assert_eq!(artifact.stats.blocks, 1);

    // Reach the active-to-VBlank boundary. Six instructions are translated;
    // every later instruction executes through the interpreter fallback.
    // Bus design: 280,896 cycles / 3 nominal cycles per instruction, with
    // VBlank occupying one tenth of the frame (integer truncation).
    let frame_instructions = 280_896 / 3;
    let budget = frame_instructions - frame_instructions / 10;
    let source = aot::emit_native_module(&cart, &artifact, budget).expect("emit fallback fixture");
    let temp = TempDir::new();
    let rom_path = temp.0.join("fixture.gba");
    std::fs::write(&rom_path, cart.bytes()).expect("write synthetic fixture");
    let harness =
        NativeHarness::build(&source, &temp.0.join("harness")).expect("build generated harness");
    let report = harness.run(&rom_path).expect("run generated harness");

    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(0x0800_0000);
    let mut bus = Bus::new(cart);
    let used = interpret::run_budget(&mut cpu, &mut bus, budget).expect("reference run");
    assert_eq!(used, budget);
    assert_eq!(cpu.pc(), 0x0800_0020);
    assert_eq!(
        bus.read16(0x0400_0006),
        Ok(160),
        "the final instruction enters modeled VBlank"
    );
    assert_eq!(report.budget_used, budget);
    assert_eq!(report.fallbacks, budget - 6);
    assert_eq!(report.pc, cpu.pc());
    assert_eq!(report.bus_sha256, sha256::hex(&bus.state_digest()));
    assert_eq!(
        report.image_sha256,
        display::snapshot(&bus).unwrap().sha256_hex()
    );
    assert!(
        report
            .matches_reference(&cpu, &bus, used)
            .expect("native/reference parity")
    );
}

#[test]
fn translated_aot_delivers_pending_irq_before_fallback_handler_step() {
    const HANDLER: u32 = 0x0800_0100;
    let cart = irq_fixture();
    let artifact = aot::discover(&cart, 0x0800_0000, false, aot::Limits::default())
        .expect("discover IRQ fixture");
    let frame_instructions = 280_896 / 3;
    let active_instructions = frame_instructions - frame_instructions / 10;
    let budget = active_instructions + 1;
    let source = aot::emit_native_module(&cart, &artifact, budget).expect("emit IRQ fixture");
    let temp = TempDir::new();
    let rom_path = temp.0.join("fixture.gba");
    std::fs::write(&rom_path, cart.bytes()).expect("write synthetic fixture");
    let harness = NativeHarness::build(&source, &temp.0.join("harness"))
        .expect("build generated IRQ harness");
    let report = harness.run(&rom_path).expect("run generated IRQ harness");

    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(0x0800_0000);
    let mut bus = Bus::new(cart);
    assert_eq!(
        interpret::run_budget(&mut cpu, &mut bus, budget),
        Ok(budget)
    );
    assert_eq!(cpu.pc(), HANDLER + 4);
    assert_eq!(cpu.cpsr() & 0xFF, 0xD2, "IRQ mode, ARM, I set, F preserved");
    assert_eq!(
        cpu.reg(14),
        0x0000_0138,
        "LR_irq identifies the behavior-only dispatcher continuation"
    );
    assert_eq!(
        cpu.read_psr(Psr::Spsr).expect("IRQ SPSR exists"),
        0x5F,
        "IRQ SPSR preserves the pre-interrupt CPSR"
    );
    assert_eq!(cpu.reg(2), 1);
    assert_eq!(
        bus.read16(0x0400_0202),
        Ok(1),
        "VBlank request remains pending"
    );
    assert_eq!(report.fallbacks, 1, "the undiscovered handler runs fresh");
    assert!(
        report
            .matches_reference(&cpu, &bus, budget)
            .expect("native/reference parity")
    );
}
