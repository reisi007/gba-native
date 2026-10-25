//! Independent golden tests for the BIOS HLE memory services.
//!
//! Fixtures are entirely synthetic: RAM patterns and instruction-free
//! addresses. No ROM, BIOS image or external file is involved.

use gba_native::bios::{self, BiosError};
use gba_native::bus::{Bus, EWRAM_BASE, EWRAM_LEN, IWRAM_BASE, VRAM_BASE};
use gba_native::cart::{Cartridge, ROM_BASE};
use gba_native::cpu::Cpu;
use gba_native::error::Error;

const FILL: u32 = 1 << 24;
const WORD: u32 = 1 << 26;

fn fresh() -> (Cpu, Bus) {
    let cart = Cartridge::from_bytes(vec![0u8; 0x400]).expect("test cart");
    (Cpu::direct_entry(), Bus::new(cart))
}

fn set_args(cpu: &mut Cpu, source: u32, destination: u32, control: u32) {
    cpu.set_reg(0, source);
    cpu.set_reg(1, destination);
    cpu.set_reg(2, control);
}

fn assert_value16(bus: &Bus, base: u32, len: usize, value: u16) {
    for offset in (0..len).step_by(2) {
        assert_eq!(
            bus.read16(base + offset as u32).expect("read"),
            value,
            "0x{:08X}",
            base + offset as u32
        );
    }
}

#[test]
fn cpu_set_halfword_copy_and_fill() {
    let (mut cpu, mut bus) = fresh();
    let source = EWRAM_BASE + 0x100;
    let destination = EWRAM_BASE + 0x200;
    for unit in 0..4u32 {
        bus.write16(source + 2 * unit, 0x1000 + unit as u16)
            .expect("seed");
    }
    cpu.set_reg(3, 0xDEAD_BEEF);
    let cpsr = cpu.cpsr();
    let pc = cpu.pc();
    set_args(&mut cpu, source, destination, 4);

    bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus).expect("copy");
    for unit in 0..4u32 {
        assert_eq!(
            bus.read16(destination + 2 * unit).expect("read"),
            0x1000 + unit as u16
        );
        assert_eq!(
            bus.read16(source + 2 * unit).expect("source"),
            0x1000 + unit as u16
        );
    }
    assert_eq!(cpu.reg(3), 0xDEAD_BEEF);
    assert_eq!(cpu.cpsr(), cpsr);
    assert_eq!(cpu.pc(), pc);

    let fill_source = EWRAM_BASE + 0x300;
    bus.write16(fill_source, 0xBEEF).expect("fill value");
    let fill_destination = EWRAM_BASE + 0x400;
    set_args(&mut cpu, fill_source, fill_destination, 3 | FILL);
    bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus).expect("fill");
    assert_value16(&bus, fill_destination, 6, 0xBEEF);
    assert_eq!(bus.read16(fill_source).expect("source"), 0xBEEF);
}

#[test]
fn cpu_set_word_copy_and_fill() {
    let (mut cpu, mut bus) = fresh();
    let source = EWRAM_BASE + 0x200;
    let destination = EWRAM_BASE + 0x300;
    for unit in 0..3u32 {
        bus.write32(source + 4 * unit, 0xCAFE_0000 + unit)
            .expect("seed");
    }
    set_args(&mut cpu, source, destination, 3 | WORD);
    bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus).expect("word copy");
    for unit in 0..3u32 {
        assert_eq!(
            bus.read32(destination + 4 * unit).expect("read"),
            0xCAFE_0000 + unit
        );
    }

    let fill_source = EWRAM_BASE + 0x400;
    bus.write32(fill_source, 0xCAFE_BABE).expect("seed");
    let fill_destination = EWRAM_BASE + 0x500;
    set_args(&mut cpu, fill_source, fill_destination, 3 | WORD | FILL);
    bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus).expect("word fill");
    for unit in 0..3u32 {
        assert_eq!(
            bus.read32(fill_destination + 4 * unit).expect("read"),
            0xCAFE_BABE
        );
    }
}

