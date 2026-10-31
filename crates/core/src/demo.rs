//! Fully synthetic direct-entry fixtures. All instruction words are our own;
//! the Nintendo logo region is zero and is never validated.
//!
//! The demo guest draws a deterministic complete 240x160 mode-3 bitmap by
//! executing ARM setup, an ARM-to-Thumb BX exchange, a Thumb pixel loop and a
//! Thumb BX back. The runner stops on its exact instruction budget before the
//! terminal ARM self-branch.

use crate::bus::Bus;
use crate::cart::{Cartridge, ROM_BASE};
use crate::cpu::Cpu;
use crate::display::{Image, rgb555_to_rgb888};
use crate::error::Result;
use crate::interpret;

pub mod input_tester;
pub mod save_tester;

pub use input_tester::{
    ENTRY as INPUT_TESTER_ENTRY, INPUT_TESTER_BUDGET, INPUT_TESTER_SAMPLE_MASK,
    expected_input_tester_image, input_tester_cartridge,
};
pub use save_tester::{SAVE_TESTER_BUDGET, expected_save_tester_image, save_tester_cartridge};

pub const ENTRY: u32 = ROM_BASE;
pub const DEMO_BUDGET: u64 = 230_411;
pub const FALLBACK_BUDGET: u64 = 16;
pub const FALLBACK_TARGET: u32 = 0x0800_0128;

const ROM_LEN: usize = 0x1000;
const DISPCNT: u32 = 0x0400_0000;
const DISPCNT_MODE3_BG2: u32 = 0x0000_0403;
const VRAM: u32 = 0x0600_0000;
const PIXEL_COUNT: u32 = 240 * 160;

pub fn demo_cartridge() -> Cartridge {
    let mut rom = vec![0u8; ROM_LEN];
    // Documented header entry point: one ARM branch past the zeroed logo area.
    write_arm(&mut rom, 0x00, arm_branch(0xE, 0x00, 0x100));

    write_arm(&mut rom, 0x100, arm_ldr_literal(0, 0x100, 0x138));
    write_arm(&mut rom, 0x104, arm_ldr_literal(1, 0x104, 0x13C));
    write_arm(&mut rom, 0x108, arm_strh(1, 0, 0));
    write_arm(&mut rom, 0x10C, arm_ldr_literal(0, 0x10C, 0x140));
    write_arm(&mut rom, 0x110, arm_ldr_literal(1, 0x110, 0x144));
    write_arm(&mut rom, 0x114, arm_mov_imm(2, 0));
    write_arm(&mut rom, 0x118, arm_ldr_literal(14, 0x118, 0x148));
    write_arm(&mut rom, 0x11C, arm_ldr_literal(12, 0x11C, 0x14C));
    write_arm(&mut rom, 0x120, arm_bx(12));
    write_arm(&mut rom, 0x124, arm_branch(0xE, 0x124, 0x124));

    write_thumb(&mut rom, 0x128, thumb_add_imm3(3, 2, 0));
    write_thumb(&mut rom, 0x12A, thumb_strh(3, 0, 0));
    write_thumb(&mut rom, 0x12C, thumb_add_imm8(0, 2));
    write_thumb(&mut rom, 0x12E, thumb_add_imm8(2, 1));
    write_thumb(&mut rom, 0x130, thumb_cmp_reg(2, 1));
    write_thumb(&mut rom, 0x132, thumb_bne(0x132, 0x128));
    write_thumb(&mut rom, 0x134, thumb_bx(14));

    write_word(&mut rom, 0x138, DISPCNT);
    write_word(&mut rom, 0x13C, DISPCNT_MODE3_BG2);
    write_word(&mut rom, 0x140, VRAM);
    write_word(&mut rom, 0x144, PIXEL_COUNT);
    write_word(&mut rom, 0x148, 0x0800_0124);
    write_word(&mut rom, 0x14C, 0x0800_0129);

    finish(rom, b"GBA-NATIVE", b"GBAD", b"00")
}

