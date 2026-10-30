//! Synthetic input-tester ROM.
//!
//! The guest sets Mode 3 and draws ten horizontal 16-pixel bands, one per
//! documented button in KEYINPUT order. A band is white while its host button
//! is held and dark otherwise. Reading is active-low on the guest side, so the
//! expected host image is a pure function of the pressed mask.
//!
//! This is our own fixture, not a retail ROM. It exists so the native AOT and
//! browser paths can prove real input handling with a deterministic frame.

use super::{
    Cartridge, Image, ROM_BASE, arm_and_reg, arm_branch, arm_ldr_literal, arm_ldrh,
    arm_mov_imm_rot, arm_mov_reg, arm_mov_reg_lsr_imm, arm_moveq_reg, arm_movne_reg, arm_mvn_reg,
    arm_strh, arm_strh_post, arm_subs_imm, arm_tst_imm, finish, rgb555_to_rgb888, write_arm,
    write_word,
};

/// Documented, deliberately small fixture image.
pub const INPUT_TESTER_LEN: usize = 0x1000;
/// Title stored in the synthetic header.
pub const INPUT_TESTER_TITLE: &str = "INPUTTEST";
/// Budget used by the committed expected-frame tests and the CI job.
pub const INPUT_TESTER_BUDGET: u64 = 20_000_000;
/// A fixed input mask used by the committed expected frame: A + B + Start.
pub const INPUT_TESTER_SAMPLE_MASK: u16 = 0x000B;

const DISPCNT: u32 = 0x0400_0000;
const DISPCNT_MODE3: u32 = 0x0000_0403;
const KEYINPUT: u32 = 0x0400_0130;
const VRAM: u32 = 0x0600_0000;
const DARK: u32 = 0x2108;
const LIGHT: u32 = 0x7FFF;

