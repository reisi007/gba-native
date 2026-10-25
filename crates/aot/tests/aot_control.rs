//! AOT discovery metadata for calls and PC writes.
//!
//! Synthetic ROM images only. The tests check that ARM BL follows the callee
//! and return address, Thumb BL stays two instructions with a dynamic callee
//! and a known return, and an ALU PC write terminates the straight-line block.

use gba_native::cart::Cartridge;
use gba_native::ir::Op;
use gba_native_aot::aot::{self, Limits};

fn cart_with(words: &[(usize, u32)]) -> Cartridge {
    let mut rom = vec![0u8; 0x200];
    for (offset, word) in words {
        rom[*offset..*offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    Cartridge::from_bytes(rom).expect("synthetic cart")
}

fn arm_branch(addr: u32, target: u32, link: bool) -> u32 {
    let difference = target as i64 - (addr as i64 + 8);
    let imm24 = ((difference >> 2) as u32) & 0x00FF_FFFF;
    0xE000_0000 | if link { 0x0B00_0000 } else { 0x0A00_0000 } | imm24
}

fn bx_lr() -> u32 {
    0xE12F_FF1E
}

fn ldr_literal(rd: u32, instruction_addr: u32, target: u32) -> u32 {
    let difference = target - (instruction_addr + 8);
    0xE59F_0000 | (rd << 12) | difference
}

fn and_imm(rd: u32, rn: u32, value: u32) -> u32 {
    0xE200_0000 | (rn << 16) | (rd << 12) | value
}

fn cmp_imm(rn: u32, value: u32) -> u32 {
    0xE350_0000 | (rn << 16) | value
}

#[test]
fn arm_bl_follows_callee_and_return_but_bx_lr_stays_dynamic() {
    let entry = 0x0800_0100;
    let callee = 0x0800_0120;
    let ret = 0x0800_0104;
    let cart = cart_with(&[
        (0x100, arm_branch(entry, callee, true)),
        (0x104, arm_branch(ret, ret, false)),
        (0x120, 0xE3A0_0001), // MOV r0,#1
        (0x124, bx_lr()),
    ]);
    let artifact = aot::discover(&cart, entry, false, Limits::default()).expect("discover");
    assert_eq!(artifact.stats.bl_calls, 1);
    assert!(artifact.stats.bl_returns >= 1);
    assert!(artifact.blocks.iter().any(|block| block.addr == callee));
    assert!(artifact.blocks.iter().any(|block| block.addr == ret));
    assert_eq!(artifact.stats.dynamic_bx, 1);
    assert_eq!(artifact.stats.resolved_bx, 0);
}

#[test]
fn alu_pc_write_terminates_the_block_and_is_not_followed_statically() {
    let entry = 0x0800_0100;
    let cart = cart_with(&[
        (0x100, 0xE1A0_F000), // MOV pc,r0
        (0x104, 0xE3A0_1001), // would be a valid next instruction
    ]);
    let artifact = aot::discover(&cart, entry, false, Limits::default()).expect("discover");
    assert_eq!(artifact.stats.pc_writes, 1);
    assert!(
        artifact
            .blocks
            .iter()
            .all(|block| block.addr != 0x0800_0104)
    );
    let first = artifact
        .blocks
        .iter()
        .find(|block| block.addr == entry)
        .expect("entry block");
    assert_eq!(first.instructions.len(), 1);
    assert!(first.instructions[0].is_control_flow());
}

#[test]
fn alu_write_invalidates_a_cached_bx_constant_but_cmp_keeps_it() {
    let entry = 0x0800_0100;
    let target = 0x0800_0140;

    let writing = cart_with(&[
        (0x100, ldr_literal(8, entry, 0x0800_0180)),
        (0x104, and_imm(8, 8, 0x0F)),
        (0x108, 0xE12F_FF18), // BX r8
        (0x140, 0xEAFF_FFFE), // B .
        (0x180, target),      // literal pool
    ]);
    let artifact =
        aot::discover(&writing, entry, false, Limits::default()).expect("discover ALU write");
    assert_eq!(artifact.stats.resolved_bx, 0);
    assert_eq!(artifact.stats.dynamic_bx, 1);
    assert!(artifact.blocks.iter().all(|block| block.addr != target));

    let comparing = cart_with(&[
        (0x100, ldr_literal(8, entry, 0x0800_0180)),
        (0x104, cmp_imm(8, 0)),
        (0x108, 0xE12F_FF18), // BX r8
        (0x140, 0xEAFF_FFFE), // B .
        (0x180, target),      // literal pool
    ]);
    let artifact =
        aot::discover(&comparing, entry, false, Limits::default()).expect("discover CMP");
    assert_eq!(artifact.stats.resolved_bx, 1);
    assert!(artifact.blocks.iter().any(|block| block.addr == target));
}

#[test]
fn thumb_bl_is_two_half_instructions_with_a_dynamic_callee() {
    let entry = 0x0800_0100;
    let ret = 0x0800_0104;
    let cart = cart_with(&[
        (0x100, 0x0000_F000), // BL first half (nn=0)
        (0x104, 0x0000_E7FE), // B . (also the return target)
    ]);
    // Overwrite the second half at 0x102 with the BL low halfword.
    let mut rom = cart.bytes().to_vec();
    rom[0x102..0x104].copy_from_slice(&(0xF800u16 | 0x20).to_le_bytes());
    let cart = Cartridge::from_bytes(rom).expect("patched synthetic cart");

    let artifact = aot::discover(&cart, entry, true, Limits::default()).expect("discover");
    assert_eq!(artifact.stats.dynamic_bl, 1);
    assert!(artifact.stats.bl_returns >= 1);
    let block = artifact
        .blocks
        .iter()
        .find(|block| block.addr == entry)
        .expect("entry block");
    assert_eq!(
        block.instructions.len(),
        2,
        "both halves in one straight line"
    );
    assert_eq!(block.instructions[0].op, Op::BlHigh { lr: entry + 4 });
    assert_eq!(block.instructions[1].op, Op::BlLow { offset: 0x40 });
    assert!(!block.instructions[0].is_control_flow());
    assert!(block.instructions[1].is_control_flow());
    assert!(artifact.blocks.iter().any(|block| block.addr == ret));
    // The dynamic callee is not assumed to be code.
    assert!(
        artifact
            .blocks
            .iter()
            .all(|block| block.addr != 0x0800_0144)
    );
}