#[test]
fn cpu_set_fill_reads_only_the_first_source_unit() {
    let (mut cpu, mut bus) = fresh();
    // The second halfword would be outside EWRAM if the source advanced.
    let source = EWRAM_BASE + EWRAM_LEN as u32 - 2;
    let destination = EWRAM_BASE + 0x800;
    bus.write16(source, 0x1234).expect("seed");
    set_args(&mut cpu, source, destination, 4 | FILL);
    bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus).expect("fill");
    assert_value16(&bus, destination, 8, 0x1234);
}

#[test]
fn cpu_set_count_zero_is_no_access_and_control_is_still_validated() {
    let (mut cpu, mut bus) = fresh();
    let destination = EWRAM_BASE + 0x900;
    bus.write16(destination, 0xAAAA).expect("seed");
    let before = bus.state_digest();
    set_args(&mut cpu, EWRAM_BASE, destination, 0);
    bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus).expect("zero count");
    assert_eq!(bus.state_digest(), before);

    // Reserved bits are rejected even when the count is zero.
    set_args(&mut cpu, EWRAM_BASE, destination, 1 << 25);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::ReservedControl {
            service: 0x0B,
            ..
        }))
    ));
    assert_eq!(bus.state_digest(), before);

    // Alignment is part of the request even with count zero.
    set_args(&mut cpu, EWRAM_BASE + 1, destination, 0);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::Misaligned {
            service: 0x0B,
            align: 2,
            ..
        }))
    ));
}

#[test]
fn cpu_set_forward_overlap_smears_instead_of_snapshotting() {
    let (mut cpu, mut bus) = fresh();
    let source = EWRAM_BASE + 0x1000;
    for unit in 0..5u16 {
        bus.write16(source + 2 * u32::from(unit), 1 + unit)
            .expect("seed");
    }
    set_args(&mut cpu, source, source + 2, 4);
    bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus).expect("overlap");
    for unit in 0..5u32 {
        assert_eq!(bus.read16(source + 2 * unit).expect("read"), 1);
    }
}

#[test]
fn cpu_set_rejects_alignment_reserved_bios_and_ranges_before_mutation() {
    let (mut cpu, mut bus) = fresh();
    let destination = EWRAM_BASE + 0xA00;
    bus.write16(destination, 0xAAAA).expect("seed");
    let before = bus.state_digest();

    set_args(&mut cpu, EWRAM_BASE + 1, destination, 1);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::Misaligned { align: 2, .. }))
    ));

    set_args(&mut cpu, EWRAM_BASE, destination + 1, 1);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::Misaligned { align: 2, .. }))
    ));

    set_args(&mut cpu, EWRAM_BASE, destination, 1 << 27);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::ReservedControl { .. }))
    ));

    set_args(&mut cpu, 0x100, destination, 1);
    assert_eq!(
        bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus),
        Err(Error::BiosAccess { addr: 0x100 })
    );

    // A range that starts below and ends inside the 16 KiB BIOS window.
    set_args(&mut cpu, 0x3FFE, destination, 2);
    assert_eq!(
        bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus),
        Err(Error::BiosAccess { addr: 0x3FFE })
    );

    // Crosses the EWRAM boundary.
    set_args(&mut cpu, EWRAM_BASE + EWRAM_LEN as u32 - 2, destination, 2);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::RangeOutsideRegion { .. }))
    ));

    // Checked expansion overflows the address space.
    set_args(&mut cpu, 0xFFFF_FFFE, destination, 2);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::RangeOverflow { .. }))
    ));

    // ROM, I/O and unmapped destinations are rejected whole.
    for bad_destination in [ROM_BASE, 0x0400_0000, EWRAM_BASE + EWRAM_LEN as u32] {
        set_args(&mut cpu, EWRAM_BASE, bad_destination, 1);
        assert!(matches!(
            bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus),
            Err(Error::Bios(BiosError::UnsupportedDestination { .. }))
        ));
    }

    // I/O is not a read source either.
    set_args(&mut cpu, 0x0400_0000, destination, 1);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::UnsupportedSource { .. }))
    ));

    assert_eq!(bus.read16(destination).expect("read"), 0xAAAA);
    assert_eq!(bus.state_digest(), before);
}

