//! Independent Thumb long-branch (two-half BL) vectors.
//!
//! Each half is one instruction with its own LR update and budget tick. The
//! first half never branches; the second half computes its target from the
//! current LR and sets the Thumb return address.

use gba_native::bus::{Bus, EWRAM_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::decode::decode_thumb;
use gba_native::error::Error;
use gba_native::interpret;
use gba_native::ir::Op;

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn bl_high(nn: u16) -> u16 {
    assert!(nn < 0x800);
    0xF000 | nn
}

fn bl_low(nn: u16) -> u16 {
    assert!(nn < 0x800);
    0xF800 | nn
}

fn thumb_with(halfwords: &[u16]) -> (Cpu, Bus) {
    let mut bus = Bus::new(cart());
    for (index, halfword) in halfwords.iter().enumerate() {
        bus.write16(EWRAM_BASE + index as u32 * 2, *halfword)
            .expect("write Thumb code");
    }
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_thumb(true);
    (cpu, bus)
}

#[test]
fn bl_halves_have_independent_lr_updates_and_an_exact_budget_stop() {
    let (mut cpu, mut bus) = thumb_with(&[bl_high(0), bl_low(4), 0xE7FE]);
    let first = decode_thumb(EWRAM_BASE, bl_high(0) as u32).expect("BL first half");
    assert_eq!(first.op, Op::BlHigh { lr: EWRAM_BASE + 4 });
    assert!(!first.is_control_flow(), "the first half must not branch");

    // Budget 1 stops exactly between the two halves.
    let used = interpret::run_budget(&mut cpu, &mut bus, 1).expect("first half");
    assert_eq!(used, 1);
    assert_eq!(cpu.pc(), EWRAM_BASE + 2, "PC is the second half");
    assert_eq!(cpu.reg(14), EWRAM_BASE + 4, "intermediate LR");
    assert!(cpu.thumb());

    let second = decode_thumb(EWRAM_BASE + 2, bl_low(4) as u32).expect("BL second half");
    assert_eq!(second.op, Op::BlLow { offset: 8 });
    assert!(second.is_control_flow());

    let used = interpret::run_budget(&mut cpu, &mut bus, 1).expect("second half");
    assert_eq!(used, 1);
    assert_eq!(
        cpu.pc(),
        EWRAM_BASE + 12,
        "LR + low offset, halfword aligned"
    );
    assert_eq!(
        cpu.reg(14),
        (EWRAM_BASE + 4) | 1,
        "return address keeps Thumb bit"
    );
    assert!(cpu.thumb(), "BL does not change instruction state");
}

#[test]
fn second_half_alone_uses_the_current_lr_and_wraps() {
    let (mut cpu, mut bus) = thumb_with(&[bl_low(1)]);
    cpu.set_reg(14, 0xFFFF_FFFE);
    interpret::step(&mut cpu, &mut bus).expect("second half");
    assert_eq!(cpu.pc(), 0x0000_0000, "LR + 2 wraps and is aligned");
    assert_eq!(cpu.reg(14), (EWRAM_BASE + 2) | 1);
}

#[test]
fn negative_high_offset_is_sign_extended_from_eleven_bits() {
    let addr = 0x0800_1000;
    let instr = decode_thumb(addr, bl_high(0x7FF) as u32).expect("BL high -1");
    assert_eq!(
        instr.op,
        Op::BlHigh {
            lr: addr.wrapping_add(4).wrapping_sub(0x1000)
        }
    );
}

#[test]
fn blx_second_half_and_register_are_not_accepted() {
    let error = decode_thumb(0, 0xE800).expect_err("ARMv5 BLX second half");
    assert!(matches!(error, Error::UnsupportedInstruction { .. }));
    // 0xF800 is the valid BL second half and is decoded independently.
    assert_eq!(
        decode_thumb(0, 0xF800).expect("BL second half").op,
        Op::BlLow { offset: 0 }
    );
}

#[test]
fn conditional_branch_and_bl_do_not_write_the_same_state_when_false() {
    let mut bus = Bus::new(cart());
    bus.write16(EWRAM_BASE, 0xD1FE).expect("BNE -2"); // Z=1 -> false
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_thumb(true);
    cpu.set_nzcv(false, true, false, false);
    let before = cpu.clone();
    interpret::step(&mut cpu, &mut bus).expect("false branch");
    assert_eq!(cpu.pc(), EWRAM_BASE + 2);
    assert_eq!(cpu.cpsr(), before.cpsr());
    assert_eq!(cpu.reg(14), 0);
}
