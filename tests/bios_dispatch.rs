//! Dispatcher contract tests: unsupported and waiting services fail explicitly.

use gba_native::bios::{self, BiosError};
use gba_native::bus::{Bus, DISPCNT_ADDR, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::error::Error;

fn fresh() -> (Cpu, Bus) {
    let cart = Cartridge::from_bytes(vec![0u8; 0x200]).expect("test cart");
    (Cpu::direct_entry(), Bus::new(cart))
}

#[test]
fn unknown_and_waiting_services_fail_with_their_number_and_no_mutation() {
    let (mut cpu, mut bus) = fresh();
    bus.write16(DISPCNT_ADDR, 0x0010).expect("dispcnt");
    bus.write32(EWRAM_BASE, 0x1234_5678).expect("ewram");
    cpu.set_reg(0, 0xAAAA_AAAA);
    cpu.set_reg(1, 0xBBBB_BBBB);
    cpu.set_reg(2, 0xCCCC_CCCC);
    cpu.set_reg(3, 0xDDDD_DDDD);
    let before = (cpu.state_digest(), bus.state_digest());

    // Includes Halt (02h), IntrWait (04h), VBlankIntrWait (05h), the
    // decompression family and the undocumented reset services.
    for service in [
        0x00u8, 0x02, 0x03, 0x04, 0x05, 0x09, 0x0A, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13, 0x14,
        0x15, 0x16, 0x17, 0x18, 0x19, 0x27, 0xFF,
    ] {
        assert_eq!(
            bios::call(service, &mut cpu, &mut bus),
            Err(Error::Bios(BiosError::UnsupportedService(service))),
            "service 0x{service:02X}"
        );
        assert_eq!(
            (cpu.state_digest(), bus.state_digest()),
            before,
            "service 0x{service:02X} mutated state"
        );
    }
}

#[test]
fn implemented_dispatcher_does_not_advance_pc_or_change_cpsr() {
    let (mut cpu, mut bus) = fresh();
    cpu.set_reg(0, 144);
    cpu.set_reg(1, 12);
    let cpsr = cpu.cpsr();
    let pc = cpu.pc();
    bios::call(bios::SERVICE_DIV, &mut cpu, &mut bus).expect("div");
    assert_eq!(cpu.reg(0), 12);
    assert_eq!(cpu.reg(1), 0);
    assert_eq!(cpu.reg(3), 12);
    assert_eq!(cpu.cpsr(), cpsr);
    assert_eq!(cpu.pc(), pc);
}
