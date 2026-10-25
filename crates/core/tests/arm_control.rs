//! Independent ARM PC-operand, PC-write, branch and exchange vectors.
//!
//! Expected values follow the documented pipeline: PC reads as address+8, or
//! address+12 for a register-specified shift; ALU PC writes are force-aligned
//! and do not change state; S=1 with Rd=PC requires an SPSR restore and is
//! explicitly rejected.

use gba_native_core::bus::Bus;
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::decode::decode_arm;
use gba_native_core::error::Error;
use gba_native_core::interpret;
use gba_native_core::ir::{Op, Src};

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn dp(opcode: u32, set_flags: bool, rn: u32, rd: u32, op2: u32) -> u32 {
    0xE000_0000 | (opcode << 21) | ((set_flags as u32) << 20) | (rn << 16) | (rd << 12) | op2
}

fn imm(value: u32) -> u32 {
    (1 << 25) | value
}

fn shift_imm(kind: u32, amount: u32, rm: u32) -> u32 {
    (amount << 7) | (kind << 5) | rm
}

fn shift_reg(kind: u32, rs: u32, rm: u32) -> u32 {
    (rs << 8) | (kind << 5) | (1 << 4) | rm
}

fn branch(cond: u32, addr: u32, target: u32, link: bool) -> u32 {
    let difference = target as i64 - (addr as i64 + 8);
    assert_eq!(difference % 4, 0);
    let imm24 = ((difference >> 2) as u32) & 0x00FF_FFFF;
    (cond << 28) | if link { 0x0B00_0000 } else { 0x0A00_0000 } | imm24
}

fn bx(rm: u32) -> u32 {
    0xE12F_FF10 | rm
}

fn execute(raw: u32, addr: u32, thumb: bool, setup: impl FnOnce(&mut Cpu)) -> Cpu {
    let mut bus = Bus::new(cart());
    let instr = if thumb {
        gba_native_core::decode::decode_thumb(addr, raw & 0xFFFF).expect("Thumb decode")
    } else {
        decode_arm(addr, raw).expect("ARM decode")
    };
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(addr);
    cpu.set_thumb(thumb);
    setup(&mut cpu);
    interpret::execute(&mut cpu, &mut bus, instr).expect("execute");
    cpu
}

#[test]
fn arm_pc_operand_reads_use_plus_8_or_plus_12_for_register_shift() {
    let addr = 0x0800_0100;

    let cpu = execute(dp(13, false, 0, 0, 15), addr, false, |_| {});
    assert_eq!(cpu.reg(0), addr + 8, "MOV r0,PC");
    assert_eq!(cpu.pc(), addr + 4);

    let cpu = execute(dp(4, false, 15, 0, imm(4)), addr, false, |_| {});
    assert_eq!(cpu.reg(0), addr + 8 + 4, "ADD r0,PC,#4");

    let cpu = execute(
        dp(13, false, 0, 0, shift_imm(0, 2, 15)),
        addr,
        false,
        |_| {},
    );
    assert_eq!(cpu.reg(0), (addr + 8) << 2, "immediate shift still uses +8");

    let cpu = execute(
        dp(13, false, 0, 0, shift_reg(0, 2, 15)),
        addr,
        false,
        |cpu| {
            cpu.set_reg(2, 1);
        },
    );
    assert_eq!(
        cpu.reg(0),
        (addr + 12) << 1,
        "Rm=PC with register shift uses +12"
    );

    let cpu = execute(
        dp(4, false, 15, 0, shift_reg(0, 2, 1)),
        addr,
        false,
        |cpu| {
            cpu.set_reg(2, 2);
            cpu.set_reg(1, 3);
        },
    );
    assert_eq!(
        cpu.reg(0),
        (addr + 12).wrapping_add(3 << 2),
        "Rn=PC with register shift uses +12"
    );
}

