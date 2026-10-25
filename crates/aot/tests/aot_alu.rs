//! Native-AOT integration for the expanded ARM ALU, shifted operands, ARM BL
//! and banked register state.
//!
//! The synthetic fixture uses only our own instruction words. Its final state is
//! hand-derived (including the inactive IRQ stack bank), the generated harness
//! is compiled and executed, and the result is compared with an independently
//! constructed `Cpu`/`Bus` rather than only with the reference interpreter.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, DISPCNT_ADDR};
use gba_native::cart::Cartridge;
use gba_native::cpu::{Cpu, Mode};
use gba_native::display;
use gba_native::interpret;
use gba_native::ir::{PSR_FIELD_C, Psr};
use gba_native::sha256;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-alu-aot-{tag}-{}-{sequence}",
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

const ENTRY: u32 = 0x0800_0000;
const FINAL_PC: u32 = 0x0800_005C;
const BUDGET: u64 = 26;

fn ldr_literal(rd: u32, instruction_addr: u32, target: u32) -> u32 {
    let base = instruction_addr + 8;
    let difference = target - base;
    assert_eq!(difference % 4, 0);
    assert!(difference < 4096);
    0xE59F_0000 | (rd << 12) | difference
}

fn branch(addr: u32, target: u32, link: bool) -> u32 {
    let difference = target as i64 - (addr as i64 + 8);
    assert_eq!(difference % 4, 0);
    let imm24 = ((difference >> 2) as u32) & 0x00FF_FFFF;
    0xE000_0000 | if link { 0x0B00_0000 } else { 0x0A00_0000 } | imm24
}

fn data_reg(opcode: u32, set_flags: bool, rn: u32, rd: u32, rm: u32) -> u32 {
    0xE000_0000 | (opcode << 21) | ((set_flags as u32) << 20) | (rn << 16) | (rd << 12) | rm
}

fn data_shift(
    opcode: u32,
    set_flags: bool,
    rn: u32,
    rd: u32,
    rm: u32,
    kind: u32,
    amount: u32,
) -> u32 {
    0xE000_0000
        | (opcode << 21)
        | ((set_flags as u32) << 20)
        | (rn << 16)
        | (rd << 12)
        | (amount << 7)
        | (kind << 5)
        | rm
}

fn fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        ldr_literal(0, 0x000, 0x100),
        ldr_literal(1, 0x004, 0x104),
        0xE1C0_10B0,                          // STRH r1,[r0]
        0xE3A0_200F,                          // MOV r2,#0x0F
        0xE3A0_30F0,                          // MOV r3,#0xF0
        data_reg(0, false, 2, 4, 3),          // AND r4,r2,r3
        data_reg(1, false, 2, 5, 3),          // EOR r5,r2,r3
        data_reg(12, false, 2, 6, 3),         // ORR r6,r2,r3
        data_reg(14, false, 3, 7, 2),         // BIC r7,r3,r2
        data_reg(15, false, 0, 8, 2),         // MVN r8,r2
        data_shift(13, false, 0, 9, 2, 0, 4), // MOV r9,r2,LSL#4
        data_shift(4, false, 9, 10, 3, 1, 1), // ADD r10,r9,r3,LSR#1
        data_reg(2, true, 10, 11, 2),         // SUBS r11,r10,r2
        data_reg(5, true, 11, 12, 2),         // ADCS r12,r11,r2
        branch(0x038, 0x060, true),           // BL callee
        0xE3A0_1077,                          // MOV r1,#0x77
        ldr_literal(0, 0x040, 0x108),
        0xE121_F000, // MSR CPSR_c,r0 (IRQ)
        ldr_literal(1, 0x048, 0x10C),
        0xE1A0_D001, // MOV r13,r1 (IRQ bank)
        ldr_literal(0, 0x050, 0x110),
        0xE121_F000, // MSR CPSR_c,r0 (SYSTEM)
        0xE1A0_000D, // MOV r0,r13 (SYSTEM SP restored)
        branch(0x05C, 0x05C, false),
        0xE3A0_0099, // callee: MOV r0,#0x99
        0xE12F_FF1E, // BX lr
    ];
    for (index, word) in words.iter().enumerate() {
        let offset = index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    for (index, word) in [
        DISPCNT_ADDR,
        0x0000_0403,
        0x0000_0012,
        0x1234_0000,
        0x0000_001F,
    ]
    .iter()
    .enumerate()
    {
        let offset = 0x100 + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    Cartridge::from_bytes(rom).expect("synthetic ALU fixture")
}

fn expected_cpu() -> Cpu {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    let registers = [
        (0u8, 0x0300_7F00),
        (1, 0x1234_0000),
        (2, 0x0000_000F),
        (3, 0x0000_00F0),
        (4, 0x0000_0000),
        (5, 0x0000_00FF),
        (6, 0x0000_00FF),
        (7, 0x0000_00F0),
        (8, 0xFFFF_FFF0),
        (9, 0x0000_00F0),
        (10, 0x0000_0168),
        (11, 0x0000_0159),
        (12, 0x0000_0169),
    ];
    for (index, value) in registers {
        cpu.set_reg(index, value);
    }
    cpu.set_nzcv(false, false, false, false);
    cpu.set_pc(FINAL_PC);
    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, Mode::Irq.bits())
        .expect("enter IRQ");
    cpu.set_reg(13, 0x1234_0000);
    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, Mode::System.bits())
        .expect("return to SYSTEM");
    cpu.set_reg(14, 0x0800_003C);
    cpu
}

