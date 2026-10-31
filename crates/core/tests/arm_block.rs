//! Independent ARM block-transfer vectors: IA/IB/DA/DB ordering, writeback,
//! ARMv4 base-in-list specials, PC entries, condition-false behavior and the
//! documented non-atomic partial-failure policy.

use gba_native_core::bus::{Bus, EWRAM_BASE, OAM_BASE, OAM_LEN};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::decode::decode_arm;
use gba_native_core::error::Error;
use gba_native_core::interpret;
use gba_native_core::ir::Op;

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0u8; 0x200]).expect("synthetic cart")
}

fn arm(word: u32) -> (Cpu, Bus) {
    let mut bus = Bus::new(cart());
    bus.write32(EWRAM_BASE, word).expect("write instruction");
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_thumb(false);
    (cpu, bus)
}

#[allow(clippy::too_many_arguments)]
fn block(cond: u32, p: u32, u: u32, s: u32, w: u32, l: u32, rn: u32, regs: u32) -> u32 {
    (cond << 28)
        | 0x0800_0000
        | (p << 24)
        | (u << 23)
        | (s << 22)
        | (w << 21)
        | (l << 20)
        | (rn << 16)
        | (regs & 0xFFFF)
}

#[test]
fn stm_modes_write_ascending_registers_from_the_documented_lowest_address() {
    let base = EWRAM_BASE + 0x100;
    for (p, u, start, label) in [
        (0, 1, base, "STMIA"),
        (1, 1, base + 4, "STMIB"),
        (0, 0, base - 8, "STMDA"),
        (1, 0, base - 12, "STMDB"),
    ] {
        let (mut cpu, mut bus) = arm(block(0xE, p, u, 0, 0, 0, 0, 0b1110));
        cpu.set_reg(0, base);
        cpu.set_reg(1, 0x11);
        cpu.set_reg(2, 0x22);
        cpu.set_reg(3, 0x33);
        let flags = cpu.flags();
        interpret::step(&mut cpu, &mut bus).expect(label);
        assert_eq!(bus.read32(start), Ok(0x11), "{label}");
        assert_eq!(bus.read32(start + 4), Ok(0x22), "{label}");
        assert_eq!(bus.read32(start + 8), Ok(0x33), "{label}");
        assert_eq!(cpu.reg(0), base, "{label} has no writeback");
        assert_eq!(cpu.pc(), EWRAM_BASE + 4);
        assert_eq!(cpu.flags(), flags);
    }
}

#[test]
fn ldm_modes_read_ascending_registers_from_the_documented_lowest_address() {
    let base = EWRAM_BASE + 0x100;
    for (p, u, start, label) in [
        (0, 1, base, "LDMIA"),
        (1, 1, base + 4, "LDMIB"),
        (0, 0, base - 8, "LDMDA"),
        (1, 0, base - 12, "LDMDB"),
    ] {
        let (mut cpu, mut bus) = arm(block(0xE, p, u, 0, 0, 1, 0, 0b1110));
        bus.write32(start, 0x11).expect("first");
        bus.write32(start + 4, 0x22).expect("second");
        bus.write32(start + 8, 0x33).expect("third");
        cpu.set_reg(0, base);
        interpret::step(&mut cpu, &mut bus).expect(label);
        assert_eq!(cpu.reg(1), 0x11, "{label}");
        assert_eq!(cpu.reg(2), 0x22, "{label}");
        assert_eq!(cpu.reg(3), 0x33, "{label}");
        assert_eq!(cpu.reg(0), base);
        assert_eq!(cpu.pc(), EWRAM_BASE + 4);
    }
}

#[test]
fn armv4_base_in_list_writeback_specials_are_exact() {
    let base = EWRAM_BASE + 0x100;

    // Base is the first (lowest) list entry: the OLD base is stored.
    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 1, 0, 0, 0b00111));
    cpu.set_reg(0, base);
    cpu.set_reg(1, 0x11);
    cpu.set_reg(2, 0x22);
    interpret::step(&mut cpu, &mut bus).expect("STMIA base first");
    assert_eq!(bus.read32(base), Ok(base));
    assert_eq!(bus.read32(base + 4), Ok(0x11));
    assert_eq!(bus.read32(base + 8), Ok(0x22));
    assert_eq!(cpu.reg(0), base + 12);

    // Base r2 is NOT first: the NEW base is stored.
    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 1, 0, 2, 0b00111));
    cpu.set_reg(0, 0x11);
    cpu.set_reg(1, 0x22);
    cpu.set_reg(2, base);
    interpret::step(&mut cpu, &mut bus).expect("STMIA base last");
    assert_eq!(bus.read32(base), Ok(0x11));
    assert_eq!(bus.read32(base + 4), Ok(0x22));
    assert_eq!(bus.read32(base + 8), Ok(base + 12));
    assert_eq!(cpu.reg(2), base + 12);

    // LDM with the base in the list: the loaded value wins, no extra increment.
    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 1, 1, 0, 0b0011));
    bus.write32(base, 0xAAAA).expect("loaded base");
    bus.write32(base + 4, 0xBBBB).expect("second");
    cpu.set_reg(0, base);
    interpret::step(&mut cpu, &mut bus).expect("LDMIA base loaded");
    assert_eq!(cpu.reg(0), 0xAAAA);
    assert_eq!(cpu.reg(1), 0xBBBB);

    // LDM with the base absent commits the increment.
    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 1, 1, 0, 0b0110));
    bus.write32(base, 0x1111).expect("first");
    bus.write32(base + 4, 0x2222).expect("second");
    cpu.set_reg(0, base);
    interpret::step(&mut cpu, &mut bus).expect("LDMIA base absent");
    assert_eq!(cpu.reg(1), 0x1111);
    assert_eq!(cpu.reg(2), 0x2222);
    assert_eq!(cpu.reg(0), base + 8);
}

