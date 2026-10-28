//! Independent golden tests for the BIOS HLE `RegisterRamReset` service.
//!
//! Fixtures are entirely synthetic RAM patterns. No ROM, BIOS image, save or
//! external file is involved.

use gba_native_core::bios::{self, BiosError};
use gba_native_core::bus::{
    Bus, DISPCNT_ADDR, EWRAM_BASE, EWRAM_LEN, IWRAM_BASE, IWRAM_LEN, OAM_BASE, OAM_LEN,
    PALETTE_BASE, PALETTE_LEN, VRAM_BASE, VRAM_LEN,
};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::display;
use gba_native_core::error::Error;

const IWRAM_TAIL: u32 = 0x0300_7E00;

fn fresh() -> (Cpu, Bus) {
    let cart = Cartridge::from_bytes(vec![0u8; 0x400]).expect("test cart");
    (Cpu::direct_entry(), Bus::new(cart))
}

fn seed(bus: &mut Bus) {
    fill32(bus, EWRAM_BASE, EWRAM_LEN, 0xA5A5_A5A5);
    fill32(bus, IWRAM_BASE, IWRAM_LEN, 0xA5A5_A5A5);
    fill16(bus, PALETTE_BASE, PALETTE_LEN, 0xA5A5);
    fill16(bus, VRAM_BASE, VRAM_LEN, 0xA5A5);
    fill16(bus, OAM_BASE, OAM_LEN, 0xA5A5);
}

fn fill32(bus: &mut Bus, base: u32, len: usize, value: u32) {
    for offset in (0..len).step_by(4) {
        bus.write32(base + offset as u32, value).expect("seed");
    }
}

fn fill16(bus: &mut Bus, base: u32, len: usize, value: u16) {
    for offset in (0..len).step_by(2) {
        bus.write16(base + offset as u32, value).expect("seed");
    }
}

fn assert_zero32(bus: &Bus, base: u32, len: usize) {
    for offset in (0..len).step_by(4) {
        assert_eq!(
            bus.read32(base + offset as u32).expect("read"),
            0,
            "0x{:08X}",
            base + offset as u32
        );
    }
}

fn assert_zero16(bus: &Bus, base: u32, len: usize) {
    for offset in (0..len).step_by(2) {
        assert_eq!(
            bus.read16(base + offset as u32).expect("read"),
            0,
            "0x{:08X}",
            base + offset as u32
        );
    }
}

fn assert_value16(bus: &Bus, base: u32, len: usize, value: u16) {
    for offset in (0..len).step_by(2) {
        assert_eq!(
            bus.read16(base + offset as u32).expect("read"),
            value,
            "0x{:08X}",
            base + offset as u32
        );
    }
}

fn assert_reset_error(cpu: &mut Cpu, bus: &mut Bus, flags: u32, error: BiosError) {
    cpu.set_reg(0, flags);
    let before = (cpu.state_digest(), bus.state_digest());
    assert_eq!(
        bios::call(bios::SERVICE_REGISTER_RAM_RESET, cpu, bus),
        Err(Error::Bios(error))
    );
    assert_eq!((cpu.state_digest(), bus.state_digest()), before);
    assert_eq!(bus.read16(DISPCNT_ADDR).expect("dispcnt"), 0x0010);
    assert_eq!(bus.read32(EWRAM_BASE).expect("ewram"), 0xA5A5_A5A5);
}

#[test]
fn register_ram_reset_with_zero_flags_only_forces_blank() {
    let (mut cpu, mut bus) = fresh();
    seed(&mut bus);
    bus.write16(DISPCNT_ADDR, 0x1234).expect("dispcnt");
    cpu.set_reg(1, 0x1111_1111);
    cpu.set_reg(2, 0x2222_2222);
    let cpsr = cpu.cpsr();
    let pc = cpu.pc();
    cpu.set_reg(0, 0);

    bios::call(bios::SERVICE_REGISTER_RAM_RESET, &mut cpu, &mut bus).expect("reset");
    assert_eq!(bus.read32(EWRAM_BASE).expect("ewram"), 0xA5A5_A5A5);
    assert_eq!(bus.read32(IWRAM_BASE).expect("iwram"), 0xA5A5_A5A5);
    assert_eq!(bus.read16(PALETTE_BASE).expect("palette"), 0xA5A5);
    assert_eq!(bus.read16(VRAM_BASE).expect("vram"), 0xA5A5);
    assert_eq!(bus.read16(OAM_BASE).expect("oam"), 0xA5A5);
    assert_eq!(bus.read16(DISPCNT_ADDR).expect("dispcnt"), 0x0080);
    assert_eq!(cpu.reg(0), 0);
    assert_eq!(cpu.reg(1), 0x1111_1111);
    assert_eq!(cpu.reg(2), 0x2222_2222);
    assert_eq!(cpu.cpsr(), cpsr);
    assert_eq!(cpu.pc(), pc);
    let image = display::snapshot(&bus).expect("snapshot");
    assert_eq!(image.pixel(0, 0), 0x00FF_FFFF);
    assert_eq!(image.pixel(239, 159), 0x00FF_FFFF);
}