#[test]
fn native_alu_shift_bl_fixture_matches_hand_expected_state_and_bus() {
    let cart = fixture();
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default()).expect("discover");
    assert_eq!(artifact.stats.bl_calls, 1);
    assert_eq!(artifact.stats.dynamic_bx, 1);

    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit");
    let blocks = source.split("fn main()").next().expect("block section");
    assert!(!blocks.contains("interpret::step"));
    assert!(!blocks.contains("decode_arm("));
    assert!(source.contains("gba_native::semantics::operand"));
    assert!(source.contains("gba_native::cpu::adc_flags"));
    assert!(source.contains("gba_native::ir::ShiftKind::Lsr"));
    assert!(source.contains("cpu.set_reg(14, 0x0800003C)"));
    assert!(source.contains("cpu.set_reg(15, 0x08000060)"));
    assert!(!source.contains("ROM_BYTES"));

    let temp = TempDir::new("fixture");
    let fixture_path = temp.path().join("fixture.gba");
    std::fs::write(&fixture_path, cart.bytes()).expect("write fixture");
    let harness =
        NativeHarness::build(&source, &temp.path().join("harness")).expect("build ALU harness");
    let report = harness.run(&fixture_path).expect("run ALU harness");

    let expected = expected_cpu();
    assert_eq!(report.pc, FINAL_PC);
    assert_eq!(report.cpsr, 0x0000_001F);
    assert_eq!(report.regs, {
        let mut regs = [0u32; 16];
        for (index, register) in regs.iter_mut().enumerate() {
            *register = expected.reg(index as u8);
        }
        regs
    });
    assert_eq!(
        report.cpu_sha256,
        sha256::hex(&expected.state_digest()),
        "inactive IRQ bank must match the independently constructed state"
    );
    assert_eq!(report.budget_used, BUDGET);
    assert_eq!(report.budget_remaining, 0);
    assert_eq!(report.fallbacks, 0);

    let mut expected_bus = Bus::new(cart.clone());
    expected_bus
        .write16(DISPCNT_ADDR, 0x0403)
        .expect("expected DISPCNT write");
    assert_eq!(report.bus_sha256, sha256::hex(&expected_bus.state_digest()));
    assert_eq!(report.vram_sha256, sha256::hex(&expected_bus.vram_digest()));
    let image = display::snapshot(&expected_bus).expect("expected image");
    assert_eq!(report.image_sha256, image.sha256_hex());
    assert_eq!(report.image_width, image.width);
    assert_eq!(report.image_height, image.height);

    let mut reference_cpu = Cpu::direct_entry();
    reference_cpu.initialize_bios_stacks();
    reference_cpu.set_pc(ENTRY);
    let mut reference_bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut reference_cpu, &mut reference_bus, BUDGET)
        .expect("reference run");
    assert!(
        report
            .matches_reference(&reference_cpu, &reference_bus, used)
            .expect("compare")
    );
}

#[test]
fn native_dynamic_pc_write_falls_back_fresh_and_binds_the_return_state() {
    // MOV pc,r0 computes a ROM target that discovery deliberately does not
    // follow. The target block must execute through a fresh reference decode.
    let mut rom = vec![0u8; 0x200];
    let words: [u32; 3] = [
        0xE59F_0010, // 0x000 LDR r0,[pc,#0x10] -> 0x018
        0xE1A0_F000, // 0x004 MOV pc,r0
        0xEAFF_FFFE, // 0x008 B .
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x018..0x01C].copy_from_slice(&0x0800_0020u32.to_le_bytes());
    rom[0x020..0x024].copy_from_slice(&0xE3A0_2055u32.to_le_bytes()); // MOV r2,#0x55
    rom[0x024..0x028].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes()); // B .
    let cart = Cartridge::from_bytes(rom).expect("synthetic PC fixture");
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default()).expect("discover");
    assert_eq!(artifact.stats.pc_writes, 1);
    assert!(
        artifact
            .blocks
            .iter()
            .all(|block| block.addr != 0x0800_0020)
    );

    let source = aot::emit_native_module(&cart, &artifact, 4).expect("emit");
    let temp = TempDir::new("pc-fallback");
    let fixture_path = temp.path().join("fixture.gba");
    std::fs::write(&fixture_path, cart.bytes()).expect("write fixture");
    let harness = NativeHarness::build(&source, &temp.path().join("harness"))
        .expect("build fallback harness");
    let report = harness.run(&fixture_path).expect("run fallback harness");
    assert_eq!(
        report.fallbacks, 2,
        "target MOV and terminal B are fresh decodes"
    );
    assert_eq!(report.regs[2], 0x55);
    assert_eq!(report.pc, 0x0800_0024);
    assert_eq!(report.budget_used, 4);
}
