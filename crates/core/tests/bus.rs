//! Bus region boundaries, ROM windows, explicit unsupported accesses and the
//! documented byte-store rules and host-injected KEYINPUT state implemented here.

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
fn ram_and_video_region_ends_are_exact() {
    let mut bus = Bus::new(cart(0x200));
    bus.write32(EWRAM_BASE, 0x1122_3344).expect("ewram");
    assert_eq!(bus.read32(EWRAM_BASE + EWRAM_LEN as u32 - 4), Ok(0));
    assert!(matches!(
        bus.read8(EWRAM_BASE + EWRAM_LEN as u32),
        Err(Error::Unmapped { .. })
    ));

    bus.write32(IWRAM_BASE, 0xAABB_CCDD).expect("iwram");
    assert_eq!(bus.read32(IWRAM_BASE + IWRAM_LEN as u32 - 4), Ok(0));
    assert!(matches!(
        bus.read8(IWRAM_BASE + IWRAM_LEN as u32),
        Err(Error::Unmapped { .. })
    ));

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
    assert!(matches!(
        bus.write16(KEYINPUT_ADDR, 0),
        Err(Error::UnsupportedIoWrite { .. })
    ));
    assert_eq!(bus.read16(KEYINPUT_ADDR), Ok(KEYINPUT_RELEASED & !KEY_A));
    assert!(matches!(
        bus.read16(DISPCNT_ADDR + 2),
        Err(Error::UnsupportedIoRead { .. })
    ));
    assert!(matches!(
        bus.read8(DISPCNT_ADDR),
        Err(Error::UnsupportedIoRead { width: 8, .. })
    ));
    assert!(matches!(
        bus.read32(DISPCNT_ADDR),
        Err(Error::UnsupportedIoRead { width: 32, .. })
    ));
    // 0x040003FF is the last I/O byte; it is unsupported I/O, not unmapped.
    assert!(matches!(
        bus.read8(DISPCNT_ADDR + 0x3FF),
        Err(Error::UnsupportedIoRead { width: 8, .. })
    ));
    assert!(matches!(
        bus.write8(DISPCNT_ADDR + 0x3FF, 0),
        Err(Error::UnsupportedIoWrite { width: 8, .. })
    ));
    assert!(matches!(
        bus.read8(DISPCNT_ADDR + 0x400),
        Err(Error::Unmapped { .. })
    ));
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
    assert!(matches!(
        bus.read32(BG0CNT_ADDR),
        Err(Error::UnsupportedIoRead { width: 32, .. })
    ));
    assert!(matches!(
        bus.write8(BG0CNT_ADDR, 0),
        Err(Error::UnsupportedIoWrite { width: 8, .. })
    ));
    assert!(matches!(
        bus.write32(BG0CNT_ADDR, 0),
        Err(Error::UnsupportedIoWrite { width: 32, .. })
    ));
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
