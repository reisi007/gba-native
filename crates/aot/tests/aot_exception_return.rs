//! Native-AOT integration for the documented ARM `S=1, Rd=PC` exception return.
//!
//! The fixtures are entirely synthetic. Expected register values are
//! hand-derived from GBATEK `#arm-opcodes-data-processing-alu`: an `S=1`
//! data-processing instruction with `Rd=R15` restores the CPSR from the current
//! mode's SPSR and takes the computed result as the PC, without applying the
//! arithmetic N/Z/C/V result.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::Bus;
use gba_native::cart::Cartridge;
use gba_native::cpu::{Cpu, USER_STACK};
use gba_native::error::Error;
use gba_native::interpret;
use gba_native::sha256;
use gba_native_aot::aot::{self, NativeHarness};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        // `create_dir` is exclusive: a stale directory from a reused PID is
        // never adopted, and the Drop guard therefore only removes a directory
        // this process created.
        loop {
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "gba-native-exception-return-{tag}-{}-{sequence}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create temp dir {path:?}: {error}"),
            }
        }
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

const ENTRY: u32 = 0x0800_0000;
const RETURN_TARGET_BLOCK: u32 = 0x0800_0030;
const FINAL_PC: u32 = 0x0800_003C;
const BUDGET: u64 = 12;
const SVC_STACK: u32 = 0x0300_7FE0;

