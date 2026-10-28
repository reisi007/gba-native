//! Instruction-level tests written from raw encodings, independent of the
//! demo assembler and of the AOT emitter.

use gba_native_core::bus::{Bus, EWRAM_BASE};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::error::Error;
use gba_native_core::interpret;

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn arm(words: &[u32]) -> (Cpu, Bus) {
    let mut bus = Bus::new(cart());
    for (index, word) in words.iter().enumerate() {
        bus.write32(EWRAM_BASE + index as u32 * 4, *word)
            .expect("write code");
    }
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_thumb(false);
    (cpu, bus)
}

fn thumb(halfwords: &[u16]) -> (Cpu, Bus) {
    let mut bus = Bus::new(cart());
    for (index, halfword) in halfwords.iter().enumerate() {
        bus.write16(EWRAM_BASE + index as u32 * 2, *halfword)
            .expect("write code");
    }
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_thumb(true);
    (cpu, bus)
}

fn steps(cpu: &mut Cpu, bus: &mut Bus, count: usize) {
    for _ in 0..count {
        interpret::step(cpu, bus).expect("instruction");
    }
}

#[test]
fn arm_mov_add_cmp_and_conditional_execution() {
    let (mut cpu, mut bus) = arm(&[
        0xE3A0_0001, // MOV r0,#1
        0xE280_0002, // ADD r0,r0,#2
        0xE350_0003, // CMP r0,#3
        0x03A0_1007, // MOVEQ r1,#7
        0xE3A0_2009, // MOV r2,#9
    ]);
    steps(&mut cpu, &mut bus, 5);
    assert_eq!(cpu.reg(0), 3);
    assert_eq!(cpu.reg(1), 7);
    assert_eq!(cpu.reg(2), 9);
    assert_eq!(cpu.flags(), (false, true, true, false));
    assert_eq!(cpu.pc(), EWRAM_BASE + 20);
    assert!(!cpu.thumb());
}

#[test]
fn false_condition_is_a_no_op_but_advances_pc() {
    let (mut cpu, mut bus) = arm(&[
        0xE3A0_0001, // MOV r0,#1
        0xE350_0002, // CMP r0,#2
        0x03A0_1007, // MOVEQ r1,#7 (not taken)
    ]);
    steps(&mut cpu, &mut bus, 3);
    assert_eq!(cpu.reg(1), 0);
    assert_eq!(cpu.flags(), (true, false, false, false));
    assert_eq!(cpu.pc(), EWRAM_BASE + 12);
}

#[test]
fn add_sub_carry_and_overflow_flags_use_literal_value() {
    let (mut cpu, mut bus) = arm(&[
        0xE59F_0004, // LDR r0,[pc,#4] -> EWRAM+12
        0xE290_1001, // ADDS r1,r0,#1
        0xE251_2001, // SUBS r2,r1,#1
        0xFFFF_FFFF, // literal
    ]);
    steps(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.reg(1), 0);
    assert_eq!(cpu.flags(), (false, true, true, false));
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.reg(2), 0xFFFF_FFFF);
    assert_eq!(cpu.flags(), (true, false, false, false));
}

#[test]
fn unaligned_literal_word_load_rotates() {
    let (mut cpu, mut bus) = arm(&[
        0xE59F_0001, // LDR r0,[pc,#1] -> effective EWRAM+9
        0xE1A0_0000,
        0x1122_3344, // aligned word at EWRAM+8
    ]);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.reg(0), 0x4411_2233);
}

#[test]
fn halfword_load_and_store_follow_arm7_odd_address_rules() {
    let (mut cpu, mut bus) = arm(&[
        0xE59F_0004, // LDR r0,[pc,#4] -> EWRAM+12
        0xE1D0_10B0, // LDRH r1,[r0] (odd)
        0xE1C0_10B0, // STRH r1,[r0] (forced align)
        EWRAM_BASE + 0x103,
    ]);
    bus.write16(EWRAM_BASE + 0x102, 0xABCD).expect("seed");
    steps(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.reg(1), 0xCD00_00AB);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(bus.read16(EWRAM_BASE + 0x102), Ok(0x00AB));
}