pub fn input_tester_cartridge() -> Cartridge {
    let mut rom = vec![0u8; INPUT_TESTER_LEN];
    write_arm(&mut rom, 0x00, arm_branch(0xE, 0x00, 0x100));

    // r3 = DISPCNT address, then Mode 3 + BG2.
    write_arm(&mut rom, 0x100, arm_ldr_literal(3, 0x100, 0x188));
    write_arm(&mut rom, 0x104, arm_ldr_literal(1, 0x104, 0x18C));
    write_arm(&mut rom, 0x108, arm_strh(1, 3, 0));
    write_arm(&mut rom, 0x10C, arm_ldr_literal(0, 0x10C, 0x190));
    write_arm(&mut rom, 0x110, arm_ldr_literal(2, 0x110, 0x194));
    write_arm(&mut rom, 0x114, arm_ldr_literal(4, 0x114, 0x198));
    write_arm(&mut rom, 0x118, arm_ldr_literal(5, 0x118, 0x19C));
    write_arm(&mut rom, 0x11C, arm_ldr_literal(6, 0x11C, 0x1A0));
    write_arm(&mut rom, 0x120, arm_ldr_literal(7, 0x120, 0x1A4));
    write_arm(&mut rom, 0x124, arm_ldr_literal(8, 0x124, 0x1A8));
    write_arm(&mut rom, 0x128, arm_ldr_literal(9, 0x128, 0x1AC));
    // Sound setup lives after the frame loop; the delay is reset each frame.
    write_arm(&mut rom, 0x12C, arm_branch(0xE, 0x12C, 0x1C0));

    // frame: r1 = ~KEYINPUT & 0x3FF, r3 = VRAM, r14 = 10 bands.
    write_arm(&mut rom, 0x130, arm_ldrh(1, 2, 0));
    write_arm(&mut rom, 0x134, arm_mvn_reg(1, 1));
    write_arm(&mut rom, 0x138, arm_and_reg(1, 1, 7));
    write_arm(&mut rom, 0x13C, arm_ldr_literal(3, 0x13C, 0x190));
    write_arm(&mut rom, 0x140, arm_mov_reg(14, 6));

    // band: colour = bit0 ? LIGHT : DARK, then 16 lines of 240 pixels.
    write_arm(&mut rom, 0x144, arm_tst_imm(1, 1));
    write_arm(&mut rom, 0x148, arm_moveq_reg(11, 8));
    write_arm(&mut rom, 0x14C, arm_movne_reg(11, 9));
    write_arm(&mut rom, 0x150, arm_mov_reg(12, 5));
    write_arm(&mut rom, 0x154, arm_mov_reg(0, 4));
    write_arm(&mut rom, 0x158, arm_strh_post(11, 3, 2));
    write_arm(&mut rom, 0x15C, arm_subs_imm(0, 0, 1));
    write_arm(&mut rom, 0x160, arm_branch(0x1, 0x160, 0x158));
    write_arm(&mut rom, 0x164, arm_subs_imm(12, 12, 1));
    write_arm(&mut rom, 0x168, arm_branch(0x1, 0x168, 0x154));
    write_arm(&mut rom, 0x16C, arm_mov_reg_lsr_imm(1, 1, 1));
    write_arm(&mut rom, 0x170, arm_subs_imm(14, 14, 1));
    write_arm(&mut rom, 0x174, arm_branch(0x1, 0x174, 0x144));

    // Delay about half a million instructions, then redraw with fresh keys.
    write_arm(&mut rom, 0x178, arm_mov_imm_rot(10, 1, 14));
    write_arm(&mut rom, 0x17C, arm_subs_imm(10, 10, 1));
    write_arm(&mut rom, 0x180, arm_branch(0x1, 0x180, 0x17C));
    write_arm(&mut rom, 0x184, arm_branch(0xE, 0x184, 0x130));

    write_word(&mut rom, 0x188, DISPCNT);
    write_word(&mut rom, 0x18C, DISPCNT_MODE3);
    write_word(&mut rom, 0x190, VRAM);
    write_word(&mut rom, 0x194, KEYINPUT);
    write_word(&mut rom, 0x198, 240);
    write_word(&mut rom, 0x19C, 16);
    write_word(&mut rom, 0x1A0, 10);
    write_word(&mut rom, 0x1A4, 0x03FF);
    write_word(&mut rom, 0x1A8, DARK);
    write_word(&mut rom, 0x1AC, LIGHT);
    write_word(&mut rom, 0x1B0, 0x0004_0000);

    // A fixed 440 Hz square tone makes the audio path audible and testable.
    write_arm(&mut rom, 0x1C0, arm_ldr_literal(3, 0x1C0, 0x1F4));
    write_arm(&mut rom, 0x1C4, arm_ldr_literal(1, 0x1C4, 0x1F8));
    write_arm(&mut rom, 0x1C8, arm_strh(1, 3, 0));
    write_arm(&mut rom, 0x1CC, arm_ldr_literal(3, 0x1CC, 0x1FC));
    write_arm(&mut rom, 0x1D0, arm_ldr_literal(1, 0x1D0, 0x200));
    write_arm(&mut rom, 0x1D4, arm_strh(1, 3, 0));
    write_arm(&mut rom, 0x1D8, arm_ldr_literal(3, 0x1D8, 0x204));
    write_arm(&mut rom, 0x1DC, arm_ldr_literal(1, 0x1DC, 0x208));
    write_arm(&mut rom, 0x1E0, arm_strh(1, 3, 0));
    write_arm(&mut rom, 0x1E4, arm_ldr_literal(3, 0x1E4, 0x20C));
    write_arm(&mut rom, 0x1E8, arm_ldr_literal(1, 0x1E8, 0x210));
    write_arm(&mut rom, 0x1EC, arm_strh(1, 3, 0));
    write_arm(&mut rom, 0x1F0, arm_branch(0xE, 0x1F0, 0x130));
    write_word(&mut rom, 0x1F4, 0x0400_0080);
    write_word(&mut rom, 0x1F8, 0x0000_0107);
    write_word(&mut rom, 0x1FC, 0x0400_0062);
    write_word(&mut rom, 0x200, 0x0000_F080);
    write_word(&mut rom, 0x204, 0x0400_0064);
    write_word(&mut rom, 0x208, 0x0000_86D6);
    write_word(&mut rom, 0x20C, 0x0400_0084);
    write_word(&mut rom, 0x210, 0x0000_0080);

    finish(rom, INPUT_TESTER_TITLE.as_bytes(), b"GBAD", b"00")
}

