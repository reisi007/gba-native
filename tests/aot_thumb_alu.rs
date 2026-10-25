//! Native-AOT integration for Thumb register ALU, shifts, high registers and
//! the two-half long branch.
//!
//! The BL callee is deliberately not statically discovered: the second half is a
//! dynamic LR target, so those three instructions use fresh reference fallbacks
//! while the first/return blocks are emitted native. Final state, bus, image and
//! the exact budget are independently hand-derived.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::aot::{self, NativeHarness};
use gba_native::bus::{Bus, DISPCNT_ADDR};
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::display;
use gba_native::interpret;
use gba_native::sha256;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-thumb-aot-{tag}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const ENTRY: u32 = 0x0800_0100;
const FINAL_PC: u32 = 0x0800_012A;
const BUDGET: u64 = 25;

fn ldr_literal(rd: u16, instruction_addr: u32, target: u32) -> u16 {
    let base = (instruction_addr + 4) & !3;
    let difference = target - base;
    assert_eq!(difference % 4, 0);
    assert!(difference / 4 < 256);
    0x4800 | (rd << 8) | (difference / 4) as u16
}

fn fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let halfwords = [
        ldr_literal(0, 0x100, 0x180),
        ldr_literal(1, 0x102, 0x184),
        0x8001, // STRH r1,[r0]
        0x22F0, // MOV r2,#0xF0
        0x230F, // MOV r3,#0x0F
        0x401A, // AND r2,r3
        0x2400, // MOV r4,#0
        0x431C, // ORR r4,r3
        0x2500, // MOV r5,#0
        0x405D, // EOR r5,r3
        0x43DE, // MVN r6,r3
        0x4277, // NEG r7,r6
        0x013F, // LSL r7,r7,#4
        0x3701, // ADD r7,#1
        0x2501, // MOV r5,#1
        0x41AC, // SBC r4,r5
        0x46B8, // MOV r8,r7
        0x46C1, // MOV r9,r8
        0xF000, // BL first half
        0xF80C, // BL second half (+0x18)
        0x4683, // MOV r11,r0
        0xE7FE, // B .
    ];
    for (index, halfword) in halfwords.iter().enumerate() {
        let offset = 0x100 + index * 2;
        rom[offset..offset + 2].copy_from_slice(&halfword.to_le_bytes());
    }
    for (offset, halfword) in [(0x140usize, 0x2099u16), (0x142, 0x3001), (0x144, 0x4770)] {
        rom[offset..offset + 2].copy_from_slice(&halfword.to_le_bytes());
    }
    rom[0x180..0x184].copy_from_slice(&DISPCNT_ADDR.to_le_bytes());
    rom[0x184..0x188].copy_from_slice(&0x0000_0403u32.to_le_bytes());
    Cartridge::from_bytes(rom).expect("synthetic Thumb ALU fixture")
}

fn expected_cpu() -> Cpu {
    let mut cpu = Cpu::direct_entry();
    for (index, value) in [
        (0u8, 0x0000_009A),
        (1, 0x0000_0403),
        (2, 0x0000_0000),
        (3, 0x0000_000F),
        (4, 0x0000_000D),
        (5, 0x0000_0001),
        (6, 0xFFFF_FFF0),
        (7, 0x0000_0101),
        (8, 0x0000_0101),
        (9, 0x0000_0101),
        (10, 0x0000_0000),
        (11, 0x0000_009A),
        (12, 0x0000_0000),
        (14, 0x0800_0129),
    ] {
        cpu.set_reg(index, value);
    }
    cpu.set_nzcv(false, false, false, false);
    cpu.set_pc(FINAL_PC);
    cpu.set_thumb(true);
    cpu
}

