//! Native-AOT integration for ARM multiply-long instructions.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::Bus;
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::interpret;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-multiply-long-aot-{tag}-{}-{sequence}",
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
const BUDGET: u64 = 5;
const FINAL_PC: u32 = 0x0800_0010;

fn fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words: [u32; 5] = [
        0xE3E0_0000, // MVN r0,#0 => 0xFFFFFFFF
        0xE3A0_1002, // MOV r1,#2
        0xE083_2190, // UMULL r3,r2,r1,r0
        0xE0C5_4190, // SMULL r5,r4,r1,r0
        0xEAFF_FFFE, // B .
    ];
    for (index, word) in words.iter().enumerate() {
        let offset = index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    Cartridge::from_bytes(rom).expect("synthetic multiply-long fixture")
}

#[test]
fn native_multiply_long_matches_reference_and_hand_state() {
    let cart = fixture();
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default())
        .expect("discover multiply-long fixture");
    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit");
    assert!(source.contains("wrapping_mul"));
    assert!(source.contains("cpu.set_reg(2, low)"));
    assert!(source.contains("cpu.set_reg(3, high)"));
    assert!(!source.contains("ROM_BYTES"));

    let temp = TempDir::new("fixture");
    let fixture_path = temp.path().join("fixture.gba");
    std::fs::write(&fixture_path, cart.bytes()).expect("write fixture");
    let harness = NativeHarness::build(&source, &temp.path().join("harness"))
        .expect("build multiply-long harness");
    let report = harness.run(&fixture_path).expect("run harness");

    assert_eq!(report.pc, FINAL_PC);
    assert_eq!(report.regs[2], 0xFFFF_FFFE);
    assert_eq!(report.regs[3], 1);
    assert_eq!(report.regs[4], 0xFFFF_FFFE);
    assert_eq!(report.regs[5], 0xFFFF_FFFF);
    assert_eq!(report.fallbacks, 0);

    let mut reference_cpu = Cpu::direct_entry();
    reference_cpu.initialize_bios_stacks();
    reference_cpu.set_pc(ENTRY);
    let mut reference_bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut reference_cpu, &mut reference_bus, BUDGET)
        .expect("reference run");
    assert!(
        report
            .matches_reference(&reference_cpu, &reference_bus, used)
            .expect("compare reference")
    );
}