/// Synthetic fixture whose BX target is computed dynamically and is therefore
/// not statically discovered. It proves the native runtime's reference fallback.
pub fn fallback_cartridge() -> Cartridge {
    let mut rom = vec![0u8; ROM_LEN];
    write_arm(&mut rom, 0x00, arm_branch(0xE, 0x00, 0x100));

    write_arm(&mut rom, 0x100, arm_ldr_literal(0, 0x100, 0x138));
    write_arm(&mut rom, 0x104, arm_ldr_literal(5, 0x104, 0x13C));
    write_arm(&mut rom, 0x108, arm_strh(5, 0, 0));
    write_arm(&mut rom, 0x10C, arm_ldr_literal(3, 0x10C, 0x140));
    write_arm(&mut rom, 0x110, arm_mov_reg(1, 3));
    write_arm(&mut rom, 0x114, arm_cmp_imm(2, 0));
    write_arm(&mut rom, 0x118, arm_branch(0x1, 0x118, 0x124));
    write_arm(&mut rom, 0x11C, arm_bx(1));
    write_arm(&mut rom, 0x120, arm_branch(0xE, 0x120, 0x120));
    write_arm(&mut rom, 0x124, arm_branch(0xE, 0x124, 0x130));
    write_arm(&mut rom, 0x128, arm_add_imm(2, 2, 1));
    write_arm(&mut rom, 0x12C, arm_branch(0xE, 0x12C, 0x130));
    write_arm(&mut rom, 0x130, arm_mov_imm(4, 0x55));
    write_arm(&mut rom, 0x134, arm_branch(0xE, 0x134, 0x130));

    write_word(&mut rom, 0x138, DISPCNT);
    write_word(&mut rom, 0x13C, DISPCNT_MODE3_BG2);
    write_word(&mut rom, 0x140, FALLBACK_TARGET);

    finish(rom, b"FALLBACK", b"GBAD", b"00")
}

pub fn run_demo_reference() -> Result<(Cpu, Bus)> {
    run_reference(demo_cartridge(), DEMO_BUDGET)
}

pub fn run_fallback_reference() -> Result<(Cpu, Bus)> {
    run_reference(fallback_cartridge(), FALLBACK_BUDGET)
}

fn run_reference(cart: Cartridge, budget: u64) -> Result<(Cpu, Bus)> {
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(ENTRY);
    cpu.set_thumb(false);
    let mut bus = Bus::new(cart);
    interpret::run_budget(&mut cpu, &mut bus, budget)?;
    Ok((cpu, bus))
}

/// The expected full RGB image for the demo, derived from the guest's pixel
/// index formula, not from host-side VRAM painting.
pub fn expected_demo_image() -> Image {
    let mut pixels = Vec::with_capacity(PIXEL_COUNT as usize);
    for y in 0..160 {
        for x in 0..240 {
            let index = y * 240 + x;
            pixels.push(rgb555_to_rgb888(index as u16));
        }
    }
    Image {
        width: 240,
        height: 160,
        pixels,
    }
}

