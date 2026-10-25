//! Native conditional, unsupported, precondition and SVC-framing SWI paths.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, DISPCNT_ADDR, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::{Cpu, FLAG_F, FLAG_I, Mode};
use gba_native::ir::PSR_FIELD_C;
use gba_native::sha256;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
const ENTRY: u32 = 0x0800_0000;
const LITERAL_OFFSET: u32 = 0x100;
const LITERALS: u32 = ENTRY + LITERAL_OFFSET;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gba-native-aot-swi-errors-{}-{sequence}",
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

fn cart(words: &[u32], literals: &[(usize, u32)]) -> Cartridge {
    let mut rom = vec![0; 0x200];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    for (index, word) in literals {
        let offset = LITERAL_OFFSET as usize + index * 4;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    Cartridge::from_bytes(rom).expect("synthetic error-path cart")
}

fn instrument_swi_errors(source: &str) -> (String, usize) {
    let mut count = 0;
    let lines = source
        .lines()
        .map(|line| {
            let call = line.trim();
            if !call.starts_with("gba_native::bios::execute_swi(") || !call.ends_with("?;") {
                return line.to_string();
            }
            count += 1;
            let call = call.strip_suffix("?;").expect("SWI call suffix");
            format!(
                "    {{ let before_cpu = cpu.state_digest(); let before_bus = bus.state_digest(); \
                 let result = {call}; match result {{ \
                 Ok(()) => {{}}, Err(error) => {{ \
                 eprintln!(\"swi_before_cpu={{}}\", gba_native::sha256::hex(&before_cpu)); \
                 eprintln!(\"swi_before_bus={{}}\", gba_native::sha256::hex(&before_bus)); \
                 eprintln!(\"swi_after_cpu={{}}\", gba_native::sha256::hex(&cpu.state_digest())); \
                 eprintln!(\"swi_after_bus={{}}\", gba_native::sha256::hex(&bus.state_digest())); \
                 return Err(error); }} }} }}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    (lines, count)
}

fn run_native_error(cart: &Cartridge, budget: u64) -> String {
    let artifact = aot::discover(cart, ENTRY, false, aot::Limits::default()).expect("discover");
    let source = aot::emit_native_module(cart, &artifact, budget).expect("emit error fixture");
    let (source, instrumented) = instrument_swi_errors(&source);
    assert_eq!(instrumented, artifact.stats.swis as usize);
    assert!(
        instrumented > 0,
        "the failing SWI must be direct native code"
    );

    let temp = TempDir::new();
    let fixture = temp.path().join("error-fixture.gba");
    std::fs::write(&fixture, cart.bytes()).expect("external synthetic ROM");
    let harness = NativeHarness::build(&source, &temp.path().join("harness"))
        .expect("build instrumented direct-emission harness");
    harness
        .run(&fixture)
        .expect_err("fixture must exercise its native SWI error")
        .to_string()
}

fn prepared_cpu() -> Cpu {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu
}

fn assert_unchanged_native_error(
    cart: &Cartridge,
    budget: u64,
    expected_error: &str,
    expected_cpu: &Cpu,
) {
    let error = run_native_error(cart, budget);
    assert!(
        error.contains(expected_error),
        "unexpected native error: {error}"
    );
    let expected_bus = Bus::new(cart.clone());
    for (field, digest) in [
        ("swi_before_cpu", sha256::hex(&expected_cpu.state_digest())),
        ("swi_after_cpu", sha256::hex(&expected_cpu.state_digest())),
        ("swi_before_bus", sha256::hex(&expected_bus.state_digest())),
        ("swi_after_bus", sha256::hex(&expected_bus.state_digest())),
    ] {
        assert!(
            error.contains(&format!("{field}={digest}")),
            "native {field} did not match the independent pre-effect oracle: {error}"
        );
    }
}

#[test]
fn conditional_false_unknown_swi_succeeds_without_svc_or_fallback() {
    let cart = cart(&[0x0F00_0006, 0xEAFF_FFFE], &[]); // EQ SWI; service field is 00h.
    let artifact = aot::discover(&cart, ENTRY, false, aot::Limits::default()).expect("discover");
    assert_eq!(artifact.stats.swis, 1);
    let source = aot::emit_native_module(&cart, &artifact, 1).expect("emit false SWI");
    let temp = TempDir::new();
    let fixture = temp.path().join("false-conditional.gba");
    std::fs::write(&fixture, cart.bytes()).expect("external synthetic ROM");
    let harness = NativeHarness::build(&source, &temp.path().join("harness"))
        .expect("build false-condition harness");
    let report = harness
        .run(&fixture)
        .expect("false unsupported SWI is a no-op");

    let mut expected_cpu = prepared_cpu();
    expected_cpu.set_pc(ENTRY + 4);
    let expected_bus = Bus::new(cart);
    assert_eq!(report.pc, ENTRY + 4);
    assert_eq!(report.cpsr, 0xDF);
    assert_eq!(report.budget_used, 1);
    assert_eq!(report.budget_remaining, 0);
    assert_eq!(report.fallbacks, 0);
    assert_eq!(
        report.cpu_sha256,
        sha256::hex(&expected_cpu.state_digest()),
        "false SWI leaves all banks and SPSRs unchanged"
    );
    assert_eq!(
        report.bus_sha256,
        sha256::hex(&expected_bus.state_digest()),
        "false SWI does not write an SVC frame"
    );
}

#[test]
fn unknown_arm_comment_uses_service_zero_and_errors_before_mutation() {
    // Comment 0x000006 has service byte 0: ARM takes bits 23..16, not bits 7..0.
    let cart = cart(&[0xEF00_0006], &[]);
    let mut expected = prepared_cpu();
    expected.set_pc(ENTRY);
    assert_unchanged_native_error(&cart, 1, "service 0x00 is not implemented", &expected);
}

#[test]
fn known_service_preconditions_fail_before_native_svc_framing() {
    let div_zero = cart(&[0xEF06_0000], &[]); // valid Div, zero divisor in r1
    let mut expected = prepared_cpu();
    expected.set_pc(ENTRY);
    assert_unchanged_native_error(
        &div_zero,
        1,
        "signed division by zero; no registers changed",
        &expected,
    );

    let cpu_set = cart(
        &[
            ldr_literal(0, ENTRY, LITERALS),
            ldr_literal(1, ENTRY + 4, LITERALS + 4),
            ldr_literal(2, ENTRY + 8, LITERALS + 8),
            0xEF0B_0000,
            0xEAFF_FFFE,
        ],
        &[
            (0, EWRAM_BASE),
            (1, DISPCNT_ADDR),
            (2, 0x0400_0001), // word copy, one unit, invalid I/O destination
        ],
    );
    expected = prepared_cpu();
    expected.set_reg(0, EWRAM_BASE);
    expected.set_reg(1, DISPCNT_ADDR);
    expected.set_reg(2, 0x0400_0001);
    expected.set_pc(ENTRY + 12);
    assert_unchanged_native_error(
        &cpu_set,
        4,
        "destination range at 0x04000000 is not writable memory",
        &expected,
    );

    let reset = cart(&[0xE3A0_0020, 0xEF01_0000, 0xEAFF_FFFE], &[]);
    expected = prepared_cpu();
    expected.set_reg(0, 0x20);
    expected.set_pc(ENTRY + 4);
    assert_unchanged_native_error(
        &reset,
        2,
        "flags 0x00000020 request unsupported reset bits 5..31",
        &expected,
    );
}

#[test]
fn invalid_svc_stack_errors_before_frame_or_service_effects() {
    let cart = cart(
        &[
            0xE3A0_00D3, // MOV r0,#SVC|I|F|ARM
            0xE121_F000, // MSR CPSR_c,r0
            0xE3A0_D000, // MOV sp,#0 (SVC frame underflows)
            0xE3A0_00DF, // MOV r0,#SYSTEM|I|F|ARM
            0xE121_F000, // MSR CPSR_c,r0
            0xEF08_0000, // supported Sqrt, rejected by invalid SVC stack
            0xEAFF_FFFE,
        ],
        &[],
    );
    let mut expected = prepared_cpu();
    expected
        .write_psr(
            gba_native::ir::Psr::Cpsr,
            PSR_FIELD_C,
            Mode::Svc.bits() | FLAG_I | FLAG_F,
        )
        .expect("independent SVC bank switch");
    expected.set_reg(13, 0);
    expected
        .write_psr(gba_native::ir::Psr::Cpsr, PSR_FIELD_C, 0xDF)
        .expect("independent return to SYSTEM before SWI");
    expected.set_reg(0, 0xDF);
    expected.set_pc(ENTRY + 20);
    assert_unchanged_native_error(&cart, 6, "invalid SVC stack pointer 0x00000000", &expected);
}

#[test]
fn supported_svc_origin_nesting_errors_before_mutation() {
    let cart = cart(
        &[
            0xE3A0_00D3, // MOV r0,#SVC|I|F|ARM
            0xE121_F000, // MSR CPSR_c,r0
            0xEF08_0000, // supported Sqrt from SVC mode
            0xEAFF_FFFE,
        ],
        &[],
    );
    let mut expected = prepared_cpu();
    expected.set_reg(0, 0xD3);
    expected
        .write_psr(
            gba_native::ir::Psr::Cpsr,
            PSR_FIELD_C,
            Mode::Svc.bits() | FLAG_I | FLAG_F,
        )
        .expect("independent SVC origin");
    expected.set_pc(ENTRY + 8);
    assert_unchanged_native_error(
        &cart,
        3,
        "nested entry from SVC mode is unsupported",
        &expected,
    );
}
