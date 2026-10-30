//! Synthetic native-AOT parity for the bounded GBA IRQ dispatcher model.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, VBLANK_FIRST_LINE, instructions_until_line};
use gba_native::cart::Cartridge;
use gba_native::cpu::{Cpu, IRQ_STACK, Mode};
use gba_native::interpret;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

const ENTRY: u32 = 0x0800_0000;
const WAIT_LOOP: u32 = 0x0800_0030;
const HANDLER: u32 = 0x0800_0100;
const IRQ_SP: u32 = IRQ_STACK - 24;
const ACTIVE_INSTRUCTIONS: u64 = instructions_until_line(VBLANK_FIRST_LINE);
const HANDLER_INSTRUCTIONS: u64 = 9;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-aot-irq-{}-{sequence}",
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
    let main: [u32; 13] = [
        ldr_literal(0, 0x00, 0xC0), // IRQ handler-pointer address
        ldr_literal(1, 0x04, 0xC4), // handler address
        0xE580_1000,                // STR r1,[r0]
        ldr_literal(0, 0x0C, 0xC8), // IE address
        0xE3A0_1001,                // MOV r1,#1 (VBlank)
        0xE1C0_10B0,                // STRH r1,[r0]
        ldr_literal(0, 0x18, 0xCC), // IME address
        0xE1C0_10B0,                // STRH r1,[r0]
        0xE3A0_2022,                // MOV r2,#0x22
        0xE3A0_3033,                // MOV r3,#0x33
        0xE3A0_C044,                // MOV r12,#0x44
        0xE321_F05F,                // MSR CPSR_c,#0x5F (unmask IRQ)
        0xEAFF_FFFE,                // B . at the wait loop
    ];
    for (index, word) in main.iter().enumerate() {
        let offset = index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    // The handler acknowledges VBlank, clobbers r0-r3/r12, and returns via
    // BX LR. It is synthetic guest code; the documented BIOS sequence is never
    // copied, emitted, or fetched.
    let handler: [u32; 9] = [
        0xE59F_001C, // LDR r0,[pc,#0x1C] -> IF literal at HANDLER+0x24
        0xE3A0_1001, // MOV r1,#1
        0xE1C0_10B0, // STRH r1,[r0]
        0xE3A0_0000, // MOV r0,#0
        0xE3A0_1000, // MOV r1,#0
        0xE3A0_2000, // MOV r2,#0
        0xE3A0_3000, // MOV r3,#0
        0xE3A0_C000, // MOV r12,#0
        0xE12F_FF1E, // BX lr
    ];
    for (index, word) in handler.iter().enumerate() {
        let offset = HANDLER as usize - ENTRY as usize + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    let if_literal = HANDLER as usize - ENTRY as usize + 0x24;
    rom[if_literal..if_literal + 4].copy_from_slice(&0x0400_0202u32.to_le_bytes());

    for (index, word) in [0x0300_7FFCu32, HANDLER, 0x0400_0200, 0x0400_0208]
        .iter()
        .enumerate()
    {
        let offset = 0xC0 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
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

fn reference_state(cart: Cartridge, budget: u64) -> (Cpu, Bus) {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(ENTRY);
    let mut bus = Bus::new(cart);
    assert_eq!(
        interpret::run_budget(&mut cpu, &mut bus, budget),
        Ok(budget)
    );
    (cpu, bus)
}

#[test]
fn native_irq_dispatch_and_return_match_reference_with_exact_fallbacks() {
    let cart = fixture();
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default())
        .expect("discover main IRQ fixture");
    assert!(
        artifact.blocks.iter().all(|block| block.addr != HANDLER),
        "the dynamic IRQ handler remains an interpreter fallback"
    );
    let budget = ACTIVE_INSTRUCTIONS + HANDLER_INSTRUCTIONS + 2;
    let source = aot::emit_native_module(&cart, &artifact, budget).expect("emit IRQ fixture");
    let temp = TempDir::new();
    let rom_path = temp.0.join("fixture.gba");
    std::fs::write(&rom_path, cart.bytes()).expect("write synthetic fixture");
    let harness = NativeHarness::build(&source, &temp.0.join("harness"))
        .expect("build generated IRQ harness");
    let report = harness.run(&rom_path).expect("run generated IRQ harness");

    let (cpu, bus) = reference_state(cart, budget);
    assert_eq!(cpu.mode(), Mode::System);
    assert_eq!(cpu.pc(), WAIT_LOOP, "return resumes the interrupted branch");
    assert_eq!(cpu.cpsr() & 0xFF, 0x5F, "CPSR restored from SPSR_irq");
    assert_eq!(cpu.reg(2), 0x22);
    assert_eq!(cpu.reg(3), 0x33);
    assert_eq!(cpu.reg(12), 0x44);
    assert_eq!(
        bus.read16(0x0400_0202),
        Ok(0),
        "handler acknowledged VBlank"
    );
    assert_eq!(
        bus.read16(0x0400_0006),
        Ok(160),
        "the modeled dispatcher runs on VBlank line 160"
    );
    let observed_frame = std::array::from_fn(|word| {
        bus.read32(IRQ_SP + word as u32 * 4)
            .expect("saved IRQ frame word")
    });
    assert_eq!(
        observed_frame,
        [0x0400_0208, 1, 0x22, 0x33, 0x44, WAIT_LOOP + 4],
        "the saved full-descending frame is restored in documented order"
    );
    assert_eq!(report.budget_used, budget);
    assert_eq!(report.fallbacks, HANDLER_INSTRUCTIONS);
    assert_eq!(report.irq_returns, 1);
    assert_eq!(
        report.native_instructions + report.fallbacks + report.irq_returns,
        budget,
        "native, fallback and modelled IRQ-return counters must close the budget"
    );
    assert!(
        report
            .matches_reference(&cpu, &bus, budget)
            .expect("native/reference state and bus parity")
    );
}
