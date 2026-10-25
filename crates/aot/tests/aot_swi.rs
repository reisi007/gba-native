//! Direct native SWI emission for synthetic ARM/Thumb HLE calls.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::{Cpu, USER_STACK};
use gba_native::interpret;
use gba_native::sha256;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
const ENTRY: u32 = 0x0800_0000;
const SVC_FRAME: u32 = 0x0300_7FD0;
const BUDGET_AFTER_BOTH_SWIS: u64 = 14;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-aot-swi-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("owned temp directory");
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

fn ldr_literal(rd: u32, instruction: u32, target: u32) -> u32 {
    let base = instruction + 8;
    assert!(target >= base);
    0xE59F_0000 | (rd << 12) | (target - base)
}

fn swi_fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words = [
        0xE3A0_0023, // MOV r0,#35
        0xE3A0_1006, // MOV r1,#6
        0xEF06_0000, // SWI Div
        0xE1A0_4000, // MOV r4,r0 (preserve independent quotient oracle)
        0xE1A0_5001, // MOV r5,r1 (preserve independent remainder oracle)
        ldr_literal(0, ENTRY + 0x14, ENTRY + 0x80),
        ldr_literal(1, ENTRY + 0x18, ENTRY + 0x84),
        0xE580_1000, // STR r1,[r0] to seed the memory service
        ldr_literal(0, ENTRY + 0x20, ENTRY + 0x88),
        ldr_literal(1, ENTRY + 0x24, ENTRY + 0x8C),
        ldr_literal(2, ENTRY + 0x28, ENTRY + 0x90),
        ldr_literal(3, ENTRY + 0x2C, ENTRY + 0x94),
        0xE12F_FF13, // BX r3 into Thumb
    ];
    for (index, word) in words.iter().enumerate() {
        let offset = index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x34..0x36].copy_from_slice(&0xDF0Cu16.to_le_bytes()); // SWI CpuFastSet
    rom[0x36..0x38].copy_from_slice(&0xE7FEu16.to_le_bytes()); // B .
    for (offset, value) in [
        (0x80, EWRAM_BASE + 0x40),
        (0x84, 0xAABB_CCDD),
        (0x88, EWRAM_BASE + 0x40),
        (0x8C, EWRAM_BASE + 0x80),
        (0x90, 0x0100_0001),  // fill one request, rounded to eight words
        (0x94, ENTRY + 0x35), // Thumb entry address
    ] {
        rom[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    Cartridge::from_bytes(rom).expect("synthetic SWI fixture")
}

fn reference(cart: &Cartridge, budget: u64) -> (Cpu, Bus, u64) {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    let mut bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut cpu, &mut bus, budget).expect("reference execution");
    (cpu, bus, used)
}

fn independent_final_bus(cart: &Cartridge) -> Bus {
    let mut bus = Bus::new(cart.clone());
    bus.write32(EWRAM_BASE + 0x40, 0xAABB_CCDD)
        .expect("independent source write");
    for index in 0..8 {
        bus.write32(EWRAM_BASE + 0x80 + index * 4, 0xAABB_CCDD)
            .expect("independent rounded CpuFastSet endpoint");
    }
    bus.write32(SVC_FRAME, 0x0000_00FF)
        .expect("independent saved CPSR");
    bus.write32(SVC_FRAME + 4, 0)
        .expect("independent saved r11");
    bus.write32(SVC_FRAME + 8, 0)
        .expect("independent saved r12");
    bus.write32(SVC_FRAME + 12, ENTRY + 0x36)
        .expect("independent saved LR");
    bus
}

