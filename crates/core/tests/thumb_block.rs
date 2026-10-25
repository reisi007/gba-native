//! Independent Thumb stack/block vectors: PUSH/POP ordering, LR/PC bits,
//! full-descending SP, LDMIA/STMIA writeback and base-in-list specials,
//! unaligned-base policy and non-atomic partial failure.

use gba_native_core::bus::{Bus, EWRAM_BASE, EWRAM_LEN};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::decode::decode_thumb;
use gba_native_core::error::Error;
use gba_native_core::interpret;
use gba_native_core::ir::Op;

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn thumb(halfword: u16) -> (Cpu, Bus) {
    let mut bus = Bus::new(cart());
    bus.write16(EWRAM_BASE, halfword)
        .expect("write instruction");
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_thumb(true);
    (cpu, bus)
}

fn push(regs: u8, lr: bool) -> u16 {
    0xB400 | (u16::from(lr) << 8) | u16::from(regs)
}

fn pop(regs: u8, pc: bool) -> u16 {
    0xBC00 | (u16::from(pc) << 8) | u16::from(regs)
}

fn ldm(base: u16, regs: u8) -> u16 {
    0xC800 | (base << 8) | u16::from(regs)
}

fn stm(base: u16, regs: u8) -> u16 {
    0xC000 | (base << 8) | u16::from(regs)
}

#[test]
fn push_pop_use_full_descending_increasing_memory_order_and_sp_restores() {
    let (mut cpu, mut bus) = thumb(push(0b101, true)); // r0, r2, LR
    cpu.set_reg(13, EWRAM_BASE + 0x100);
    cpu.set_reg(0, 0x11);
    cpu.set_reg(2, 0x22);
    cpu.set_reg(14, 0x33);
    interpret::step(&mut cpu, &mut bus).expect("PUSH");
    assert_eq!(bus.read32(EWRAM_BASE + 0xF4), Ok(0x11));
    assert_eq!(bus.read32(EWRAM_BASE + 0xF8), Ok(0x22));
    assert_eq!(bus.read32(EWRAM_BASE + 0xFC), Ok(0x33));
    assert_eq!(cpu.reg(13), EWRAM_BASE + 0xF4);
    assert_eq!(cpu.pc(), EWRAM_BASE + 2);

    let (mut cpu, mut bus) = thumb(pop(0b1010, true)); // r1, r3, PC
    cpu.set_reg(13, EWRAM_BASE + 0x100);
    bus.write32(EWRAM_BASE + 0x100, 0x44).expect("r1");
    bus.write32(EWRAM_BASE + 0x104, 0x55).expect("r3");
    bus.write32(EWRAM_BASE + 0x108, 0x0800_1235).expect("pc");
    interpret::step(&mut cpu, &mut bus).expect("POP PC");
    assert_eq!(cpu.reg(1), 0x44);
    assert_eq!(cpu.reg(3), 0x55);
    assert_eq!(cpu.pc(), 0x0800_1234, "POP PC clears bit 0");
    assert!(cpu.thumb(), "Thumb POP PC keeps Thumb state");
    assert_eq!(cpu.reg(13), EWRAM_BASE + 0x10C);

    // POP without the PC bit just increments SP.
    let (mut cpu, mut bus) = thumb(pop(0b0001, false));
    cpu.set_reg(13, EWRAM_BASE + 0x100);
    bus.write32(EWRAM_BASE + 0x100, 0x7777).expect("r0");
    interpret::step(&mut cpu, &mut bus).expect("POP");
    assert_eq!(cpu.reg(0), 0x7777);
    assert_eq!(cpu.reg(13), EWRAM_BASE + 0x104);
    assert_eq!(cpu.pc(), EWRAM_BASE + 2);
}

#[test]
fn empty_push_pop_and_ldm_stm_lists_are_explicit_errors() {
    for raw in [push(0, false), pop(0, false), ldm(0, 0), stm(0, 0)] {
        let (mut cpu, mut bus) = thumb(raw);
        let before = cpu.clone();
        let digest = bus.state_digest();
        let error = interpret::step(&mut cpu, &mut bus).expect_err("empty list");
        assert!(
            matches!(error, Error::UnsupportedInstruction { .. }),
            "{raw:04X}"
        );
        assert_eq!(cpu.pc(), before.pc());
        assert_eq!(cpu.reg(13), before.reg(13));
        assert_eq!(bus.state_digest(), digest);
    }
}

#[test]
fn ldm_stm_writeback_and_thumb_base_in_list_specials() {
    let base = EWRAM_BASE + 0x100;

    // STMIA with base first stores the OLD base, then writes the new base.
    let (mut cpu, mut bus) = thumb(stm(0, 0b0011));
    cpu.set_reg(0, base);
    cpu.set_reg(1, 0x22);
    interpret::step(&mut cpu, &mut bus).expect("STMIA base first");
    assert_eq!(bus.read32(base), Ok(base));
    assert_eq!(bus.read32(base + 4), Ok(0x22));
    assert_eq!(cpu.reg(0), base + 8);

    // STMIA with a later base stores the NEW base.
    let (mut cpu, mut bus) = thumb(stm(1, 0b0011));
    cpu.set_reg(0, 0x11);
    cpu.set_reg(1, base);
    interpret::step(&mut cpu, &mut bus).expect("STMIA base last");
    assert_eq!(bus.read32(base), Ok(0x11));
    assert_eq!(bus.read32(base + 4), Ok(base + 8));
    assert_eq!(cpu.reg(1), base + 8);

    // LDMIA with the base in the list: loaded value wins, no increment.
    let (mut cpu, mut bus) = thumb(ldm(0, 0b0011));
    bus.write32(base, 0xAAAA).expect("base value");
    bus.write32(base + 4, 0xBBBB).expect("second");
    cpu.set_reg(0, base);
    interpret::step(&mut cpu, &mut bus).expect("LDMIA base loaded");
    assert_eq!(cpu.reg(0), 0xAAAA);
    assert_eq!(cpu.reg(1), 0xBBBB);

    // LDMIA with the base absent increments.
    let (mut cpu, mut bus) = thumb(ldm(0, 0b0110));
    bus.write32(base, 0x1111).expect("first");
    bus.write32(base + 4, 0x2222).expect("second");
    cpu.set_reg(0, base);
    interpret::step(&mut cpu, &mut bus).expect("LDMIA base absent");
    assert_eq!(cpu.reg(1), 0x1111);
    assert_eq!(cpu.reg(2), 0x2222);
    assert_eq!(cpu.reg(0), base + 8);
}

