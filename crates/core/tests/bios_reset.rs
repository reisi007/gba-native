//! Independent golden tests for the BIOS HLE `RegisterRamReset` service.
//!
//! Fixtures are entirely synthetic RAM patterns. No ROM, BIOS image, save or
//! external file is involved.

use gba_native_core::bios;
use gba_native_core::bus::{
    BG0CNT_ADDR, BG0HOFS_ADDR, BG0VOFS_ADDR, Bus, DISPCNT_ADDR, EWRAM_BASE, EWRAM_LEN, FIFO_A_ADDR,
    FIFO_B_ADDR, IWRAM_BASE, IWRAM_LEN, OAM_BASE, OAM_LEN, PALETTE_BASE, PALETTE_LEN, SoundFifo,
    VRAM_BASE, VRAM_LEN,
};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::display;

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
fn register_ram_reset_group_five_is_empty_and_group_seven_resets_other_io() {
    let (mut cpu, mut bus) = fresh();
    bus.write16(DISPCNT_ADDR, 0x0010).expect("dispcnt");
    bus.write16(BG0CNT_ADDR, 0x1234).expect("bg0cnt");
    bus.write16(BG0HOFS_ADDR, 0x0102).expect("bg0hofs");
    bus.write16(BG0VOFS_ADDR, 0x0203).expect("bg0vofs");
    bus.write16(0x0400_0084, 0x1234).expect("sound register");
    bus.write32(FIFO_A_ADDR, 0x0403_0201).expect("Sound A FIFO");
    cpu.set_reg(0, 0xA0); // groups 5 and 7; sound group 6 is not selected.

    bios::call(bios::SERVICE_REGISTER_RAM_RESET, &mut cpu, &mut bus).expect("reset 5 and 7");

    // Group 7 resets the modelled "other I/O" set, group 5 has no SIO state,
    // and the unselected group 6 leaves sound storage untouched. Scroll
    // registers are write-only, so compare the full bus digest.
    let (_, mut expected) = fresh();
    expected
        .write16(0x0400_0084, 0x1234)
        .expect("expected sound register");
    expected
        .write32(FIFO_A_ADDR, 0x0403_0201)
        .expect("expected Sound A FIFO");
    expected
        .write16(DISPCNT_ADDR, 0x0080)
        .expect("expected dispcnt");
    assert_eq!(bus.state_digest(), expected.state_digest());
    // DISPCNT is not part of group 7; only the unconditional forced blank shows.
    assert_eq!(bus.read16(DISPCNT_ADDR).expect("dispcnt"), 0x0080);
}

#[test]
fn register_ram_reset_group_six_applies_known_sound_effects_only() {
    let (mut cpu, mut bus) = fresh();
    bus.write16(DISPCNT_ADDR, 0x0010).expect("dispcnt");
    bus.write16(BG0CNT_ADDR, 0x1234).expect("bg0cnt");
    bus.write16(0x0400_0060, 0x55AA)
        .expect("unresolved sound register");
    bus.write16(0x0400_0080, 0x1111).expect("sound control L");
    bus.write16(0x0400_0082, 0x2222).expect("sound control H");
    bus.write16(0x0400_0084, 0x1234).expect("sound register");
    bus.write16(0x0400_0088, 0xABCD).expect("sound bias");
    bus.write16(0x0400_0090, 0x3456).expect("wave RAM");
    bus.write32(0x0400_00A0, 0xDEAD_BEEF)
        .expect("Sound A FIFO state");
    bus.write32(FIFO_B_ADDR, 0xCAFE_BABE)
        .expect("Sound B FIFO state");
    bus.write16(0x0400_0102, 0x5678).expect("timer register");
    cpu.set_reg(0, 0x40);

    bios::call(bios::SERVICE_REGISTER_RAM_RESET, &mut cpu, &mut bus).expect("reset sound group");

    let (_, mut expected) = fresh();
    expected
        .write16(BG0CNT_ADDR, 0x1234)
        .expect("expected bg0cnt");
    expected
        .write16(0x0400_0060, 0x55AA)
        .expect("expected unresolved sound register");
    expected
        .write16(0x0400_0082, 0x880E)
        .expect("expected sound control H");
    expected
        .write16(0x0400_0088, 0x03CD)
        .expect("expected retained sound bias");
    expected
        .write16(0x0400_0102, 0x5678)
        .expect("expected timer register");
    expected
        .write16(DISPCNT_ADDR, 0x0080)
        .expect("expected forced blank");
    assert_eq!(bus.state_digest(), expected.state_digest());
    assert_eq!(bus.sound_fifo_len(SoundFifo::A), 0);
    assert_eq!(bus.sound_fifo_len(SoundFifo::B), 0);
}

#[test]
fn register_ram_reset_all_documented_groups_are_accepted() {
    let (mut cpu, mut bus) = fresh();
    seed(&mut bus);
    cpu.set_reg(0, 0xFF);
    bios::call(bios::SERVICE_REGISTER_RAM_RESET, &mut cpu, &mut bus).expect("reset 0xFF");
    assert_zero32(&bus, EWRAM_BASE, EWRAM_LEN);
    assert_zero16(&bus, PALETTE_BASE, PALETTE_LEN);
    assert_eq!(bus.read16(DISPCNT_ADDR).expect("dispcnt"), 0x0080);
}

#[test]
fn register_ram_reset_ignores_bits_above_the_documented_byte() {
    // The documented service examines only bits 0..7; higher bits are ignored
    // rather than rejected, so they select nothing and still force blank.
    let (mut cpu, mut bus) = fresh();
    bus.write32(EWRAM_BASE, 0xDEAD_BEEF).expect("seed");
    bus.write16(DISPCNT_ADDR, 0x0010).expect("dispcnt");
    cpu.set_reg(0, 0x0000_0100);
    bios::call(bios::SERVICE_REGISTER_RAM_RESET, &mut cpu, &mut bus).expect("reset bit 8");
    assert_eq!(bus.read32(EWRAM_BASE).expect("ewram"), 0xDEAD_BEEF);
    assert_eq!(bus.read16(DISPCNT_ADDR).expect("dispcnt"), 0x0080);
}
