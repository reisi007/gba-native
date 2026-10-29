//! Synthetic ARM and Thumb guest programs that start and poll Timer 0.
//!
//! The programs are our own instruction words. They check that the reference
//! interpreter advances the instruction-budget timer model while executing
//! normal guest code, not only through direct `Bus::tick_frame` calls.

use gba_native_core::bus::Bus;
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::interpret;

const ENTRY: u32 = 0x0800_0000;
const TM0_L: u32 = 0x0400_0100;
const TM0_H: u32 = 0x0400_0102;
const BUDGET: u64 = 20;

fn run(rom: Vec<u8>, thumb: bool) -> (Cpu, Bus) {
    let cart = Cartridge::from_bytes(rom).expect("synthetic timer guest");
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(ENTRY);
    cpu.set_thumb(thumb);
    let mut bus = Bus::new(cart);
    assert_eq!(
        interpret::run_budget(&mut cpu, &mut bus, BUDGET),
        Ok(BUDGET)
    );
    (cpu, bus)
}

fn arm_program() -> Vec<u8> {
    let mut rom = vec![0u8; 0x200];
    let words: [u32; 8] = [
        0xE59F_0078, // 0x00 LDR r0,[pc,#0x78] -> 0x08000080
        0xE3A0_1000, // 0x04 MOV r1,#0
        0xE1C0_10B0, // 0x08 STRH r1,[r0] (reload)
        0xE59F_2070, // 0x0C LDR r2,[pc,#0x70] -> 0x08000084
        0xE3A0_3080, // 0x10 MOV r3,#0x80 (start, F/1)
        0xE1C2_30B0, // 0x14 STRH r3,[r2] (enable)
        0xE1D0_40B0, // 0x18 LDRH r4,[r0] (poll)
        0xEAFF_FFFE, // 0x1C B .
    ];
    for (index, word) in words.iter().enumerate() {
        rom[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    rom[0x80..0x84].copy_from_slice(&TM0_L.to_le_bytes());
    rom[0x84..0x88].copy_from_slice(&TM0_H.to_le_bytes());
    rom
}

fn thumb_program() -> Vec<u8> {
    let mut rom = vec![0u8; 0x200];
    let halfwords: [u16; 8] = [
        0x4810, // 0x00 LDR r0,[pc,#0x40] -> 0x08000044
        0x2100, // 0x02 MOV r1,#0
        0x8001, // 0x04 STRH r1,[r0] (reload)
        0x4A10, // 0x06 LDR r2,[pc,#0x40] -> 0x08000048
        0x2380, // 0x08 MOV r3,#0x80 (start, F/1)
        0x8013, // 0x0A STRH r3,[r2] (enable)
        0x8804, // 0x0C LDRH r4,[r0] (poll)
        0xE7FE, // 0x0E B .
    ];
    for (index, halfword) in halfwords.iter().enumerate() {
        rom[index * 2..index * 2 + 2].copy_from_slice(&halfword.to_le_bytes());
    }
    rom[0x44..0x48].copy_from_slice(&TM0_L.to_le_bytes());
    rom[0x48..0x4C].copy_from_slice(&TM0_H.to_le_bytes());
    rom
}

fn assert_polled_three_cycles(cpu: &Cpu, bus: &Bus, expected_pc: u32) {
    assert_eq!(cpu.reg(4), 3, "the poll sees one enabled instruction tick");
    assert_eq!(
        bus.read16(TM0_L).expect("timer counter"),
        45,
        "15 enabled instruction ticks at three cycles each"
    );
    assert_eq!(
        bus.read16(0x0400_0202).expect("IF") & (1 << 3),
        0,
        "the fixture enables no timer IRQ"
    );
    assert_eq!(cpu.pc(), expected_pc);
}

#[test]
fn arm_guest_starts_and_polls_the_instruction_budget_timer() {
    let (cpu, bus) = run(arm_program(), false);
    assert_polled_three_cycles(&cpu, &bus, 0x0800_001C);
}

#[test]
fn thumb_guest_starts_and_polls_the_instruction_budget_timer() {
    let (cpu, bus) = run(thumb_program(), true);
    assert_polled_three_cycles(&cpu, &bus, 0x0800_000E);
}