#[test]
fn arm_alu_pc_writes_are_aligned_and_s1_pc_is_explicitly_unsupported() {
    let addr = 0x0800_0100;
    let cpu = execute(dp(13, false, 0, 15, 0), addr, false, |cpu| {
        cpu.set_reg(0, 0x0800_0206);
    });
    assert_eq!(cpu.pc(), 0x0800_0204, "MOV PC clears the low two bits");
    assert!(!cpu.thumb());

    let cpu = execute(dp(4, false, 1, 15, imm(4)), addr, false, |cpu| {
        cpu.set_reg(1, 0x0800_0202);
    });
    assert_eq!(cpu.pc(), 0x0800_0204, "ADD PC is force-aligned");

    let cpu = execute(dp(15, false, 0, 15, 0), addr, false, |cpu| {
        cpu.set_reg(0, 0x1234_5678);
    });
    assert_eq!(
        cpu.pc(),
        (!0x1234_5678u32) & !3,
        "MVN PC with S=0 is allowed and force-aligned"
    );

    for raw in [dp(13, true, 0, 15, 0), dp(4, true, 1, 15, 0)] {
        let error = decode_arm(addr, raw).expect_err("S=1 PC write needs SPSR restore");
        assert!(
            format!("{error}").contains("SPSR restore"),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn conditional_pc_write_is_a_noop_that_advances_pc() {
    let addr = 0x0800_0100;
    let raw = (dp(13, false, 0, 15, 0) & 0x0FFF_FFFF) | (0x1 << 28); // MOVNE PC,r0
    let cpu = execute(raw, addr, false, |cpu| {
        cpu.set_reg(0, 0x0800_0200);
        cpu.set_nzcv(false, true, false, false); // Z=1: NE false
    });
    assert_eq!(cpu.pc(), addr + 4);
}

#[test]
fn arm_branch_and_link_compute_target_and_link_register() {
    let addr = 0x0800_0100;
    let raw = branch(0xE, addr, 0x0800_0200, false);
    let instr = decode_arm(addr, raw).expect("B");
    assert_eq!(
        instr.op,
        Op::B {
            target: 0x0800_0200
        }
    );
    let cpu = execute(raw, addr, false, |_| {});
    assert_eq!(cpu.pc(), 0x0800_0200);

    let raw = branch(0xE, addr, 0x0800_0240, true);
    let instr = decode_arm(addr, raw).expect("BL");
    assert_eq!(
        instr.op,
        Op::Bl {
            target: 0x0800_0240
        }
    );
    let cpu = execute(raw, addr, false, |_| {});
    assert_eq!(cpu.pc(), 0x0800_0240);
    assert_eq!(cpu.reg(14), addr + 4, "BL links the next instruction");

    // Conditional BL with a false condition must not touch LR.
    let raw = branch(0x1, addr, 0x0800_0240, true); // BLNE
    let cpu = execute(raw, addr, false, |cpu| {
        cpu.set_reg(14, 0xDEAD_BEEF);
        cpu.set_nzcv(false, true, false, false); // Z=1: NE false
    });
    assert_eq!(cpu.pc(), addr + 4);
    assert_eq!(cpu.reg(14), 0xDEAD_BEEF);
}

#[test]
fn branch_targets_wrap_at_the_32_bit_boundary() {
    let addr = 0xFFFF_FFFC;
    let raw = branch(0xE, addr, addr, false);
    let instr = decode_arm(addr, raw).expect("wrapping B");
    assert_eq!(instr.op, Op::B { target: addr });
    let cpu = execute(raw, addr, false, |_| {});
    assert_eq!(cpu.pc(), addr);

    let raw = branch(0xE, addr, addr, true);
    let cpu = execute(raw, addr, false, |_| {});
    assert_eq!(cpu.pc(), addr);
    assert_eq!(cpu.reg(14), 0, "addr+4 wraps to zero");
}

#[test]
fn bx_exchanges_state_aligns_and_handles_pc_operand_as_documented() {
    // ARM BX to Thumb.
    let cpu = execute(bx(0), 0x0800_0100, false, |cpu| {
        cpu.set_reg(0, 0x0800_0201);
    });
    assert!(cpu.thumb());
    assert_eq!(cpu.pc(), 0x0800_0200);

    // ARM BX PC behaves as BX $+8 and stays in ARM state.
    let addr = 0x0800_0100;
    let cpu = execute(bx(15), addr, false, |_| {});
    assert!(!cpu.thumb());
    assert_eq!(cpu.pc(), addr + 8);

    // Thumb BX to ARM.
    let cpu = execute(0x4700, 0x0800_0200, true, |cpu| {
        cpu.set_reg(0, 0x0800_0300);
    });
    assert!(!cpu.thumb());
    assert_eq!(cpu.pc(), 0x0800_0300);

    // Thumb BX PC switches to ARM and aligns (($+4) AND NOT 2).
    let addr = 0x0800_0202;
    let cpu = execute(0x4778, addr, true, |_| {});
    assert!(!cpu.thumb());
    assert_eq!(cpu.pc(), ((addr + 4) & !2) & !1);
}

#[test]
fn armv5_exchange_encodings_are_not_accepted() {
    for raw in [0xE12F_FF30, 0xE12F_FF20] {
        let error = decode_arm(0x0800_0000, raw).expect_err("ARMv5 exchange rejected");
        assert!(matches!(error, Error::UnsupportedInstruction { .. }));
    }
    // Representative ARMv5/newer encodings must not fall through as ALU:
    // unconditional BLX-immediate space, CLZ, and the QADD/QSUB pattern.
    for raw in [0xFA00_0000, 0xE16F_0F10, 0xE100_0051] {
        let error = decode_arm(0x0800_0000, raw).expect_err("newer encoding rejected");
        assert!(matches!(error, Error::UnsupportedInstruction { .. }));
    }
}

#[test]
fn register_condition_false_never_writes_the_destination_or_pc_target() {
    let addr = 0x0800_0100;
    let raw = dp(13, false, 0, 0, 3) & 0x0FFF_FFFF; // MOVEQ r0,r3 (cond=0)
    let cpu = execute(raw, addr, false, |cpu| {
        cpu.set_reg(0, 0x1111_1111);
        cpu.set_reg(3, 0x2222_2222);
        cpu.set_nzcv(false, false, false, false); // Z=0: EQ false
    });
    assert_eq!(cpu.reg(0), 0x1111_1111);
    assert_eq!(cpu.pc(), addr + 4);
}

#[test]
fn typed_ir_marks_pc_writes_as_control_flow() {
    let addr = 0x0800_0100;
    for raw in [
        dp(13, false, 0, 15, 3),
        dp(4, false, 1, 15, 3),
        dp(0, false, 1, 15, 3),
        branch(0xE, addr, 0x0800_0200, true),
        bx(0),
    ] {
        let instr = decode_arm(addr, raw).expect("control instruction");
        assert!(instr.is_control_flow(), "raw={raw:#010X}");
    }
    for raw in [dp(13, false, 0, 1, 3), dp(2, false, 1, 1, 3)] {
        assert!(!decode_arm(addr, raw).expect("ALU").is_control_flow());
    }
    assert_eq!(
        decode_arm(addr, dp(13, false, 0, 4, 3)).expect("MOV").op,
        Op::Mov {
            rd: 4,
            src: Src::Reg(3),
            set_flags: false
        }
    );
}