fn cart_with(words: &[(usize, u32)]) -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    for (offset, word) in words {
        rom[*offset..*offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    Cartridge::from_bytes(rom).expect("synthetic cart")
}

fn ldr_literal(rd: u32, instruction_addr: u32, target: u32) -> u32 {
    let difference = target - (instruction_addr + 8);
    0xE59F_0000 | (rd << 12) | difference
}

fn msr_cpsr_c(rm: u32) -> u32 {
    0xE121_F000 | rm
}

fn msr_spsr_fc(rm: u32) -> u32 {
    0xE169_F000 | rm
}

/// LDR/MOV r0 = 0x0800_0032, MSR to SVC, SPSR_svc = SYSTEM/ARM with NZCV set,
/// then `MOVS PC,r0`. A never-taken conditional branch keeps the return target
/// discoverable so the native run needs no fallback.
fn native_fixture() -> Cartridge {
    cart_with(&[
        (0x000, ldr_literal(0, 0x000, 0x048)), // r0 = 0x0800_0032
        (0x004, 0x0A00_0009),                  // BEQ +0x24 -> 0x030 (never taken)
        (0x008, ldr_literal(1, 0x008, 0x04C)), // r1 = 0x13 (SVC)
        (0x00C, msr_cpsr_c(1)),                // switch to SVC
        (0x010, ldr_literal(1, 0x010, 0x050)), // r1 = 0xF000001F
        (0x014, msr_spsr_fc(1)),               // SPSR_svc = SYSTEM, ARM, NZCV
        (0x018, 0xE1A0_200D),                  // MOV r2,r13 (SVC SP)
        (0x01C, 0xE1B0_F000),                  // MOVS PC,r0 (exception return)
        (0x030, 0xE1A0_300D),                  // MOV r3,r13 (SYSTEM SP)
        (0x034, 0xE10F_4000),                  // MRS r4,CPSR
        (0x038, 0xE3A0_505A),                  // MOV r5,#0x5A
        (0x03C, 0xEAFF_FFFE),                  // B .
        (0x048, 0x0800_0032),                  // unaligned ARM target
        (0x04C, 0x0000_0013),                  // SVC mode bits
        (0x050, 0xF000_001F),                  // SYSTEM, ARM, N=Z=C=V=1
    ])
}

fn reference(cart: &Cartridge, budget: u64) -> (Cpu, Bus, u64) {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(ENTRY);
    cpu.set_thumb(false);
    let mut bus = Bus::new(cart.clone());
    let mut used = 0;
    while used < budget {
        if let Err(error) = interpret::step(&mut cpu, &mut bus) {
            panic!("reference failed at pc=0x{:08X}: {error}", cpu.pc());
        }
        used += 1;
    }
    (cpu, bus, used)
}

#[test]
fn native_exception_return_restores_cpsr_bank_and_target_state() {
    let cart = native_fixture();
    let artifact =
        aot::discover(&cart, ENTRY, false, aot::Limits::default()).expect("discover fixture");
    assert_eq!(artifact.stats.pc_writes, 1, "the return is control flow");
    assert_eq!(artifact.stats.decode_stops, 0);
    assert!(
        artifact
            .blocks
            .iter()
            .any(|block| block.addr == RETURN_TARGET_BLOCK),
        "the return target is translated"
    );

    let source = aot::emit_native_module(&cart, &artifact, BUDGET).expect("emit source");
    assert!(
        source.contains("    cpu.return_from_exception(value)?;"),
        "the return is emitted as the validated SPSR restore"
    );
    assert!(
        !source.contains("set_logic_flags(value, shifter_carry)"),
        "the restored CPSR replaces the S=1 NZCV result"
    );
    assert!(!source.contains("ROM_BYTES"));

    let (cpu, bus, used) = reference(&cart, BUDGET);
    assert_eq!(used, BUDGET);
    // Hand oracle for the reference path: the SPSR write set NZCV and SYSTEM,
    // the MOVS result must not clear N.
    assert_eq!(cpu.cpsr(), 0xF000_001F);
    assert_eq!(cpu.pc(), FINAL_PC);

    let temp = TempDir::new("fixture");
    let fixture = temp.path().join("fixture.gba");
    std::fs::write(&fixture, cart.bytes()).expect("write fixture");
    let harness =
        NativeHarness::build(&source, &temp.path().join("harness")).expect("build harness");
    let report = harness.run(&fixture).expect("run harness");

    assert_eq!(report.budget_used, BUDGET);
    assert_eq!(report.budget_remaining, 0);
    assert_eq!(report.fallbacks, 0, "every block is translated");
    assert_eq!(report.pc, FINAL_PC);
    assert_eq!(report.cpsr, 0xF000_001F, "CPSR came from SPSR_svc");
    assert!(!report.thumb);
    assert_eq!(
        report.regs,
        [
            0x0800_0032, // r0: unaligned ARM target, never truncated
            0xF000_001F, // r1: last literal
            0x0300_7FE0, // r2: SVC SP read before the return
            0x0300_7F00, // r3: SYSTEM SP read after the bank switch
            0xF000_001F, // r4: CPSR after the return
            0x0000_005A, // r5
            0x0000_0000, // r6
            0x0000_0000, // r7
            0x0000_0000, // r8
            0x0000_0000, // r9
            0x0000_0000, // r10
            0x0000_0000, // r11
            0x0000_0000, // r12
            USER_STACK,  // r13: USER/SYSTEM bank
            0x0000_0000, // r14: USER/SYSTEM bank
            FINAL_PC,
        ]
    );
    assert_eq!(SVC_STACK, report.regs[2]);
    assert_eq!(report.cpu_sha256, sha256::hex(&cpu.state_digest()));
    assert!(
        report
            .matches_reference(&cpu, &bus, used)
            .expect("compare state")
    );

    let no_rom = Command::new(harness.executable())
        .output()
        .expect("run without arguments");
    assert!(!no_rom.status.success());
}

#[test]
fn native_exception_return_without_spsr_fails_closed() {
    // Direct-entry USER/SYSTEM has no SPSR. The emitted code must report the
    // documented error instead of treating the instruction as a no-op.
    let cart = cart_with(&[(0x000, 0xE1B0_F000), (0x004, 0xEAFF_FFFE)]);
    let artifact =
        aot::discover(&cart, ENTRY, false, aot::Limits::default()).expect("discover fixture");
    assert_eq!(artifact.stats.pc_writes, 1);
    assert_eq!(artifact.stats.blocks, 1);
    let source = aot::emit_native_module(&cart, &artifact, 4).expect("emit source");

    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(ENTRY);
    let mut bus = Bus::new(cart.clone());
    let error = interpret::step(&mut cpu, &mut bus).expect_err("SYSTEM has no SPSR");
    assert!(matches!(error, Error::Psr(_)), "unexpected error: {error}");

    let temp = TempDir::new("no-spsr");
    let fixture = temp.path().join("fixture.gba");
    std::fs::write(&fixture, cart.bytes()).expect("write fixture");
    let harness =
        NativeHarness::build(&source, &temp.path().join("harness")).expect("build harness");
    let error = harness.run(&fixture).expect_err("the harness must fail");
    let message = format!("{error}");
    assert!(
        message.contains("SPSR"),
        "the native failure names the missing SPSR: {message}"
    );
}