#[test]
fn arm_b_and_thumb_b_skip_the_following_instruction() {
    let (mut cpu, mut bus) = arm(&[
        0xEA00_0000, // B EWRAM+8
        0xE3A0_0001, // skipped
        0xE3A0_1002, // MOV r1,#2
    ]);
    steps(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.reg(0), 0);
    assert_eq!(cpu.reg(1), 2);
    assert_eq!(cpu.pc(), EWRAM_BASE + 12);

    let (mut cpu, mut bus) = thumb(&[
        0xE000, // B EWRAM+4
        0x2101, // skipped MOV r1,#1
        0x2202, // MOV r2,#2
    ]);
    steps(&mut cpu, &mut bus, 2);
    assert_eq!(cpu.reg(1), 0);
    assert_eq!(cpu.reg(2), 2);
    assert_eq!(cpu.pc(), EWRAM_BASE + 6);
}

#[test]
fn arm_thumb_bx_round_trip_preserves_state_distinction() {
    let (mut cpu, mut bus) = arm(&[
        0xE59F_E010, // LDR lr,[pc,#0x10] -> EWRAM+0x18
        0xE59F_0010, // LDR r0,[pc,#0x10] -> EWRAM+0x1C
        0xE12F_FF10, // BX r0
        0xE3A0_3055, // MOV r3,#0x55 (executed after return)
        0xEAFF_FFFE, // B self
        0xE1A0_0000,
        0xE1A0_0000,
        0, // literal lr
        0, // literal r0
    ]);
    bus.write32(EWRAM_BASE + 0x18, EWRAM_BASE + 0x0C)
        .expect("lr literal");
    bus.write32(EWRAM_BASE + 0x1C, EWRAM_BASE + 0x21)
        .expect("thumb literal");
    bus.write16(EWRAM_BASE + 0x20, 0x212A).expect("thumb mov");
    bus.write16(EWRAM_BASE + 0x22, 0x4770).expect("thumb bx");

    steps(&mut cpu, &mut bus, 3);
    assert!(cpu.thumb());
    assert_eq!(cpu.pc(), EWRAM_BASE + 0x20);
    steps(&mut cpu, &mut bus, 2);
    assert!(!cpu.thumb());
    assert_eq!(cpu.pc(), EWRAM_BASE + 0x0C);
    assert_eq!(cpu.reg(1), 0x2A);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.reg(3), 0x55);
    assert_eq!(cpu.pc(), EWRAM_BASE + 0x10);
}

#[test]
fn thumb_high_register_mov_and_cmp_use_msb_bits() {
    let (mut cpu, mut bus) = thumb(&[
        0x4640, // MOV r0,r8
        0x4680, // MOV r8,r0
        0x4540, // CMP r0,r8
    ]);
    cpu.set_reg(8, 0x1234);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.reg(0), 0x1234);
    cpu.set_reg(0, 0x55);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.reg(8), 0x55);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.flags(), (false, true, true, false));
}

#[test]
fn thumb_immediate_arithmetic_and_conditional_branch() {
    let (mut cpu, mut bus) = thumb(&[
        0x2005, // MOV r0,#5
        0x3002, // ADD r0,#2
        0x2807, // CMP r0,#7
        0xD000, // BEQ +0 -> skip next
        0x2101, // skipped
        0x2202, // MOV r2,#2
    ]);
    steps(&mut cpu, &mut bus, 4);
    assert_eq!(cpu.reg(0), 7);
    assert_eq!(cpu.reg(1), 0);
    assert_eq!(cpu.flags(), (false, true, true, false));
    assert_eq!(cpu.pc(), EWRAM_BASE + 10);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.reg(2), 2);
    assert_eq!(cpu.pc(), EWRAM_BASE + 12);
}

