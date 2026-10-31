//! Guest SWI 11h through the typed exception wrapper, with the opt-in policy.

use gba_native_core::bios::{BiosError, DecompressionPolicy};
use gba_native_core::bus::Bus;
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::{Cpu, Mode};
use gba_native_core::error::Error;
use gba_native_core::interpret;

const ENTRY: u32 = 0x0800_0000;
const STREAM: u32 = 0x0800_0100;
const DESTINATION: u32 = 0x0200_0000;

fn fixture(policy: DecompressionPolicy) -> (Cpu, Bus) {
    let mut rom = vec![0u8; 0x400];
    rom[..2].copy_from_slice(&0xDF11u16.to_le_bytes()); // thumb svc 0x11
    let mut stream = 0x0000_0610u32.to_le_bytes().to_vec(); // type 1, size 6
    stream.extend_from_slice(&[0b0010_0000, b'A', b'B', 0x10, 0x01]);
    rom[0x100..0x100 + stream.len()].copy_from_slice(&stream);
    let mut bus = Bus::new(Cartridge::from_bytes(rom).expect("synthetic cart"))
        .with_decompression_policy(policy);
    bus.write32(DESTINATION, 0xA5A5_A5A5).expect("seed");
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_thumb(true);
    cpu.set_reg(0, STREAM);
    cpu.set_reg(1, DESTINATION);
    (cpu, bus)
}

#[test]
fn thumb_swi11_decompresses_into_ewram_after_exception_entry() {
    let (mut cpu, mut bus) = fixture(DecompressionPolicy::DocumentedFormats);
    let cpsr = cpu.cpsr();
    assert_eq!(interpret::run_budget(&mut cpu, &mut bus, 1), Ok(1));
    assert_eq!(cpu.pc(), ENTRY + 2);
    assert_eq!(cpu.cpsr(), cpsr);
    assert_eq!(cpu.mode(), Mode::System);
    assert_eq!(cpu.reg(0), STREAM);
    assert_eq!(cpu.reg(1), DESTINATION);
    let output: Vec<u8> = (0..6)
        .map(|offset| bus.read8(DESTINATION + offset).expect("output byte"))
        .collect();
    assert_eq!(output, b"ABABAB");
}

#[test]
fn strict_default_reports_swi11_as_unsupported_and_writes_nothing() {
    let (mut cpu, mut bus) = fixture(DecompressionPolicy::Strict);
    assert_eq!(
        interpret::run_budget(&mut cpu, &mut bus, 1),
        Err(Error::Bios(BiosError::UnsupportedService(0x11)))
    );
    assert_eq!(bus.read32(DESTINATION), Ok(0xA5A5_A5A5));
    assert_eq!(cpu.pc(), ENTRY);
}