#[test]
fn unaligned_sp_or_block_base_is_rejected_before_any_access() {
    let (mut cpu, mut bus) = thumb(push(0b0011, false));
    cpu.set_reg(13, EWRAM_BASE + 0x102);
    cpu.set_reg(0, 0x11);
    cpu.set_reg(1, 0x22);
    let digest = bus.state_digest();
    let error = interpret::step(&mut cpu, &mut bus).expect_err("unaligned SP");
    assert!(matches!(
        error,
        Error::UnalignedBlockBase { thumb: true, .. }
    ));
    assert_eq!(cpu.reg(13), EWRAM_BASE + 0x102);
    assert_eq!(bus.state_digest(), digest);

    let (mut cpu, mut bus) = thumb(ldm(0, 0b0011));
    cpu.set_reg(0, EWRAM_BASE + 0x102);
    let error = interpret::step(&mut cpu, &mut bus).expect_err("unaligned LDM base");
    assert!(matches!(
        error,
        Error::UnalignedBlockBase { thumb: true, .. }
    ));
    assert_eq!(cpu.reg(0), EWRAM_BASE + 0x102);
}

#[test]
fn partial_stack_and_block_failure_is_not_atomic() {
    let last_word = EWRAM_BASE + EWRAM_LEN as u32 - 4;

    // PUSH: SP + 4 makes the first of two words land in the last EWRAM word
    // and the second one past the end.
    let (mut cpu, mut bus) = thumb(push(0b0011, false));
    cpu.set_reg(13, last_word + 8);
    cpu.set_reg(0, 0xAAAA_AAAA);
    cpu.set_reg(1, 0xBBBB_BBBB);
    let error = interpret::step(&mut cpu, &mut bus).expect_err("PUSH over the end");
    assert!(matches!(error, Error::Unmapped { .. }));
    assert_eq!(bus.read32(last_word), Ok(0xAAAA_AAAA));
    assert_eq!(cpu.reg(13), last_word + 8, "SP is not committed");

    // POP: the first read succeeds, the second fails, SP is unchanged.
    let (mut cpu, mut bus) = thumb(pop(0b0110, false));
    cpu.set_reg(13, last_word);
    cpu.set_reg(1, 0x1111);
    cpu.set_reg(2, 0x2222);
    bus.write32(last_word, 0xCAFE_BABE).expect("last word");
    let error = interpret::step(&mut cpu, &mut bus).expect_err("POP over the end");
    assert!(matches!(error, Error::Unmapped { .. }));
    assert_eq!(cpu.reg(1), 0xCAFE_BABE);
    assert_eq!(cpu.reg(2), 0x2222);
    assert_eq!(cpu.reg(13), last_word);

    // LDM: first register loaded, second fails, base not committed.
    let (mut cpu, mut bus) = thumb(ldm(0, 0b0110));
    cpu.set_reg(0, last_word);
    cpu.set_reg(1, 0x1111);
    cpu.set_reg(2, 0x2222);
    bus.write32(last_word, 0x0BAD_F00D).expect("last word");
    let error = interpret::step(&mut cpu, &mut bus).expect_err("LDM over the end");
    assert!(matches!(error, Error::Unmapped { .. }));
    assert_eq!(cpu.reg(1), 0x0BAD_F00D);
    assert_eq!(cpu.reg(2), 0x2222);
    assert_eq!(cpu.reg(0), last_word);
}

#[test]
fn typed_ir_marks_control_flow_and_never_changes_flags() {
    let push_ir = decode_thumb(0, u32::from(push(0b0101, true))).expect("PUSH");
    assert_eq!(
        push_ir.op,
        Op::Push {
            regs: 0b0101,
            lr: true
        }
    );
    assert!(!push_ir.is_control_flow());

    let pop_ir = decode_thumb(0, u32::from(pop(0b0010, true))).expect("POP PC");
    assert_eq!(
        pop_ir.op,
        Op::Pop {
            regs: 0b0010,
            pc: true
        }
    );
    assert!(pop_ir.is_control_flow());

    let ldm_ir = decode_thumb(0, u32::from(ldm(3, 0b1000_0001))).expect("LDMIA");
    assert_eq!(
        ldm_ir.op,
        Op::ThumbBlock {
            load: true,
            rn: 3,
            regs: 0b1000_0001
        }
    );
    assert!(!ldm_ir.is_control_flow());

    let (mut cpu, mut bus) = thumb(stm(0, 0b0011));
    cpu.set_reg(0, EWRAM_BASE + 0x100);
    cpu.set_reg(1, 0x1234);
    cpu.set_nzcv(true, true, true, true);
    interpret::step(&mut cpu, &mut bus).expect("STMIA");
    assert_eq!(cpu.flags(), (true, true, true, true));
}