#[test]
fn pc_in_list_load_aligns_and_store_writes_instruction_address_plus_twelve() {
    let base = EWRAM_BASE + 0x100;
    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 0, 1, 0, 0x8002));
    bus.write32(base, 0x1234).expect("r1");
    bus.write32(base + 4, 0x0800_1237).expect("pc");
    cpu.set_reg(0, base);
    interpret::step(&mut cpu, &mut bus).expect("LDM PC");
    assert_eq!(cpu.reg(1), 0x1234);
    assert_eq!(cpu.pc(), 0x0800_1234);
    assert!(!cpu.thumb(), "ARMv4 LDM PC keeps ARM state");

    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 0, 0, 0, 0x8002));
    cpu.set_reg(0, base);
    cpu.set_reg(1, 0x5678);
    interpret::step(&mut cpu, &mut bus).expect("STM PC");
    assert_eq!(bus.read32(base), Ok(0x5678));
    assert_eq!(bus.read32(base + 4), Ok(EWRAM_BASE + 12));
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);

    // A false conditional LDM PC falls through without any access.
    let (mut cpu, mut bus) = arm(block(0x0, 0, 1, 0, 0, 1, 0, 0x8002));
    cpu.set_reg(0, base);
    cpu.set_nzcv(false, false, false, false); // EQ false
    let digest = bus.state_digest();
    interpret::step(&mut cpu, &mut bus).expect("false LDM PC");
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
    assert_eq!(bus.state_digest(), digest);
}

#[test]
fn s_user_bank_empty_and_pc_base_encodings_are_rejected() {
    for raw in [
        block(0xE, 0, 1, 1, 0, 1, 0, 0b0010),  // S=1
        block(0xE, 0, 1, 0, 0, 1, 0, 0),       // empty list
        block(0xE, 0, 1, 0, 0, 1, 15, 0b0010), // Rn=PC
    ] {
        let (mut cpu, mut bus) = arm(raw);
        cpu.set_reg(0, EWRAM_BASE + 0x100);
        assert!(matches!(
            interpret::step(&mut cpu, &mut bus),
            Err(Error::UnsupportedInstruction { .. })
        ));
        assert_eq!(cpu.pc(), EWRAM_BASE);
    }
}

#[test]
fn unaligned_base_aligns_only_the_transfer_address() {
    let base = EWRAM_BASE + 0x102;

    // STMIA: words go to base&!3 and base&!3+4; writeback uses original+8.
    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 1, 0, 0, 0b0110));
    cpu.set_reg(0, base);
    cpu.set_reg(1, 0x1111);
    cpu.set_reg(2, 0x2222);
    interpret::step(&mut cpu, &mut bus).expect("unaligned STMIA");
    assert_eq!(bus.read32(EWRAM_BASE + 0x100), Ok(0x1111));
    assert_eq!(bus.read32(EWRAM_BASE + 0x104), Ok(0x2222));
    assert_eq!(cpu.reg(0), base + 8);

    // LDMIA: reads the same aligned words; writeback uses original+8.
    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 1, 1, 0, 0b0110));
    bus.write32(EWRAM_BASE + 0x100, 0xAAAA).expect("first");
    bus.write32(EWRAM_BASE + 0x104, 0xBBBB).expect("second");
    cpu.set_reg(0, base);
    interpret::step(&mut cpu, &mut bus).expect("unaligned LDMIA");
    assert_eq!(cpu.reg(1), 0xAAAA);
    assert_eq!(cpu.reg(2), 0xBBBB);
    assert_eq!(cpu.reg(0), base + 8);

    // LDMDA: the aligned base is the mode reference; writeback is original-8.
    let (mut cpu, mut bus) = arm(block(0xE, 0, 0, 0, 1, 1, 0, 0b0110));
    bus.write32(EWRAM_BASE + 0xFC, 0xCCCC).expect("da first");
    bus.write32(EWRAM_BASE + 0x100, 0xDDDD).expect("da second");
    cpu.set_reg(0, base);
    interpret::step(&mut cpu, &mut bus).expect("unaligned LDMDA");
    assert_eq!(cpu.reg(1), 0xCCCC);
    assert_eq!(cpu.reg(2), 0xDDDD);
    assert_eq!(cpu.reg(0), base - 8);
}