fn independent_final_cpu_digest() -> String {
    // Cpu::write_psr intentionally cannot manufacture an SPSR T-bit change.
    // Serialize the complete expected bank state directly so this oracle can
    // independently include SPSR_svc=0xFF after the Thumb SWI return.
    let mut bytes = Vec::with_capacity(158);
    for word in [
        // r0-r7 shared.
        EWRAM_BASE + 0x40,
        EWRAM_BASE + 0x80,
        0x0100_0001,
        ENTRY + 0x35,
        5,
        5,
        0,
        0,
        // Shared r8-r12 and FIQ-only r8-r12.
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        // USER/SYSTEM, FIQ, IRQ, SVC, ABT and UND SP banks.
        USER_STACK,
        0,
        0x0300_7FA0,
        0x0300_7FE0,
        0,
        0,
        // Matching LR banks, PC and final CPSR.
        0,
        0,
        0,
        ENTRY + 0x36,
        0,
        0,
        ENTRY + 0x36,
        0xFF,
    ] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for (present, value) in [
        (false, 0u32), // USER/SYSTEM has no SPSR
        (true, 0),     // SPSR_fiq
        (true, 0),     // SPSR_irq
        (true, 0xFF),  // SPSR_svc saves the interrupted Thumb CPSR
        (true, 0),     // SPSR_abt
        (true, 0),     // SPSR_und
    ] {
        bytes.push(u8::from(present));
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    sha256::hex(&sha256::digest(&bytes))
}

#[test]
fn discovery_emits_arm_and_thumb_swi_blocks_with_return_continuations() {
    let cart = swi_fixture();
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default()).expect("discover");
    assert_eq!(artifact.stats.swis, 2);
    assert_eq!(
        artifact.stats.pc_writes, 0,
        "SWI is not a register PC write"
    );
    assert!(
        artifact
            .blocks
            .iter()
            .any(|block| block.addr == ENTRY + 0x0C)
    );
    assert!(
        artifact
            .blocks
            .iter()
            .any(|block| block.addr == ENTRY + 0x36)
    );
    assert!(
        artifact
            .blocks
            .iter()
            .any(|block| block.addr == ENTRY + 0x34 && block.thumb)
    );

    let source = aot::emit_native_module(&cart, &artifact, BUDGET_AFTER_BOTH_SWIS)
        .expect("emit direct HLE calls");
    assert!(source.contains("gba_native::bios::execute_swi(0x00060000u32, false"));
    assert!(source.contains("gba_native::bios::execute_swi(0x0000000Cu32, true"));
    assert!(source.contains("cpu.initialize_bios_stacks();"));
    assert!(!source.contains("ROM_BYTES"));
    for block in [
        "fn block_08000000_arm",
        "fn block_0800000c_arm",
        "fn block_08000034_thumb",
    ] {
        let body = source
            .split_once(block)
            .expect("emitted block")
            .1
            .split_once("\n}\n")
            .expect("block ends")
            .0;
        assert!(!body.contains("interpret::step"), "{block} must be direct");
        assert!(
            !body.contains("interpret::execute"),
            "{block} must be direct"
        );
    }
}

#[test]
fn generated_native_swi_matches_independent_state_and_reference_at_budget_edges() {
    let cart = swi_fixture();
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default()).expect("discover");
    let temp = TempDir::new();
    let fixture = temp.path().join("fixture.gba");
    std::fs::write(&fixture, cart.bytes()).expect("external fixture ROM");

    let full_source =
        aot::emit_native_module(&cart, &artifact, BUDGET_AFTER_BOTH_SWIS).expect("full source");
    let full_harness =
        NativeHarness::build(&full_source, &temp.path().join("full")).expect("build full harness");
    let report = full_harness.run(&fixture).expect("run full harness");
    let (reference_cpu, reference_bus, used) = reference(&cart, BUDGET_AFTER_BOTH_SWIS);
    let expected_bus = independent_final_bus(&cart);
    let expected_regs = [
        EWRAM_BASE + 0x40,
        EWRAM_BASE + 0x80,
        0x0100_0001,
        ENTRY + 0x35,
        5,
        5,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        USER_STACK,
        0,
        ENTRY + 0x36,
    ];

    assert_eq!(used, BUDGET_AFTER_BOTH_SWIS);
    assert_eq!(report.budget_used, BUDGET_AFTER_BOTH_SWIS);
    assert_eq!(report.budget_remaining, 0);
    assert_eq!(report.fallbacks, 0);
    assert_eq!(report.pc, ENTRY + 0x36);
    assert_eq!(report.cpsr, 0x0000_00FF);
    assert!(
        report.thumb,
        "Thumb SWI return restores the interrupted T bit"
    );
    assert_eq!(report.regs, expected_regs);
    assert_eq!(
        report.cpu_sha256,
        independent_final_cpu_digest(),
        "independent full-bank oracle includes SPSR_svc and LR_svc"
    );
    assert_eq!(report.bus_sha256, sha256::hex(&expected_bus.state_digest()));
    assert_eq!(
        report.cpu_sha256,
        sha256::hex(&reference_cpu.state_digest()),
        "the complete native CPU digest includes banked SVC SPSR/LR"
    );
    assert!(
        report
            .matches_reference(&reference_cpu, &expected_bus, used)
            .expect("oracle bus")
    );
    assert!(
        report
            .matches_reference(&reference_cpu, &reference_bus, used)
            .expect("reference")
    );

    let wrong = temp.path().join("wrong.gba");
    let mut bytes = cart.bytes().to_vec();
    bytes[0x84] ^= 1;
    std::fs::write(&wrong, bytes).expect("different valid-sized ROM");
    let error = full_harness
        .run(&wrong)
        .expect_err("wrong digest must fail");
    assert!(format!("{error}").contains("ROM mismatch"));

    for (budget, expected_pc) in [(2, ENTRY + 8), (3, ENTRY + 0x0C)] {
        let source = aot::emit_native_module(&cart, &artifact, budget).expect("budget source");
        let harness = NativeHarness::build(&source, &temp.path().join(format!("budget-{budget}")))
            .expect("build budget harness");
        let report = harness.run(&fixture).expect("run budget harness");
        let (cpu, bus, used) = reference(&cart, budget);
        assert_eq!(report.budget_used, budget);
        assert_eq!(report.pc, expected_pc);
        assert!(
            report
                .matches_reference(&cpu, &bus, used)
                .expect("budget match")
        );
    }
}
