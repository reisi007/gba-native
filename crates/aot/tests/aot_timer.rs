//! Native-AOT parity for the instruction-budget timer model.
//!
//! Two synthetic guests (ARM and Thumb) start Timer 0, poll its counter into a
//! register and then spin. The generated native blocks must reproduce the
//! reference interpreter's CPU, timer and bus state exactly.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::Bus;
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::interpret;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

const ENTRY: u32 = 0x0800_0000;
const TM0_L: u32 = 0x0400_0100;
const TM0_H: u32 = 0x0400_0102;
const BUDGET: u64 = 20;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-aot-timer-{tag}-{}-{sequence}",
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

fn arm_program() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words: [u32; 8] = [
        0xE59F_0078, // 0x00 LDR r0,[pc,#0x78] -> TM0CNT_L
        0xE3A0_1000, // 0x04 MOV r1,#0
        0xE1C0_10B0, // 0x08 STRH r1,[r0]
        0xE59F_2070, // 0x0C LDR r2,[pc,#0x70] -> TM0CNT_H
        0xE3A0_3080, // 0x10 MOV r3,#0x80
        0xE1C2_30B0, // 0x14 STRH r3,[r2] (start, F/1)
        0xE1D0_40B0, // 0x18 LDRH r4,[r0]
        0xEAFF_FFFE, // 0x1C B .
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x80..0x84].copy_from_slice(&TM0_L.to_le_bytes());
    rom[0x84..0x88].copy_from_slice(&TM0_H.to_le_bytes());
    Cartridge::from_bytes(rom).expect("synthetic ARM timer fixture")
}

fn thumb_program() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let halfwords: [u16; 8] = [
        0x4810, // 0x00 LDR r0,[pc,#0x40] -> 0x08000044
        0x2100, // 0x02 MOV r1,#0
        0x8001, // 0x04 STRH r1,[r0]
        0x4A10, // 0x06 LDR r2,[pc,#0x40] -> 0x08000048
        0x2380, // 0x08 MOV r3,#0x80
        0x8013, // 0x0A STRH r3,[r2] (start, F/1)
        0x8804, // 0x0C LDRH r4,[r0]
        0xE7FE, // 0x0E B .
    ];
    for (index, halfword) in halfwords.iter().enumerate() {
        rom[index * 2..index * 2 + 2].copy_from_slice(&halfword.to_le_bytes());
    }
    rom[0x44..0x48].copy_from_slice(&TM0_L.to_le_bytes());
    rom[0x48..0x4C].copy_from_slice(&TM0_H.to_le_bytes());
    Cartridge::from_bytes(rom).expect("synthetic Thumb timer fixture")
}

fn reference(cart: &Cartridge, thumb: bool) -> (Cpu, Bus) {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(ENTRY);
    cpu.set_thumb(thumb);
    let mut bus = Bus::new(cart.clone());
    assert_eq!(
        interpret::run_budget(&mut cpu, &mut bus, BUDGET),
        Ok(BUDGET)
    );
    (cpu, bus)
}

fn run_case(tag: &str, cart: Cartridge, thumb: bool, expected_pc: u32) {
    let artifact = aot::discover(&cart, ENTRY, thumb, aot::Limits::default())
        .expect("discover synthetic timer guest");
    assert_eq!(artifact.stats.decode_stops, 0);
    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit timer guest");
    assert!(!source.contains("ROM_BYTES"));
    let blocks = source
        .split("pub fn run_program")
        .next()
        .expect("block section");
    assert!(!blocks.contains("interpret::step"));
    assert!(!blocks.contains("decode_arm("));
    assert!(!blocks.contains("decode_thumb("));

    let temp = TempDir::new(tag);
    let fixture_path = temp.path().join("fixture.gba");
    std::fs::write(&fixture_path, cart.bytes()).expect("write fixture");
    let harness =
        NativeHarness::build(&source, &temp.path().join("harness")).expect("build timer harness");
    let report = harness.run(&fixture_path).expect("run timer harness");

    let (cpu, bus) = reference(&cart, thumb);
    assert!(
        report
            .matches_reference(&cpu, &bus, BUDGET)
            .expect("compare reference"),
        "{tag} native report differs from the reference interpreter"
    );
    assert_eq!(report.pc, expected_pc);
    assert_eq!(report.fallbacks, 0, "the timer fixture is fully translated");
    assert_eq!(report.regs[4], 3, "polled counter after one enabled tick");
    assert_eq!(
        bus.read16(TM0_L).expect("timer counter"),
        45,
        "fifteen enabled instruction ticks at three cycles each"
    );
    assert_eq!(
        report.bus_sha256,
        gba_native::sha256::hex(&bus.state_digest())
    );
}

#[test]
fn native_arm_timer_poll_matches_the_reference_interpreter() {
    run_case("arm", arm_program(), false, 0x0800_001C);
}

#[test]
fn native_thumb_timer_poll_matches_the_reference_interpreter() {
    run_case("thumb", thumb_program(), true, 0x0800_000E);
}
