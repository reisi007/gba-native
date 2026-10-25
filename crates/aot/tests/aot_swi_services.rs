//! Native execution of the remaining documented HLE services with hand-derived state.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, DISPCNT_ADDR, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::{Cpu, FLAG_F, FLAG_I, IRQ_STACK, Mode, SVC_STACK, USER_STACK};
use gba_native::interpret;
use gba_native::ir::{PSR_FIELD_C, PSR_FIELD_F, Psr};
use gba_native::sha256;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
const ENTRY: u32 = 0x0800_0000;
const SVC_FRAME: u32 = 0x0300_7FD0;
const LITERAL_OFFSET: u32 = 0x100;
const LITERALS: u32 = ENTRY + LITERAL_OFFSET;
const SOURCE: u32 = EWRAM_BASE + 0x100;
const DIV_MAGNITUDE: u32 = EWRAM_BASE + 0x108;
const DESTINATION: u32 = EWRAM_BASE + 0x120;
const WORD_ONE: u32 = 0x1234_5678;
const WORD_TWO: u32 = 0xCAFE_BABE;
const BUDGET: u64 = 31;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-aot-swi-services-{}-{sequence}",
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

fn add_literal_load(code: &mut Vec<u32>, rd: u32, index: u32) {
    let address = ENTRY + code.len() as u32 * 4;
    code.push(ldr_literal(rd, address, LITERALS + index * 4));
}

fn services_fixture() -> Cartridge {
    let mut code = vec![
        0xE3A0_B011, // MOV r11,#0x11
        0xE3A0_C022, // MOV r12,#0x22
        0xE3A0_E033, // MOV lr,#0x33
        0xE3A0_0023, // MOV r0,#35
        0xE3A0_1006, // MOV r1,#6
        0xEF06_0000, // SWI Div
    ];
    code.extend([
        0xE1A0_9000, // preserve Div quotient
        0xE1A0_A001, // preserve Div remainder
        0xE1A0_2003, // preserve Div quotient magnitude for the bus oracle
    ]);
    add_literal_load(&mut code, 0, 7);
    code.push(0xE580_2000); // STR r2,[r0]
    code.push(0xE3A0_000A); // MOV r0,#10 (DivArm denominator)
    add_literal_load(&mut code, 1, 0); // DivArm numerator = -1234
    code.extend([0xEF07_0000]); // SWI DivArm
    code.extend([
        0xE1A0_5000, // preserve DivArm quotient
        0xE1A0_6001, // preserve DivArm remainder
        0xE1A0_7003, // preserve DivArm quotient magnitude
    ]);
    add_literal_load(&mut code, 0, 1); // Sqrt input = 0xFFFE0001
    code.push(0xEF08_0000); // SWI Sqrt
    code.push(0xE1A0_8000); // preserve Sqrt result
    add_literal_load(&mut code, 3, 2); // EWRAM source
    add_literal_load(&mut code, 4, 3); // first copied word
    code.push(0xE583_4000); // STR r4,[r3]
    add_literal_load(&mut code, 4, 4); // second copied word
    code.push(0xE583_4004); // STR r4,[r3,#4]
    add_literal_load(&mut code, 0, 2); // CpuSet source
    add_literal_load(&mut code, 1, 5); // CpuSet destination
    add_literal_load(&mut code, 2, 6); // two aligned words
    code.push(0xEF0B_0000); // SWI CpuSet
    code.extend([0xE3A0_0000, 0xEF01_0000]); // flags=0; SWI RegisterRamReset
    code.push(0xEAFF_FFFE); // B .; keep the literal pool outside discovery

    let literals = [
        (-1234i32) as u32,
        4_294_836_225,
        SOURCE,
        WORD_ONE,
        WORD_TWO,
        DESTINATION,
        0x0400_0002, // word width, count two
        DIV_MAGNITUDE,
    ];
    let mut rom = vec![0; 0x200];
    for (index, word) in code.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    for (index, word) in literals.iter().enumerate() {
        let offset = LITERAL_OFFSET as usize + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    Cartridge::from_bytes(rom).expect("synthetic all-services cart")
}

fn expected_cpu() -> Cpu {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();

    // Construct every bank explicitly rather than using the reference run as
    // the CPU oracle. The final SWI changes only the SVC SPSR/LR bank.
    for (mode, sp, lr, spsr) in [
        (Mode::Fiq, 0, 0, 0),
        (Mode::Irq, IRQ_STACK, 0, 0),
        (Mode::Svc, SVC_STACK, ENTRY + 0x7C, 0xDF),
        (Mode::Abt, 0, 0, 0),
        (Mode::Und, 0, 0, 0),
        (Mode::System, USER_STACK, 0x33, 0),
    ] {
        cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, mode.bits() | FLAG_I | FLAG_F)
            .expect("privileged bank selection");
        for (register, value) in [0xFFFF, 5, 5, 0x11, 0x22].into_iter().enumerate() {
            cpu.set_reg(
                8 + register as u8,
                if mode == Mode::Fiq { 0 } else { value },
            );
        }
        cpu.set_reg(13, sp);
        cpu.set_reg(14, lr);
        if mode.has_spsr() {
            let fields = if spsr == 0 {
                PSR_FIELD_F
            } else {
                PSR_FIELD_F | PSR_FIELD_C
            };
            cpu.write_psr(Psr::Spsr, fields, spsr)
                .expect("independent banked SPSR value");
        }
    }

    cpu.write_psr(Psr::Cpsr, PSR_FIELD_C, 0xDF)
        .expect("final SYSTEM mode and interrupt masks");
    for (register, value) in [
        0,
        DESTINATION,
        0x0400_0002,
        SOURCE,
        WORD_TWO,
        (-123i32) as u32,
        (-4i32) as u32,
        123,
    ]
    .into_iter()
    .enumerate()
    {
        cpu.set_reg(register as u8, value);
    }
    cpu.set_nzcv(false, false, false, false);
    cpu.set_pc(ENTRY + 0x7C);
    cpu
}