#[test]
fn arm_and_thumb_multiply_keep_the_low_32_bits() {
    // THUMB MUL r2,r3 after r2=7, r3=6.
    let (mut cpu, mut bus) = thumb(&[0x2207, 0x2306, 0x435A]);
    steps(&mut cpu, &mut bus, 3);
    assert_eq!(cpu.reg(2), 42);

    // 0x10001 * 0x10001 = 0x1_0002_0001, truncated to the low word.
    let (mut cpu, mut bus) = arm(&[0xE000_0291]); // MUL r0,r1,r2
    cpu.set_reg(1, 0x1_0001);
    cpu.set_reg(2, 0x1_0001);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.reg(0), 0x0002_0001);

    // MLA r0,r1,r2,r3 = r1*r2 + r3.
    let (mut cpu, mut bus) = arm(&[0xE020_3291]);
    cpu.set_reg(1, 10);
    cpu.set_reg(2, 20);
    cpu.set_reg(3, 5);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.reg(0), 205);
}

#[test]
fn unsupported_encodings_are_rejected_without_state_progress() {
    let (mut cpu, mut bus) = arm(&[0xE080_0091]); // UMULL
    let error = interpret::step(&mut cpu, &mut bus).expect_err("multiply rejected");
    assert!(matches!(
        error,
        Error::UnsupportedInstruction {
            raw: 0xE080_0091,
            thumb: false,
            ..
        }
    ));
    assert_eq!(cpu.pc(), EWRAM_BASE);

    let (mut cpu, mut bus) = arm(&[0xE1B0_F000]); // MOVS pc,r0 needs SPSR restore
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::UnsupportedInstruction { .. })
    ));

    // A word LDR PC is implemented; the narrow byte form into PC stays an
    // explicit unsupported encoding.
    let (mut cpu, mut bus) = arm(&[0xE5DF_F000]); // LDRB pc,[pc]
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::UnsupportedInstruction { .. })
    ));

    let (mut cpu, mut bus) = arm(&[0xE10F_F000]); // MRS pc,CPSR is malformed
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::UnsupportedInstruction { .. })
    ));

    let (mut cpu, mut bus) = arm(&[0xE129_0000]); // MSR fixed Rd-field != 1111
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::UnsupportedInstruction { .. })
    ));

    let (mut cpu, mut bus) = arm(&[0xE12F_FF30]); // BLX register
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::UnsupportedInstruction { .. })
    ));

    let (mut cpu, mut bus) = arm(&[0xE140_0001]); // CMP with S=0 (malformed)
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::UnsupportedInstruction { .. })
    ));

    let (mut cpu, mut bus) = thumb(&[0xB400]); // PUSH
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::UnsupportedInstruction {
            raw: 0xB400,
            thumb: true,
            ..
        })
    ));

    let (mut cpu, mut bus) = thumb(&[0x4780]); // Thumb BLX register (ARMv5)
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::UnsupportedInstruction { .. })
    ));

    let (mut cpu, mut bus) = thumb(&[0x4400]); // reserved low/low high-register ADD
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::UnsupportedInstruction { .. })
    ));
}

#[test]
fn unaligned_and_nonexecutable_fetches_fail_explicitly() {
    let (mut cpu, mut bus) = arm(&[0xE3A0_0001]);
    cpu.set_pc(EWRAM_BASE + 2);
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::UnalignedFetch { thumb: false, .. })
    ));

    cpu.set_pc(0x0400_0000);
    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::NonExecutable { .. })
    ));
}

#[test]
fn ram_code_is_decoded_fresh_after_self_modification() {
    let (mut cpu, mut bus) = thumb(&[0x3001]); // ADD r0,#1
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.reg(0), 1);
    bus.write16(EWRAM_BASE, 0x3002).expect("patch"); // ADD r0,#2
    cpu.set_pc(EWRAM_BASE);
    steps(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.reg(0), 3);
}