/// Expected frame for a pressed mask (one means pressed).
#[must_use]
pub fn expected_input_tester_image(pressed: u16) -> Image {
    let dark = rgb555_to_rgb888(DARK as u16);
    let light = rgb555_to_rgb888(LIGHT as u16);
    let mut pixels = Vec::with_capacity(240 * 160);
    for y in 0..160u16 {
        let row = y / 16;
        let colour = if row < 10 && (pressed >> row) & 1 != 0 {
            light
        } else {
            dark
        };
        pixels.extend(std::iter::repeat_n(colour, 240));
    }
    Image {
        width: 240,
        height: 160,
        pixels,
    }
}

/// ROM base entry point (documented header branch target).
pub const ENTRY: u32 = ROM_BASE;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;
    use crate::cpu::Cpu;
    use crate::display;
    use crate::input::{InputScript, run_scripted};

    fn run_idle(budget: u64) -> Bus {
        let cart = input_tester_cartridge();
        let mut cpu = Cpu::direct_entry();
        cpu.set_pc(ENTRY);
        cpu.set_thumb(false);
        let mut bus = Bus::new(cart);
        run_scripted(&mut cpu, &mut bus, budget, &InputScript::default()).unwrap();
        bus
    }

    #[test]
    fn header_is_valid_and_has_the_expected_title() {
        let cart = input_tester_cartridge();
        assert_eq!(cart.len(), INPUT_TESTER_LEN);
        assert!(cart.header().fixed_ok);
        assert!(cart.header().complement_ok);
        assert_eq!(cart.header().title, INPUT_TESTER_TITLE);
    }

    #[test]
    fn idle_frame_is_ten_dark_bands() {
        let bus = run_idle(2_000_000);
        let image = display::snapshot(&bus).expect("snapshot");
        assert_eq!(image, expected_input_tester_image(0));
    }

    #[test]
    fn pressed_mask_lights_the_matching_bands() {
        // A+B+Start = bits 0, 1 and 3.
        let mask = 0x000B;
        assert_eq!(INPUT_TESTER_SAMPLE_MASK, mask);
        let cart = input_tester_cartridge();
        let mut cpu = Cpu::direct_entry();
        cpu.set_pc(ENTRY);
        cpu.set_thumb(false);
        let mut bus = Bus::new(cart);
        let script = InputScript::parse("0:A+B+Start:2000000").unwrap();
        run_scripted(&mut cpu, &mut bus, 2_000_000, &script).unwrap();
        let image = display::snapshot(&bus).expect("snapshot");
        assert_eq!(image, expected_input_tester_image(mask));
        let dark = rgb555_to_rgb888(DARK as u16);
        let light = rgb555_to_rgb888(LIGHT as u16);
        assert_eq!(image.pixel(120, 0), light);
        assert_eq!(image.pixel(120, 16), light);
        assert_eq!(image.pixel(120, 32), dark);
        assert_eq!(image.pixel(120, 48), light);
        assert_eq!(image.pixel(120, 64), dark);
    }

    #[test]
    fn boot_setup_enables_a_square_tone() {
        use crate::audio::{AudioEngine, AudioSettings};
        let bus = run_idle(2_000_000);
        let regs = bus.sound_registers();
        assert_eq!(regs[0x24] & 0x80, 0x80, "master sound enable");
        let mut engine = AudioEngine::new(AudioSettings::new(100, false));
        engine.advance(&regs, 2048);
        assert!(engine.samples().iter().any(|sample| *sample != 0));
    }

    #[test]
    fn input_changes_are_picked_up_on_the_next_frame() {
        let cart = input_tester_cartridge();
        let mut cpu = Cpu::direct_entry();
        cpu.set_pc(ENTRY);
        cpu.set_thumb(false);
        let mut bus = Bus::new(cart);
        // Release everything in the final part of the run.
        let script = InputScript::parse("0:A:1500000\n1500000:START:500000").unwrap();
        run_scripted(&mut cpu, &mut bus, 2_000_000, &script).unwrap();
        let image = display::snapshot(&bus).expect("snapshot");
        assert_eq!(image, expected_input_tester_image(0x0008));
    }
}