fn expected_bus(cart: &Cartridge) -> Bus {
    let mut bus = Bus::new(cart.clone());
    bus.write32(SOURCE, WORD_ONE)
        .expect("independent source word 1");
    bus.write32(SOURCE + 4, WORD_TWO)
        .expect("independent source word 2");
    bus.write32(DIV_MAGNITUDE, 5)
        .expect("independent Div quotient magnitude");
    bus.write32(DESTINATION, WORD_ONE)
        .expect("independent CpuSet word 1");
    bus.write32(DESTINATION + 4, WORD_TWO)
        .expect("independent CpuSet word 2");
    bus.write32(SVC_FRAME, 0xDF)
        .expect("independent saved CPSR");
    bus.write32(SVC_FRAME + 4, 0x11)
        .expect("independent saved r11");
    bus.write32(SVC_FRAME + 8, 0x22)
        .expect("independent saved r12");
    bus.write32(SVC_FRAME + 12, ENTRY + 0x7C)
        .expect("independent saved SVC LR");
    bus.write16(DISPCNT_ADDR, 0x0080)
        .expect("RegisterRamReset always forces blank");
    bus
}

#[test]
fn remaining_services_run_natively_against_independent_full_state_oracles() {
    let cart = services_fixture();
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default()).expect("discover");
    assert_eq!(artifact.stats.swis, 5);
    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit services");
    assert_eq!(source.matches("gba_native::bios::execute_swi(").count(), 5);
    assert!(!source.contains("ROM_BYTES"));

    let temp = TempDir::new();
    let fixture = temp.path().join("six-services.gba");
    std::fs::write(&fixture, cart.bytes()).expect("external synthetic ROM");
    let harness = NativeHarness::build(&source, &temp.path().join("harness"))
        .expect("build direct-emission harness");
    let report = harness.run(&fixture).expect("run six native HLE calls");

    let expected_cpu = expected_cpu();
    let expected_bus = expected_bus(&cart);
    let mut reference_cpu = Cpu::direct_entry();
    reference_cpu.initialize_bios_stacks();
    let mut reference_bus = Bus::new(cart.clone());
    assert_eq!(
        interpret::run_budget(&mut reference_cpu, &mut reference_bus, BUDGET)
            .expect("reference services"),
        BUDGET
    );

    assert_eq!(report.budget_used, BUDGET);
    assert_eq!(report.budget_remaining, 0);
    assert_eq!(report.fallbacks, 0);
    assert_eq!(report.pc, ENTRY + 0x7C);
    assert_eq!(report.cpsr, 0xDF);
    assert!(!report.thumb);
    assert_eq!(
        report.regs,
        [
            0,
            DESTINATION,
            0x0400_0002,
            SOURCE,
            WORD_TWO,
            (-123i32) as u32,
            (-4i32) as u32,
            123,
            65_535,
            5,
            5,
            0x11,
            0x22,
            USER_STACK,
            0x33,
            ENTRY + 0x7C,
        ]
    );
    assert_eq!(
        report.cpu_sha256,
        sha256::hex(&expected_cpu.state_digest()),
        "independent digest includes all inactive banks and SPSRs"
    );
    assert_eq!(
        report.bus_sha256,
        sha256::hex(&expected_bus.state_digest()),
        "independent bus oracle includes copied words, final SVC frame and DISPCNT"
    );
    assert!(
        report
            .matches_reference(&reference_cpu, &reference_bus, BUDGET)
            .expect("reference report comparison")
    );
}
