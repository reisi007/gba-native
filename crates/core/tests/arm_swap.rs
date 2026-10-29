//! Independent ARMv4T SWP/SWPB encoding and memory-semantic vectors.

use gba_native_core::bus::{Bus, EWRAM_BASE};
use gba_native_core::cart::Cartridge;
use gba_native_core::cpu::Cpu;
use gba_native_core::decode::decode_arm;
use gba_native_core::error::Error;
use gba_native_core::interpret;
use gba_native_core::ir::Op;

fn cart() -> Cartridge {
    Cartridge::from_bytes(vec![0; 0x200]).expect("synthetic cartridge")
}

fn swap(byte: bool, rn: u32, rd: u32, rm: u32) -> u32 {
    0xE100_0090 | (u32::from(byte) << 22) | (rn << 16) | (rd << 12) | rm
}

fn arm(word: u32) -> (Cpu, Bus) {
    let mut bus = Bus::new(cart());
    bus.write32(EWRAM_BASE, word).expect("write instruction");
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(EWRAM_BASE);
    cpu.set_thumb(false);
    (cpu, bus)
}

#[test]
fn decoder_accepts_documented_forms_and_rejects_unpredictable_aliases() {
    assert_eq!(
        decode_arm(0x100, swap(false, 1, 2, 3)).expect("SWP").op,
        Op::Swap {
            rd: 2,
            rm: 3,
            rn: 1,
            byte: false,
        }
    );
    assert_eq!(
        decode_arm(0x100, swap(true, 1, 2, 3)).expect("SWPB").op,
        Op::Swap {
            rd: 2,
            rm: 3,
            rn: 1,
            byte: true,
        }
    );
    decode_arm(0, swap(false, 1, 2, 2)).expect("documented Rd=Rm alias");

    for invalid in [
        swap(false, 15, 1, 2),
        swap(false, 1, 15, 2),
        swap(false, 1, 2, 15),
        swap(false, 1, 1, 2),
        swap(false, 1, 2, 1),
        swap(false, 1, 2, 3) | (1 << 8),
    ] {
        assert!(decode_arm(0, invalid).is_err(), "{invalid:08X}");
    }
}

#[test]
fn word_swap_rotates_unaligned_load_but_stores_unrotated_word() {
    let (mut cpu, mut bus) = arm(swap(false, 1, 3, 2));
    let address = EWRAM_BASE + 0x41;
    bus.write32(address & !3, 0x1122_3344).expect("seed memory");
    cpu.set_reg(1, address);
    cpu.set_reg(2, 0xAABB_CCDD);
    cpu.set_reg(3, 0xDEAD_BEEF);
    let flags = cpu.flags();

    interpret::step(&mut cpu, &mut bus).expect("unaligned SWP");

    assert_eq!(cpu.reg(3), 0x4411_2233, "loaded word is rotated right by 8");
    assert_eq!(
        bus.read32(address & !3),
        Ok(0xAABB_CCDD),
        "store is unrotated"
    );
    assert_eq!(cpu.pc(), EWRAM_BASE + 4);
    assert_eq!(cpu.flags(), flags, "SWP does not alter flags");
}

#[test]
fn byte_swap_zero_extends_load_and_stores_only_the_low_byte() {
    let (mut cpu, mut bus) = arm(swap(true, 1, 3, 2));
    let address = EWRAM_BASE + 0x41;
    bus.write32(address & !3, 0x1122_3344).expect("seed memory");
    cpu.set_reg(1, address);
    cpu.set_reg(2, 0xAABB_CCDD);

    interpret::step(&mut cpu, &mut bus).expect("SWPB");

    assert_eq!(cpu.reg(3), 0x33, "loaded byte is zero-extended");
    assert_eq!(bus.read32(address & !3), Ok(0x1122_DD44));
}

#[test]
fn documented_same_destination_and_source_register_uses_old_source_value() {
    let (mut cpu, mut bus) = arm(swap(false, 1, 2, 2));
    let address = EWRAM_BASE + 0x40;
    bus.write32(address, 0x89AB_CDEF).expect("seed memory");
    cpu.set_reg(1, address);
    cpu.set_reg(2, 0x0123_4567);

    interpret::step(&mut cpu, &mut bus).expect("SWP Rd=Rm");

    assert_eq!(cpu.reg(2), 0x89AB_CDEF);
    assert_eq!(bus.read32(address), Ok(0x0123_4567));
}

#[test]
fn failed_memory_access_does_not_write_destination_or_advance_pc() {
    let (mut cpu, mut bus) = arm(swap(false, 1, 3, 2));
    cpu.set_reg(1, 0x0800_0000);
    cpu.set_reg(2, 0xAABB_CCDD);
    cpu.set_reg(3, 0xDEAD_BEEF);

    assert!(matches!(
        interpret::step(&mut cpu, &mut bus),
        Err(Error::RomWrite { addr: 0x0800_0000 })
    ));
    assert_eq!(cpu.reg(3), 0xDEAD_BEEF);
    assert_eq!(cpu.pc(), EWRAM_BASE);
}
