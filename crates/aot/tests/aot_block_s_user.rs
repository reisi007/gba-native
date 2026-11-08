//! Generated native AOT coverage for ARM User-bank block transfers.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::{Bus, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::{Cpu, Mode, USER_STACK};
use gba_native::interpret;
use gba_native::ir::{PSR_FIELD_C, Psr};
use gba_native_aot::aot::{self, NativeHarness};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("gba-native-s-user-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words: [u32; 10] = [
        0xE59F_0038, // LDR r0, [pc, #0x38] -> literal at 0x40
        0xE3A0_8011, // MOV r8, #0x11 (System/User bank)
        0xE321_F011, // MSR CPSR_c, #FIQ
        0xE3A0_8022, // MOV r8, #0x22 (FIQ bank)
        0xE3A0_1033, // MOV r1, #0x33
        0xE580_1000, // STR r1, [r0]
        0xE8D0_0100, // LDMIA r0, {r8}^
        0xE8C0_0100, // STMIA r0, {r8}^
        0xE321_F01F, // MSR CPSR_c, #System
        0xEAFF_FFFE, // B . (budget stops before this)
    ];
    for (i, word) in words.iter().enumerate() {
        rom[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x40..0x44].copy_from_slice(&(EWRAM_BASE + 0x200).to_le_bytes());
    finish(rom, b"GBA-SU\0")
}

fn exception_return_fixture() -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    let words: [u32; 9] = [
        0xE59F_0038, // LDR r0, [pc,#0x38] -> EWRAM+0x200
        0xE321_F013, // MSR CPSR_c, #SVC
        0xE361_F01F, // MSR SPSR_svc_c, #System
        0xE368_F4A0, // MSR SPSR_svc_f, #0xA0000000
        0xE59F_1038, // LDR r1, [pc,#0x38] -> SVC SP value
        0xE580_1000, // STR r1, [r0]
        0xE59F_1034, // LDR r1, [pc,#0x34] -> return PC
        0xE580_1004, // STR r1, [r0,#4]
        0xE8D0_A000, // LDMIA r0,{r13,pc}^
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x40..0x44].copy_from_slice(&(EWRAM_BASE + 0x200).to_le_bytes());
    rom[0x50..0x54].copy_from_slice(&0x0300_7FD0u32.to_le_bytes());
    rom[0x54..0x58].copy_from_slice(&0x0800_1237u32.to_le_bytes());
    finish(rom, b"GBA-SR\0")
}

fn finish(mut rom: Vec<u8>, title: &[u8]) -> Cartridge {
    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA0 + title.len()].copy_from_slice(title);
    rom[0xAC..0xB0].copy_from_slice(b"GBAD");
    rom[0xB0..0xB2].copy_from_slice(b"00");
    rom[0xB2] = 0x96;
    let sum = rom[0xA0..=0xBC].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).unwrap()
}

#[test]
fn generated_fiq_block_transfers_use_user_bank_and_match_interpreter() {
    let cart = fixture();
    let artifact = aot::discover(&cart, 0x0800_0000, false, aot::Limits::default()).unwrap();
    let budget = 9;
    let source = aot::emit_native_module(&cart, &artifact, budget).unwrap();
    let translated = source.split("pub fn run_program").next().unwrap();
    assert!(!translated.contains("interpret::step"));
    assert!(
        source.contains("cpu.user_reg("),
        "expected User-bank store accessor"
    );
    assert!(
        source.contains("cpu.set_user_reg("),
        "expected User-bank load accessor"
    );

    let mut expected_cpu = Cpu::direct_entry();
    expected_cpu.initialize_bios_stacks();
    expected_cpu.set_pc(0x0800_0000);
    expected_cpu.set_thumb(false);
    let mut expected_bus = Bus::new(cart.clone());
    assert_eq!(
        interpret::run_budget(&mut expected_cpu, &mut expected_bus, budget),
        Ok(budget)
    );
    assert_eq!(expected_cpu.mode(), Mode::System);
    assert_eq!(expected_cpu.reg(8), 0x33);
    assert_eq!(expected_bus.read32(EWRAM_BASE + 0x200), Ok(0x33));

    let temp = Temp::new();
    let rom = temp.path().join("synthetic.gba");
    std::fs::write(&rom, cart.bytes()).unwrap();
    let harness = NativeHarness::build(&source, &temp.path().join("harness")).unwrap();
    let report = harness.run(&rom).unwrap();
    assert_eq!(report.budget_used, budget);
    assert_eq!(report.fallbacks, 0);
    assert!(
        report
            .matches_reference(&expected_cpu, &expected_bus, budget)
            .unwrap(),
        "native={report:#?}; reference_cpu={expected_cpu:#?}; reference_bus={:?}",
        expected_bus.state_digest()
    );
    mode_fiq(&mut expected_cpu);
    assert_eq!(expected_cpu.reg(8), 0x22, "inactive FIQ bank was preserved");
}

#[test]
fn generated_ldm_user_bank_pc_restores_spsr_and_current_mode_bank() {
    let cart = exception_return_fixture();
    let artifact = aot::discover(&cart, 0x0800_0000, false, aot::Limits::default()).unwrap();
    let budget = 9;
    let source = aot::emit_native_module(&cart, &artifact, budget).unwrap();
    let translated = source.split("pub fn run_program").next().unwrap();
    assert!(!translated.contains("interpret::step"));
    assert!(source.contains("cpu.validate_exception_return()?;"));
    assert!(source.contains("cpu.return_from_exception(pc_value)?;"));

    let mut expected_cpu = Cpu::direct_entry();
    expected_cpu.initialize_bios_stacks();
    expected_cpu.set_pc(0x0800_0000);
    expected_cpu.set_thumb(false);
    let mut expected_bus = Bus::new(cart.clone());
    assert_eq!(
        interpret::run_budget(&mut expected_cpu, &mut expected_bus, budget),
        Ok(budget)
    );
    assert_eq!(expected_cpu.mode(), Mode::System);
    assert_eq!(expected_cpu.flags(), (true, false, true, false));
    assert_eq!(expected_cpu.pc(), 0x0800_1234);
    assert_eq!(expected_cpu.reg(13), USER_STACK);
    assert_eq!(expected_bus.read32(EWRAM_BASE + 0x200), Ok(0x0300_7FD0));

    let temp = Temp::new();
    let rom = temp.path().join("synthetic.gba");
    std::fs::write(&rom, cart.bytes()).unwrap();
    let harness = NativeHarness::build(&source, &temp.path().join("harness")).unwrap();
    let report = harness.run(&rom).unwrap();
    assert_eq!(report.budget_used, budget);
    assert_eq!(report.fallbacks, 0);
    assert!(
        report
            .matches_reference(&expected_cpu, &expected_bus, budget)
            .unwrap(),
        "native CPU digest {}, bus digest {} did not match reference",
        report.cpu_sha256,
        report.bus_sha256
    );

    expected_cpu
        .write_psr(Psr::Cpsr, PSR_FIELD_C, Mode::Svc.bits())
        .unwrap();
    assert_eq!(
        expected_cpu.reg(13),
        0x0300_7FD0,
        "SVC SP was loaded before return"
    );
}

fn mode_fiq(cpu: &mut Cpu) {
    cpu.write_psr(
        gba_native::ir::Psr::Cpsr,
        gba_native::ir::PSR_FIELD_C,
        Mode::Fiq.bits(),
    )
    .unwrap();
}