fn finish(mut rom: Vec<u8>, title: &[u8], game_code: &[u8], maker_code: &[u8]) -> Cartridge {
    assert!(title.len() <= 12);
    assert_eq!(game_code.len(), 4);
    assert_eq!(maker_code.len(), 2);
    rom[0xA0..0xAC].fill(0);
    rom[0xA0..0xA0 + title.len()].copy_from_slice(title);
    rom[0xAC..0xB0].copy_from_slice(game_code);
    rom[0xB0..0xB2].copy_from_slice(maker_code);
    rom[0xB2] = 0x96;
    rom[0xB3] = 0;
    rom[0xB4] = 0;
    rom[0xB5..0xBC].fill(0);
    rom[0xBC] = 0;
    rom[0xBD] = 0;
    rom[0xBE..0xC0].fill(0);
    let sum = rom[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    rom[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(rom).expect("synthetic ROM is valid")
}

fn write_arm(rom: &mut [u8], offset: usize, word: u32) {
    write_word(rom, offset, word);
}

fn write_thumb(rom: &mut [u8], offset: usize, halfword: u16) {
    rom[offset..offset + 2].copy_from_slice(&halfword.to_le_bytes());
}

fn write_word(rom: &mut [u8], offset: usize, word: u32) {
    assert_eq!(offset % 4, 0);
    rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
}

fn arm_ldr_literal(rd: u32, instr_addr: u32, target: u32) -> u32 {
    let base = instr_addr + 8;
    let (up, difference) = if target >= base {
        (true, target - base)
    } else {
        (false, base - target)
    };
    assert_eq!(difference % 4, 0);
    assert!(difference < 4096);
    0xE51F_0000 | ((up as u32) << 23) | (rd << 12) | difference
}

fn arm_branch(cond: u32, instr_addr: u32, target: u32) -> u32 {
    let difference = target as i64 - (instr_addr as i64 + 8);
    assert_eq!(difference % 4, 0);
    assert!((-0x0080_0000..=0x007F_FFFC).contains(&difference));
    let imm24 = ((difference >> 2) as u32) & 0x00FF_FFFF;
    (cond << 28) | 0x0A00_0000 | imm24
}

fn arm_bx(rm: u32) -> u32 {
    0xE12F_FF10 | rm
}

fn arm_mov_imm(rd: u32, imm8: u32) -> u32 {
    0xE3A0_0000 | (rd << 12) | imm8
}

fn arm_mov_reg(rd: u32, rm: u32) -> u32 {
    0xE1A0_0000 | (rd << 12) | rm
}

fn arm_add_imm(rd: u32, rn: u32, imm8: u32) -> u32 {
    0xE280_0000 | (rn << 16) | (rd << 12) | imm8
}

fn arm_cmp_imm(rn: u32, imm8: u32) -> u32 {
    0xE350_0000 | (rn << 16) | imm8
}

fn arm_ldrb(rd: u32, rn: u32, offset: u32) -> u32 {
    assert!(offset < 4096);
    0xE5D0_0000 | (rn << 16) | (rd << 12) | offset
}

fn arm_strb(rd: u32, rn: u32, offset: u32) -> u32 {
    assert!(offset < 4096);
    0xE5C0_0000 | (rn << 16) | (rd << 12) | offset
}

fn arm_strh(rd: u32, rn: u32, offset: u32) -> u32 {
    0xE1C0_00B0 | (rn << 16) | (rd << 12) | ((offset & 0xF0) << 4) | (offset & 0xF)
}

fn arm_ldrh(rd: u32, rn: u32, offset: u32) -> u32 {
    0xE1D0_00B0 | (rn << 16) | (rd << 12) | ((offset & 0xF0) << 4) | (offset & 0xF)
}

fn arm_strh_post(rd: u32, rn: u32, offset: u32) -> u32 {
    assert!(offset < 256);
    0xE0C0_00B2 | (rn << 16) | (rd << 12) | offset
}

fn arm_mvn_reg(rd: u32, rm: u32) -> u32 {
    0xE1E0_0000 | (rd << 12) | rm
}

fn arm_and_reg(rd: u32, rn: u32, rm: u32) -> u32 {
    0xE000_0000 | (rn << 16) | (rd << 12) | rm
}

fn arm_tst_imm(rn: u32, imm8: u32) -> u32 {
    0xE310_0000 | (rn << 16) | imm8
}

fn arm_moveq_reg(rd: u32, rm: u32) -> u32 {
    0x01A0_0000 | (rd << 12) | rm
}

fn arm_movne_reg(rd: u32, rm: u32) -> u32 {
    0x11A0_0000 | (rd << 12) | rm
}

fn arm_subs_imm(rd: u32, rn: u32, imm8: u32) -> u32 {
    0xE250_0000 | (rn << 16) | (rd << 12) | imm8
}

fn arm_mov_reg_lsr_imm(rd: u32, rm: u32, rotation: u32) -> u32 {
    assert!(rotation < 32);
    0xE1A0_0000 | (rd << 12) | (rotation << 7) | (1 << 5) | rm
}

/// `mov rd, #imm8, #rotation` with an even rotation in bits.
fn arm_mov_imm_rot(rd: u32, imm8: u32, rotation: u32) -> u32 {
    assert!(rotation < 32 && rotation.is_multiple_of(2));
    0xE3A0_0000 | (rd << 12) | ((rotation / 2) << 8) | imm8
}

fn thumb_add_imm3(rd: u32, rs: u32, imm3: u32) -> u16 {
    (0x1C00 | (imm3 << 6) | (rs << 3) | rd) as u16
}

fn thumb_add_imm8(rd: u32, imm8: u32) -> u16 {
    (0x3000 | (rd << 8) | imm8) as u16
}

fn thumb_strh(rd: u32, rb: u32, offset: u32) -> u16 {
    assert_eq!(offset % 2, 0);
    assert!(offset / 2 < 32);
    (0x8000 | ((offset / 2) << 6) | (rb << 3) | rd) as u16
}

fn thumb_cmp_reg(rd: u32, rs: u32) -> u16 {
    (0x4280 | (rs << 3) | rd) as u16
}

fn thumb_bne(instr_addr: u32, target: u32) -> u16 {
    let difference = target as i64 - (instr_addr as i64 + 4);
    assert_eq!(difference % 2, 0);
    let imm8 = difference / 2;
    assert!((-128..=127).contains(&imm8));
    let encoded = (imm8 as i8) as u8;
    0xD100u16 | u16::from(encoded)
}

fn thumb_bx(rm: u32) -> u16 {
    (0x4700 | (rm << 3)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_header_is_valid_and_sized() {
        let cart = demo_cartridge();
        assert_eq!(cart.len(), ROM_LEN);
        assert!(cart.header().fixed_ok);
        assert!(cart.header().complement_ok);
        assert_eq!(cart.header().title, "GBA-NATIVE");
        assert!(cart.bytes()[0x04..0xA0].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn demo_expected_image_formula() {
        let image = expected_demo_image();
        assert_eq!(image.pixels.len(), 38400);
        assert_eq!(image.pixel(0, 0), 0x000000);
        assert_eq!(image.pixel(31, 0), rgb555_to_rgb888(31));
        assert_eq!(image.pixel(0, 1), rgb555_to_rgb888(240));
    }
}