#[test]
fn cpu_set_reads_rom_but_rejects_rom_overread_before_mutation() {
    let mut cpu = Cpu::direct_entry();
    let mut bytes = vec![0u8; 0x400];
    bytes[0] = 0x34;
    bytes[1] = 0x12;
    let mut bus = Bus::new(Cartridge::from_bytes(bytes).expect("cart"));
    let destination = EWRAM_BASE + 0xB00;
    set_args(&mut cpu, ROM_BASE, destination, 1);
    bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus).expect("rom copy");
    assert_eq!(bus.read16(destination).expect("read"), 0x1234);

    bus.write16(destination, 0xAAAA).expect("seed");
    let before = bus.state_digest();
    set_args(&mut cpu, ROM_BASE + 0x3FE, destination, 2);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::RangeOutsideRegion { .. }))
    ));
    assert_eq!(bus.state_digest(), before);
}

#[test]
fn cpu_fast_set_rounds_count_up_to_eight_word_blocks() {
    let (mut cpu, mut bus) = fresh();
    let source = EWRAM_BASE + 0x800;
    bus.write32(source, 0xA5A5_5A5A).expect("seed");
    for count in 0..=17u32 {
        let destination = EWRAM_BASE + 0x1000 + count * 0x100;
        for unit in 0..=32u32 {
            bus.write32(destination + 4 * unit, 0xDEAD_BEEF)
                .expect("seed");
        }
        set_args(&mut cpu, source, destination, count | FILL);
        bios::call(bios::SERVICE_CPU_FAST_SET, &mut cpu, &mut bus)
            .unwrap_or_else(|error| panic!("count {count}: {error}"));
        let expected = if count == 0 { 0 } else { count.div_ceil(8) * 8 };
        for unit in 0..expected {
            assert_eq!(
                bus.read32(destination + 4 * unit).expect("read"),
                0xA5A5_5A5A,
                "count {count} unit {unit}"
            );
        }
        assert_eq!(
            bus.read32(destination + 4 * expected).expect("sentinel"),
            0xDEAD_BEEF,
            "count {count} wrote past the rounded end"
        );
    }
}

#[test]
fn cpu_fast_set_copy_and_forward_overlap() {
    let (mut cpu, mut bus) = fresh();
    let source = EWRAM_BASE + 0x2000;
    for unit in 0..4u32 {
        bus.write32(source + 4 * unit, 11 * (unit + 1))
            .expect("seed");
    }
    let destination = EWRAM_BASE + 0x3000;
    set_args(&mut cpu, source, destination, 4);
    bios::call(bios::SERVICE_CPU_FAST_SET, &mut cpu, &mut bus).expect("copy");
    for unit in 0..8u32 {
        let expected = if unit < 4 { 11 * (unit + 1) } else { 0 };
        assert_eq!(bus.read32(destination + 4 * unit).expect("read"), expected);
    }

    set_args(&mut cpu, source, source + 4, 4);
    bios::call(bios::SERVICE_CPU_FAST_SET, &mut cpu, &mut bus).expect("overlap");
    for unit in 0..8u32 {
        assert_eq!(bus.read32(source + 4 * unit).expect("read"), 11);
    }
}

#[test]
fn cpu_fast_set_fill_reads_only_the_first_word() {
    let (mut cpu, mut bus) = fresh();
    let source = EWRAM_BASE + EWRAM_LEN as u32 - 4;
    let destination = EWRAM_BASE + 0x4000;
    bus.write32(source, 0x1357_9BDF).expect("seed");
    set_args(&mut cpu, source, destination, 1 | FILL);
    bios::call(bios::SERVICE_CPU_FAST_SET, &mut cpu, &mut bus).expect("fill");
    for unit in 0..8u32 {
        assert_eq!(
            bus.read32(destination + 4 * unit).expect("read"),
            0x1357_9BDF
        );
    }
}

