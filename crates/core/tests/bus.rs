//! Bus region boundaries, ROM windows, explicit unsupported accesses and the
//! documented byte-store rules and host-injected KEYINPUT state implemented here.

use gba_native_core::bus::BiosDataPolicy;
use gba_native_core::bus::{
    BG0CNT_ADDR, BG0HOFS_ADDR, BG0VOFS_ADDR, Bus, DISPCNT_ADDR, EWRAM_BASE, EWRAM_LEN, IWRAM_BASE,
    IWRAM_LEN, KEY_A, KEY_B, KEY_DOWN, KEY_L, KEY_LEFT, KEY_MASK, KEY_R, KEY_RIGHT, KEY_SELECT,
    KEY_START, KEY_UP, KEYINPUT_ADDR, KEYINPUT_RELEASED, OAM_BASE, OAM_LEN, PALETTE_BASE,
    PALETTE_LEN, VRAM_BASE, VRAM_LEN,
};
use gba_native_core::cart::{Cartridge, ROM_BASE};
use gba_native_core::error::Error;

fn cart(len: usize) -> Cartridge {
    let mut bytes = vec![0u8; len];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = index as u8;
    }
    bytes[0xB2] = 0x96;
    let sum = bytes[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    bytes[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    Cartridge::from_bytes(bytes).expect("cart")
}

#[test]
fn rom_windows_are_immutable_aliases() {
    let bus = Bus::new(cart(0x200));
    for window in [0x0800_0000u32, 0x0A00_0000, 0x0C00_0000] {
        assert_eq!(bus.read8(window + 0x10).expect("byte"), 0x10);
        assert_eq!(
            bus.read16(window + 0x10).expect("half"),
            u16::from_le_bytes([0x10, 0x11])
        );
        assert_eq!(
            bus.read32(window + 0x10).expect("word"),
            u32::from_le_bytes([0x10, 0x11, 0x12, 0x13])
        );
    }
    assert!(matches!(
        bus.read16(ROM_BASE + 0x200),
        Err(Error::RomOverread { .. })
    ));
    assert!(matches!(
        bus.read32(ROM_BASE + 0x200),
        Err(Error::RomOverread { .. })
    ));
    assert!(matches!(
        bus.read8(0x0A00_0000 + 0x200),
        Err(Error::RomOverread { .. })
    ));
    assert!(matches!(
        bus.read16(ROM_BASE + 0x1FF),
        Err(Error::UnalignedHalfword { .. })
    ));
}

#[test]
fn work_ram_mirrors_and_video_region_ends_are_exact() {
    let mut bus = Bus::new(cart(0x200));
    // Work RAM mirrors its bank across the whole 16 MiB window, so a bank end
    // aliases the bank start instead of being unmapped.
    bus.write32(EWRAM_BASE, 0x1122_3344).expect("ewram");
    assert_eq!(bus.fetch16(EWRAM_BASE + EWRAM_LEN as u32), Ok(0x3344));
    assert_eq!(bus.read32(EWRAM_BASE + EWRAM_LEN as u32 - 4), Ok(0));
    assert_eq!(bus.read8(EWRAM_BASE + EWRAM_LEN as u32), Ok(0x44));

    bus.write32(IWRAM_BASE, 0xAABB_CCDD).expect("iwram");
    assert_eq!(bus.fetch16(IWRAM_BASE + IWRAM_LEN as u32), Ok(0xCCDD));
    assert_eq!(bus.read32(IWRAM_BASE + IWRAM_LEN as u32 - 4), Ok(0));
    assert_eq!(bus.read8(IWRAM_BASE + IWRAM_LEN as u32), Ok(0xDD));

    bus.write16(VRAM_BASE + VRAM_LEN as u32 - 2, 0x1234)
        .expect("vram end");
    assert_eq!(bus.read16(VRAM_BASE + VRAM_LEN as u32 - 2), Ok(0x1234));
    assert!(matches!(
        bus.read8(VRAM_BASE + VRAM_LEN as u32),
        Err(Error::Unmapped { .. })
    ));

    bus.write16(PALETTE_BASE + PALETTE_LEN as u32 - 2, 0x7FFF)
        .expect("palette end");
    assert_eq!(
        bus.read16(PALETTE_BASE + PALETTE_LEN as u32 - 2),
        Ok(0x7FFF)
    );
    assert!(matches!(
        bus.read8(PALETTE_BASE + PALETTE_LEN as u32),
        Err(Error::Unmapped { .. })
    ));

    bus.write16(OAM_BASE + OAM_LEN as u32 - 2, 0xBEEF)
        .expect("oam end");
    assert_eq!(bus.read16(OAM_BASE + OAM_LEN as u32 - 2), Ok(0xBEEF));
    assert!(matches!(
        bus.read8(OAM_BASE + OAM_LEN as u32),
        Err(Error::Unmapped { .. })
    ));
}

#[test]
fn byte_stores_to_16bit_video_memory_duplicate_or_ignore() {
    let mut bus = Bus::new(cart(0x200));
    bus.write8(VRAM_BASE + 3, 0xAB).expect("vram byte");
    assert_eq!(bus.read16(VRAM_BASE + 2), Ok(0xABAB));
    bus.write8(PALETTE_BASE + 1, 0xCD).expect("palette byte");
    assert_eq!(bus.read16(PALETTE_BASE), Ok(0xCDCD));
    bus.write8(OAM_BASE + 1, 0xEF).expect("oam byte");
    assert_eq!(bus.read16(OAM_BASE), Ok(0));
}

#[test]
fn implemented_io_registers_and_keyinput_are_explicit() {
    let mut bus = Bus::new(cart(0x200));
    assert_eq!(bus.read16(KEYINPUT_ADDR), Ok(KEYINPUT_RELEASED));
    bus.write16(DISPCNT_ADDR, 0x0403).expect("dispcnt");
    assert_eq!(bus.dispcnt(), 0x0403);
    assert_eq!(bus.read16(DISPCNT_ADDR), Ok(0x0403));
    bus.set_pressed_keys(KEY_A);
    // KEYINPUT is read-only; a write is accepted and has no effect.
    assert_eq!(bus.write16(KEYINPUT_ADDR, 0), Ok(()));
    assert_eq!(bus.read16(KEYINPUT_ADDR), Ok(KEYINPUT_RELEASED & !KEY_A));
    assert!(matches!(
        bus.read16(DISPCNT_ADDR + 2),
        Err(Error::UnsupportedIoRead { .. })
    ));
    assert!(matches!(
        bus.read8(DISPCNT_ADDR),
        Err(Error::UnsupportedIoRead { width: 8, .. })
    ));
    // 32-bit I/O is two halfword reads; the second half (0x04000002) fails.
    assert!(matches!(
        bus.read32(DISPCNT_ADDR),
        Err(Error::UnsupportedIoRead { width: 16, .. })
    ));
    // 0x040003FF is the last I/O byte; it is unsupported I/O, not unmapped.
    assert!(matches!(
        bus.read8(DISPCNT_ADDR + 0x3FF),
        Err(Error::UnsupportedIoRead { width: 8, .. })
    ));
    // Unmodelled I/O writes are ignored and recorded.
    assert_eq!(bus.write8(DISPCNT_ADDR + 0x3FF, 0), Ok(()));
    assert!(matches!(
        bus.read8(DISPCNT_ADDR + 0x400),
        Err(Error::Unmapped { .. })
    ));
}

#[test]
fn ignored_io_writes_are_recorded_without_mutating_guest_state() {
    let mut bus = Bus::new(cart(0x200));
    let before = bus.state_digest();

    // 0xA8 is an unmodelled halfword; 32-bit writes compose two halfwords.
    bus.write32(0x0400_00A8, 0x1234_5678)
        .expect("unused I/O write");
    // A write in the unused I/O address window is ignored too.
    bus.write16(0x0400_0400, 0xABCD)
        .expect("unused I/O window write");
    bus.write16(0x0400_00A8, 0x9ABC)
        .expect("repeat unused I/O address");

    assert_eq!(
        bus.ignored_io_writes(),
        &[0x0400_00A8, 0x0400_00AA, 0x0400_0400]
    );
    assert_eq!(bus.state_digest(), before);
    assert_eq!(bus.ignored_io_write_count(), 4);
    assert!(!bus.ignored_io_writes_truncated());
    assert!(matches!(
        bus.read16(0x0400_00A8),
        Err(Error::UnsupportedIoRead { .. })
    ));
    assert!(matches!(
        bus.read16(0x0400_0400),
        Err(Error::Unmapped { .. })
    ));
}

#[test]
fn ignored_io_write_log_is_bounded_and_reports_truncation() {
    let mut bus = Bus::new(cart(0x200));
    let before = bus.state_digest();
    for offset in 0..4098u32 {
        bus.write8(0x0400_0400 + offset, offset as u8)
            .expect("unused I/O write");
    }

    assert_eq!(bus.ignored_io_writes().len(), 4096);
    assert_eq!(bus.ignored_io_writes()[0], 0x0400_0400);
    assert_eq!(bus.ignored_io_write_count(), 4098);
    assert!(bus.ignored_io_writes_truncated());
    assert_eq!(bus.state_digest(), before);
}

#[test]
fn host_pressed_keys_map_to_active_low_keyinput_and_ignore_unused_bits() {
    let mut bus = Bus::new(cart(0x200));
    assert_eq!(bus.read16(KEYINPUT_ADDR), Ok(KEYINPUT_RELEASED));

    let mappings = [
        (KEY_A, 1 << 0),
        (KEY_B, 1 << 1),
        (KEY_SELECT, 1 << 2),
        (KEY_START, 1 << 3),
        (KEY_RIGHT, 1 << 4),
        (KEY_LEFT, 1 << 5),
        (KEY_UP, 1 << 6),
        (KEY_DOWN, 1 << 7),
        (KEY_R, 1 << 8),
        (KEY_L, 1 << 9),
    ];
    for (key, keyinput_bit) in mappings {
        bus.set_pressed_keys(key);
        assert_eq!(
            bus.read16(KEYINPUT_ADDR),
            Ok(KEYINPUT_RELEASED & !keyinput_bit),
            "host key mask {key:#06x}"
        );
    }

    bus.set_pressed_keys(KEY_A | KEY_START);
    assert_eq!(
        bus.read16(KEYINPUT_ADDR),
        Ok(KEYINPUT_RELEASED & !(KEY_A | KEY_START))
    );
    bus.set_pressed_keys(KEY_UP | KEY_RIGHT | KEY_B);
    assert_eq!(
        bus.read16(KEYINPUT_ADDR),
        Ok(KEYINPUT_RELEASED & !(KEY_UP | KEY_RIGHT | KEY_B))
    );

    bus.set_pressed_keys(KEY_A | !KEY_MASK);
    assert_eq!(bus.read16(KEYINPUT_ADDR), Ok(KEYINPUT_RELEASED & !KEY_A));
}

#[test]
fn keyinput_state_is_included_in_bus_digest() {
    let mut bus = Bus::new(cart(0x200));
    let released = bus.state_digest();
    bus.set_pressed_keys(KEY_A | KEY_START);
    let pressed = bus.state_digest();
    assert_ne!(released, pressed);

    bus.set_pressed_keys(0);
    assert_eq!(bus.state_digest(), released);
}

#[test]
fn bg0_register_widths_write_only_scroll_and_digest_are_explicit() {
    let mut bus = Bus::new(cart(0x200));
    let initial = bus.state_digest();
    bus.write16(BG0CNT_ADDR, 0x8123).expect("BG0CNT write");
    assert_eq!(bus.read16(BG0CNT_ADDR), Ok(0x8123));
    let with_control = bus.state_digest();
    assert_ne!(initial, with_control);

    // Scroll bits 9..15 are unused and have no state effect.
    bus.write16(BG0HOFS_ADDR, 0x0200)
        .expect("ignored scroll bits");
    assert_eq!(bus.state_digest(), with_control);
    bus.write16(BG0HOFS_ADDR, 0x0105)
        .expect("horizontal scroll");
    let with_horizontal = bus.state_digest();
    assert_ne!(with_control, with_horizontal);
    bus.write16(BG0VOFS_ADDR, 0x01FE).expect("vertical scroll");
    assert_ne!(with_horizontal, bus.state_digest());

    for addr in [BG0HOFS_ADDR, BG0VOFS_ADDR] {
        assert!(matches!(
            bus.read16(addr),
            Err(Error::UnsupportedIoRead { addr: actual, width: 16 }) if actual == addr
        ));
    }
    assert!(matches!(
        bus.read8(BG0CNT_ADDR),
        Err(Error::UnsupportedIoRead { width: 8, .. })
    ));
    // A 32-bit I/O read is two halfword reads, so the second half fails first.
    assert!(matches!(
        bus.read32(BG0CNT_ADDR),
        Err(Error::UnsupportedIoRead { width: 16, .. })
    ));
    assert_eq!(bus.write8(BG0CNT_ADDR, 0), Ok(()));
    assert_eq!(bus.write32(BG0CNT_ADDR, 0), Ok(()));
}

#[test]
fn unsupported_accesses_are_terminal_errors() {
    let mut bus = Bus::new(cart(0x200));
    assert!(matches!(
        bus.read8(0x0000_0010),
        Err(Error::BiosAccess { .. })
    ));
    assert!(matches!(
        bus.write16(0x0000_0010, 0),
        Err(Error::BiosAccess { .. })
    ));
    assert!(matches!(
        bus.write32(ROM_BASE, 0),
        Err(Error::RomWrite { .. })
    ));
    assert!(matches!(
        bus.read8(0x0E00_0000),
        Err(Error::UnsupportedSave { .. })
    ));
    assert!(matches!(
        bus.read16(0x0600_0001),
        Err(Error::UnalignedHalfword { .. })
    ));
    assert!(matches!(
        bus.read32(0x0200_0002),
        Err(Error::UnalignedWord { .. })
    ));
    assert!(matches!(
        bus.fetch32(0x0400_0000),
        Err(Error::NonExecutable { .. })
    ));
    assert!(matches!(
        bus.fetch16(0x0200_0001),
        Err(Error::UnalignedFetch { .. })
    ));
}

#[test]
fn ram_code_is_fetchable_but_video_memory_is_not() {
    let mut bus = Bus::new(cart(0x200));
    bus.write32(EWRAM_BASE, 0xE3A0_0001).expect("ewram code");
    assert_eq!(bus.fetch32(EWRAM_BASE), Ok(0xE3A0_0001));
    bus.write16(VRAM_BASE, 0x4770).expect("vram");
    assert!(matches!(
        bus.fetch16(VRAM_BASE),
        Err(Error::NonExecutable { .. })
    ));
}

#[test]
fn strict_bios_data_policy_is_the_default_for_all_widths() {
    let bus = Bus::new(cart(0x200));
    assert_eq!(bus.bios_data_policy(), BiosDataPolicy::Strict);
    for addr in [0x0000_0000, 0x0000_000A, 0x0000_3FFF] {
        assert!(matches!(bus.read8(addr), Err(Error::BiosAccess { .. })));
    }
    for addr in [0x0000_0000, 0x0000_000A, 0x0000_3FFE] {
        assert!(matches!(bus.read16(addr), Err(Error::BiosAccess { .. })));
    }
    for addr in [0x0000_0000, 0x0000_0008, 0x0000_3FFC] {
        assert!(matches!(bus.read32(addr), Err(Error::BiosAccess { .. })));
    }
}

#[test]
fn synthetic_bios_zero_fill_returns_zero_for_every_read_width() {
    let bus = Bus::with_synthetic_bios_zero_fill(cart(0x200));
    assert_eq!(bus.bios_data_policy(), BiosDataPolicy::SyntheticZeroFill);
    for addr in [0x0000_0000, 0x0000_000A, 0x0000_3FFF] {
        assert_eq!(bus.read8(addr), Ok(0), "byte at {addr:#010X}");
    }
    for addr in [0x0000_0000, 0x0000_000A, 0x0000_3FFE] {
        assert_eq!(bus.read16(addr), Ok(0), "halfword at {addr:#010X}");
    }
    for addr in [0x0000_0000, 0x0000_0008, 0x0000_3FFC] {
        assert_eq!(bus.read32(addr), Ok(0), "word at {addr:#010X}");
    }
    // Alignment validation still runs before the zero-fill policy.
    assert!(matches!(
        bus.read16(0x0000_0001),
        Err(Error::UnalignedHalfword { .. })
    ));
    assert!(matches!(
        bus.read32(0x0000_0002),
        Err(Error::UnalignedWord { .. })
    ));
    // The policy ends at the 16 KiB BIOS window; the next byte is unmapped.
    assert!(matches!(
        bus.read8(0x0000_4000),
        Err(Error::Unmapped { .. })
    ));
}

#[test]
fn synthetic_bios_zero_fill_never_makes_bios_executable_or_writable() {
    let bus = Bus::with_synthetic_bios_zero_fill(cart(0x200));
    for addr in [0x0000_0000, 0x0000_000A] {
        assert!(matches!(
            bus.fetch16(addr),
            Err(Error::NonExecutable { .. })
        ));
    }
    for addr in [0x0000_0000, 0x0000_0008] {
        assert!(matches!(
            bus.fetch32(addr),
            Err(Error::NonExecutable { .. })
        ));
    }

    let mut bus = Bus::with_synthetic_bios_zero_fill(cart(0x200));
    assert!(matches!(
        bus.write8(0x0000_000A, 0x11),
        Err(Error::BiosAccess { .. })
    ));
    assert!(matches!(
        bus.write16(0x0000_000A, 0x1111),
        Err(Error::BiosAccess { .. })
    ));
    assert!(matches!(
        bus.write32(0x0000_0008, 0x1111_1111),
        Err(Error::BiosAccess { .. })
    ));
}

#[test]
fn bios_data_policy_is_host_configuration_outside_the_state_digest() {
    let mut strict = Bus::new(cart(0x200));
    let mut zero_fill = Bus::with_synthetic_bios_zero_fill(cart(0x200));
    assert_eq!(strict.state_digest(), zero_fill.state_digest());
    strict.write32(EWRAM_BASE, 0x1234_5678).expect("ewram");
    zero_fill.write32(EWRAM_BASE, 0x1234_5678).expect("ewram");
    assert_eq!(strict.state_digest(), zero_fill.state_digest());
    assert_ne!(strict.bios_data_policy(), zero_fill.bios_data_policy());
}