#[test]
fn register_ram_reset_clears_each_supported_memory_region() {
    let (mut cpu, mut bus) = fresh();
    seed(&mut bus);
    cpu.set_reg(0, 0x01);
    bios::call(bios::SERVICE_REGISTER_RAM_RESET, &mut cpu, &mut bus).expect("ewram");
    assert_zero32(&bus, EWRAM_BASE, EWRAM_LEN);
    assert_eq!(bus.read32(IWRAM_BASE).expect("iwram"), 0xA5A5_A5A5);

    seed(&mut bus);
    cpu.set_reg(0, 0x02);
    bios::call(bios::SERVICE_REGISTER_RAM_RESET, &mut cpu, &mut bus).expect("iwram");
    assert_zero32(&bus, IWRAM_BASE, (IWRAM_TAIL - IWRAM_BASE) as usize);
    assert_value16(&bus, IWRAM_TAIL, 512, 0xA5A5);
    assert_eq!(bus.read32(EWRAM_BASE).expect("ewram"), 0xA5A5_A5A5);

    seed(&mut bus);
    cpu.set_reg(0, 0x04);
    bios::call(bios::SERVICE_REGISTER_RAM_RESET, &mut cpu, &mut bus).expect("palette");
    assert_zero16(&bus, PALETTE_BASE, PALETTE_LEN);
    assert_eq!(bus.read16(VRAM_BASE).expect("vram"), 0xA5A5);

    seed(&mut bus);
    cpu.set_reg(0, 0x08);
    bios::call(bios::SERVICE_REGISTER_RAM_RESET, &mut cpu, &mut bus).expect("vram");
    assert_zero16(&bus, VRAM_BASE, VRAM_LEN);
    assert_eq!(bus.read16(PALETTE_BASE).expect("palette"), 0xA5A5);

    seed(&mut bus);
    cpu.set_reg(0, 0x10);
    bios::call(bios::SERVICE_REGISTER_RAM_RESET, &mut cpu, &mut bus).expect("oam");
    assert_zero16(&bus, OAM_BASE, OAM_LEN);
    assert_eq!(bus.read16(VRAM_BASE).expect("vram"), 0xA5A5);
}

#[test]
fn register_ram_reset_all_memory_flags_preserves_iwram_tail() {
    let (mut cpu, mut bus) = fresh();
    seed(&mut bus);
    for offset in (IWRAM_TAIL..IWRAM_BASE + IWRAM_LEN as u32).step_by(2) {
        bus.write16(offset, 0x5A5A).expect("tail");
    }
    cpu.set_reg(0, 0x1F);
    cpu.set_reg(1, 0x9999_9999);
    let cpsr = cpu.cpsr();
    let pc = cpu.pc();
    bios::call(bios::SERVICE_REGISTER_RAM_RESET, &mut cpu, &mut bus).expect("reset all");

    assert_zero32(&bus, EWRAM_BASE, EWRAM_LEN);
    assert_zero32(&bus, IWRAM_BASE, (IWRAM_TAIL - IWRAM_BASE) as usize);
    assert_value16(&bus, IWRAM_TAIL, 512, 0x5A5A);
    assert_zero16(&bus, PALETTE_BASE, PALETTE_LEN);
    assert_zero16(&bus, VRAM_BASE, VRAM_LEN);
    assert_zero16(&bus, OAM_BASE, OAM_LEN);
    assert_eq!(bus.read16(DISPCNT_ADDR).expect("dispcnt"), 0x0080);
    assert_eq!(cpu.reg(0), 0x1F);
    assert_eq!(cpu.reg(1), 0x9999_9999);
    assert_eq!(cpu.cpsr(), cpsr);
    assert_eq!(cpu.pc(), pc);
    let image = display::snapshot(&bus).expect("snapshot");
    assert_eq!(image.pixel(120, 80), 0x00FF_FFFF);
}

#[test]
fn register_ram_reset_documented_but_unmodeled_groups_are_pre_effect_errors() {
    let (mut cpu, mut bus) = fresh();
    seed(&mut bus);
    bus.write16(DISPCNT_ADDR, 0x0010).expect("dispcnt");
    for bit in 5..=7 {
        let flags = 1u32 << bit;
        assert_reset_error(
            &mut cpu,
            &mut bus,
            flags,
            BiosError::UnsupportedResetFlags { flags },
        );
    }
    let flags = 0xFF;
    assert_reset_error(
        &mut cpu,
        &mut bus,
        flags,
        BiosError::UnsupportedResetFlags { flags },
    );
}

#[test]
fn register_ram_reset_reserved_bits_are_pre_effect_errors() {
    let (mut cpu, mut bus) = fresh();
    seed(&mut bus);
    bus.write16(DISPCNT_ADDR, 0x0010).expect("dispcnt");
    for bit in 8..32 {
        let flags = 1u32 << bit;
        assert_reset_error(
            &mut cpu,
            &mut bus,
            flags,
            BiosError::ReservedResetFlags { flags },
        );
    }
    let flags = u32::MAX;
    assert_reset_error(
        &mut cpu,
        &mut bus,
        flags,
        BiosError::ReservedResetFlags { flags },
    );
}