#[test]
fn cpu_fast_set_rejects_reserved_bits_alignment_and_rounded_overflow() {
    let (mut cpu, mut bus) = fresh();
    let destination = EWRAM_BASE + 0x5000;
    bus.write32(destination, 0xAAAA_AAAA).expect("seed");
    let before = bus.state_digest();

    set_args(&mut cpu, EWRAM_BASE, destination, 1 | WORD);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_FAST_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::ReservedControl {
            service: 0x0C,
            ..
        }))
    ));

    set_args(&mut cpu, EWRAM_BASE, destination, 1 << 21);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_FAST_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::ReservedControl { .. }))
    ));

    set_args(&mut cpu, EWRAM_BASE + 1, destination, 1);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_FAST_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::Misaligned { align: 4, .. }))
    ));

    // One requested word rounds up to eight and crosses the EWRAM end.
    set_args(&mut cpu, EWRAM_BASE, EWRAM_BASE + EWRAM_LEN as u32 - 4, 1);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_FAST_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::RangeOutsideRegion { .. }))
    ));

    // Eight rounded words at the top of the address space cannot be expanded.
    set_args(&mut cpu, EWRAM_BASE, 0xFFFF_FFF8, 1);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_FAST_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::RangeOverflow { .. }))
    ));

    set_args(&mut cpu, 0x10, destination, 1);
    assert_eq!(
        bios::call(bios::SERVICE_CPU_FAST_SET, &mut cpu, &mut bus),
        Err(Error::BiosAccess { addr: 0x10 })
    );

    set_args(&mut cpu, EWRAM_BASE, ROM_BASE, 1);
    assert!(matches!(
        bios::call(bios::SERVICE_CPU_FAST_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::UnsupportedDestination { .. }))
    ));

    assert_eq!(bus.read32(destination).expect("read"), 0xAAAA_AAAA);
    assert_eq!(bus.state_digest(), before);
}

#[test]
fn cpu_set_preserves_32_bit_patterns_exactly() {
    let (mut cpu, mut bus) = fresh();
    let source = EWRAM_BASE + 0xC00;
    let destination = EWRAM_BASE + 0xD00;
    let patterns = [
        0x0000_0000,
        0x8000_0000,
        0xFFFF_FFFF,
        0x7FFF_FFFF,
        0xDEAD_BEEF,
    ];
    for (unit, value) in patterns.iter().enumerate() {
        bus.write32(source + 4 * unit as u32, *value).expect("seed");
    }
    set_args(&mut cpu, source, destination, 5 | WORD);
    bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus).expect("pattern copy");
    for (unit, value) in patterns.iter().enumerate() {
        assert_eq!(
            bus.read32(destination + 4 * unit as u32).expect("read"),
            *value
        );
    }

    let fill_destination = EWRAM_BASE + 0xE00;
    set_args(&mut cpu, source + 8, fill_destination, 5 | WORD | FILL);
    bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus).expect("pattern fill");
    for unit in 0..5u32 {
        assert_eq!(
            bus.read32(fill_destination + 4 * unit).expect("read"),
            0xFFFF_FFFF
        );
    }
}

#[test]
fn cpu_set_copies_between_documented_regions() {
    let (mut cpu, mut bus) = fresh();
    let source = EWRAM_BASE + 0xE00;
    bus.write16(source, 0x1234).expect("seed");
    set_args(&mut cpu, source, IWRAM_BASE + 0xE00, 1);
    bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus).expect("to iwram");
    assert_eq!(bus.read16(IWRAM_BASE + 0xE00).expect("read"), 0x1234);

    set_args(&mut cpu, source, VRAM_BASE + 0xE00, 1);
    bios::call(bios::SERVICE_CPU_SET, &mut cpu, &mut bus).expect("to vram");
    assert_eq!(bus.read16(VRAM_BASE + 0xE00).expect("read"), 0x1234);
}