#[test]
fn native_thumb_alu_shift_and_bl_halves_match_hand_expected_state() {
    let cart = fixture();
    let artifact = aot::discover(&cart, ENTRY, true, aot::Limits::default()).expect("discover");
    assert_eq!(artifact.stats.dynamic_bl, 1);
    assert_eq!(artifact.stats.bl_returns, 1);
    assert!(
        artifact
            .blocks
            .iter()
            .all(|block| block.addr != 0x0800_0140)
    );

    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit");
    let blocks = source.split("fn main()").next().expect("block section");
    assert!(!blocks.contains("interpret::step"));
    assert!(source.contains("gba_native::semantics::operand"));
    assert!(source.contains("gba_native::cpu::sbc_flags"));
    assert!(source.contains("gba_native::ir::ShiftKind::Lsl"));
    assert!(source.contains("cpu.set_reg(14, 0x08000128u32)"));
    assert!(source.contains("cpu.reg(14).wrapping_add(0x00000018u32)"));
    assert!(source.contains("cpu.set_reg(14, 0x08000129u32)"));
    assert!(!source.contains("ROM_BYTES"));

    let temp = TempDir::new("fixture");
    let fixture_path = temp.path().join("fixture.gba");
    std::fs::write(&fixture_path, cart.bytes()).expect("write fixture");
    let harness =
        NativeHarness::build(&source, &temp.path().join("harness")).expect("build Thumb harness");
    let report = harness.run(&fixture_path).expect("run Thumb harness");

    let expected = expected_cpu();
    assert_eq!(report.pc, FINAL_PC);
    assert_eq!(report.cpsr, 0x0000_00FF);
    assert!(report.thumb);
    assert_eq!(report.regs[4], 0x0D);
    assert_eq!(report.regs[7], 0x101);
    assert_eq!(report.regs[9], 0x101);
    assert_eq!(report.regs[11], 0x9A);
    assert_eq!(report.regs[14], 0x0800_0129);
    assert_eq!(report.budget_used, BUDGET);
    assert_eq!(report.budget_remaining, 0);
    assert_eq!(
        report.fallbacks, 3,
        "the dynamic BL callee is fresh-decoded"
    );
    assert_eq!(report.cpu_sha256, sha256::hex(&expected.state_digest()));

    let mut expected_bus = Bus::new(cart.clone());
    expected_bus
        .write16(DISPCNT_ADDR, 0x0403)
        .expect("expected DISPCNT write");
    assert_eq!(report.bus_sha256, sha256::hex(&expected_bus.state_digest()));
    assert_eq!(report.vram_sha256, sha256::hex(&expected_bus.vram_digest()));
    let image = display::snapshot(&expected_bus).expect("expected image");
    assert_eq!(report.image_sha256, image.sha256_hex());

    let mut reference_cpu = Cpu::direct_entry();
    reference_cpu.set_pc(ENTRY);
    reference_cpu.set_thumb(true);
    let mut reference_bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut reference_cpu, &mut reference_bus, BUDGET)
        .expect("reference run");
    assert_eq!(used, BUDGET);
    assert!(
        report
            .matches_reference(&reference_cpu, &reference_bus, used)
            .expect("compare")
    );

    // Exact mid-pair stop: 19 instructions end immediately after the first BL
    // half, leaving the intermediate LR and the second half at PC.
    let mid_budget = 19;
    let mid_source = aot::emit_native_module(&cart, &artifact, mid_budget).expect("emit mid");
    let mid_harness = NativeHarness::build(&mid_source, &temp.path().join("mid-harness"))
        .expect("build mid harness");
    let mid_report = mid_harness.run(&fixture_path).expect("run mid harness");
    assert_eq!(mid_report.budget_used, mid_budget);
    assert_eq!(mid_report.pc, 0x0800_0126);
    assert_eq!(mid_report.regs[14], 0x0800_0128, "intermediate BL LR");
    assert_eq!(mid_report.fallbacks, 0);
    let mut mid_cpu = Cpu::direct_entry();
    mid_cpu.set_pc(ENTRY);
    mid_cpu.set_thumb(true);
    let mut mid_bus = Bus::new(cart.clone());
    let mid_used =
        interpret::run_budget(&mut mid_cpu, &mut mid_bus, mid_budget).expect("mid reference run");
    assert!(
        mid_report
            .matches_reference(&mid_cpu, &mid_bus, mid_used)
            .expect("compare mid-pair state")
    );
}
