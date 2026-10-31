//! Guest SWI 0Fh through the typed exception wrapper, with the opt-in policy.

use gba_native_core::bios::{AffinePolicy, BiosError};
use gba_native_core::bus::Bus;
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::{Cpu, Mode};
use gba_native_core::error::Error;
use gba_native_core::interpret;

const ENTRY: u32 = 0x0800_0000;
const SOURCE: u32 = 0x0800_0100;
const DESTINATION: u32 = 0x0200_0000;

fn fixture(policy: AffinePolicy) -> (Cpu, Bus) {
    let mut rom = vec![0u8; 0x400];
    rom[..2].copy_from_slice(&0xDF0Fu16.to_le_bytes()); // thumb svc 0x0F
    let entry = [0x0100u16, 0x0100, 0x0000];
    for (index, value) in entry.iter().enumerate() {
        rom[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&value.to_le_bytes());
    }
    let mut bus =
        Bus::new(Cartridge::from_bytes(rom).expect("synthetic cart")).with_affine_policy(policy);
    bus.write32(DESTINATION, 0xA5A5_A5A5).expect("seed");
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_thumb(true);
    cpu.set_reg(0, SOURCE);
    cpu.set_reg(1, DESTINATION);
    cpu.set_reg(2, 1);
    cpu.set_reg(3, 2);
    (cpu, bus)
}

#[test]
fn thumb_swi0f_writes_affine_parameters_after_exception_entry() {
    let (mut cpu, mut bus) = fixture(AffinePolicy::Documented);
    let cpsr = cpu.cpsr();
    assert_eq!(interpret::run_budget(&mut cpu, &mut bus, 1), Ok(1));
    assert_eq!(cpu.pc(), ENTRY + 2);
    assert_eq!(cpu.cpsr(), cpsr);
    assert_eq!(cpu.mode(), Mode::System);
    assert_eq!(cpu.reg(0), SOURCE);
    assert_eq!(cpu.reg(1), DESTINATION);
    assert_eq!(bus.read16(DESTINATION), Ok(0x0100));
    assert_eq!(bus.read16(DESTINATION + 2), Ok(0x0000));
    assert_eq!(bus.read16(DESTINATION + 4), Ok(0x0000));
    assert_eq!(bus.read16(DESTINATION + 6), Ok(0x0100));
}

#[test]
fn strict_default_reports_swi0f_as_unsupported_and_writes_nothing() {
    let (mut cpu, mut bus) = fixture(AffinePolicy::Strict);
    assert_eq!(
        interpret::run_budget(&mut cpu, &mut bus, 1),
        Err(Error::Bios(BiosError::UnsupportedService(0x0F)))
    );
    assert_eq!(bus.read32(DESTINATION), Ok(0xA5A5_A5A5));
    assert_eq!(cpu.pc(), ENTRY);
}