#[test]
fn unaligned_base_in_list_specials_keep_the_original_base() {
    let base = EWRAM_BASE + 0x102;

    // STMIA with the base first stores the old (still unaligned) base.
    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 1, 0, 0, 0b0011));
    cpu.set_reg(0, base);
    cpu.set_reg(1, 0x11);
    interpret::step(&mut cpu, &mut bus).expect("unaligned STMIA base first");
    assert_eq!(bus.read32(EWRAM_BASE + 0x100), Ok(base));
    assert_eq!(bus.read32(EWRAM_BASE + 0x104), Ok(0x11));
    assert_eq!(cpu.reg(0), base + 8);

    // LDMIA with the base in the list: the loaded value wins.
    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 1, 1, 0, 0b0011));
    bus.write32(EWRAM_BASE + 0x100, 0xAAAA)
        .expect("loaded base");
    bus.write32(EWRAM_BASE + 0x104, 0xBBBB).expect("second");
    cpu.set_reg(0, base);
    interpret::step(&mut cpu, &mut bus).expect("unaligned LDMIA base loaded");
    assert_eq!(cpu.reg(0), 0xAAAA);
    assert_eq!(cpu.reg(1), 0xBBBB);
}

#[test]
fn partial_multiaccess_failure_is_observable_and_never_promises_atomicity() {
    // Palette/VRAM/OAM have explicit prototype aliases; the first unmapped word
    // immediately after the narrow OAM alias window is a stable failure edge.
    let last_word = OAM_BASE + (OAM_LEN * 2) as u32 - 4;

    // LDM: the first register is written, the second access fails, no WB.
    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 1, 1, 3, 0b0110));
    bus.write32(last_word, 0xCAFE_BABE).expect("last word");
    cpu.set_reg(3, last_word);
    cpu.set_reg(1, 0x1111_2222);
    let error = interpret::step(&mut cpu, &mut bus).expect_err("LDM over the end");
    assert!(matches!(error, Error::Unmapped { .. }));
    assert_eq!(cpu.reg(1), 0xCAFE_BABE, "first load completed");
    assert_eq!(cpu.reg(2), 0, "second load did not complete");
    assert_eq!(cpu.reg(3), last_word, "no writeback after failure");

    // STM: the first store is visible, the second fails, no WB.
    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 1, 0, 3, 0b0110));
    cpu.set_reg(3, last_word);
    cpu.set_reg(1, 0xAAAA_AAAA);
    cpu.set_reg(2, 0xBBBB_BBBB);
    let error = interpret::step(&mut cpu, &mut bus).expect_err("STM over the end");
    assert!(matches!(error, Error::Unmapped { .. }));
    assert_eq!(bus.read32(last_word), Ok(0xAAAA_AAAA));
    assert_eq!(cpu.reg(3), last_word, "no writeback after failure");
}

#[test]
fn rom_store_error_is_terminal_without_writeback() {
    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 1, 0, 0, 0b0010));
    cpu.set_reg(0, 0x0800_0000);
    cpu.set_reg(1, 0xFFFF_FFFF);
    let error = interpret::step(&mut cpu, &mut bus).expect_err("ROM block store");
    assert!(matches!(error, Error::RomWrite { .. }));
    assert_eq!(cpu.reg(0), 0x0800_0000);
    assert_eq!(cpu.pc(), EWRAM_BASE);
}

#[test]
fn block_ir_is_typed_and_flags_are_never_changed() {
    let instruction = decode_arm(0, block(0xE, 1, 0, 0, 1, 0, 13, 0x4000)).expect("STMDB sp!,{lr}");
    assert_eq!(
        instruction.op,
        Op::BlockTransfer {
            load: false,
            rn: 13,
            regs: 0x4000,
            pre_index: true,
            add: false,
            writeback: true,
        }
    );
    assert!(!instruction.is_control_flow());

    let (mut cpu, mut bus) = arm(block(0xE, 0, 1, 0, 0, 1, 0, 0b0010));
    bus.write32(EWRAM_BASE + 0x100, 0x1234_5678).expect("seed");
    cpu.set_reg(0, EWRAM_BASE + 0x100);
    cpu.set_nzcv(true, true, true, true);
    interpret::step(&mut cpu, &mut bus).expect("LDM");
    assert_eq!(cpu.flags(), (true, true, true, true));
}
