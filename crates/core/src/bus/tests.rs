use super::*;

fn bus() -> Bus {
    let cart = Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart");
    Bus::new(cart)
}

#[test]
fn ram_is_little_endian() {
    let mut bus = bus();
    bus.write32(EWRAM_BASE, 0x1234_5678).expect("write");
    assert_eq!(bus.read8(EWRAM_BASE).expect("byte"), 0x78);
    assert_eq!(bus.read16(EWRAM_BASE).expect("half"), 0x5678);
    assert_eq!(bus.read32(EWRAM_BASE).expect("word"), 0x1234_5678);
}

#[test]
fn byte_writes_to_video_memory_duplicate() {
    let mut bus = bus();
    bus.write8(VRAM_BASE + 3, 0xAB).expect("vram byte");
    assert_eq!(bus.read16(VRAM_BASE + 2).expect("vram half"), 0xABAB);
    bus.write8(PALETTE_BASE + 1, 0xCD).expect("palette byte");
    assert_eq!(bus.read16(PALETTE_BASE).expect("palette half"), 0xCDCD);
    bus.write8(OAM_BASE + 1, 0xEF).expect("oam byte");
    assert_eq!(bus.read16(OAM_BASE).expect("oam half"), 0);
}

#[test]
fn rejects_unsupported_and_out_of_bounds() {
    let mut bus = bus();
    assert!(matches!(
        bus.read8(0x0000_0010),
        Err(Error::BiosAccess { .. })
    ));
    assert!(matches!(
        bus.read16(0x0400_0004),
        Err(Error::UnsupportedIoRead { .. })
    ));
    assert!(matches!(
        bus.write16(0x0400_0004, 0),
        Err(Error::UnsupportedIoWrite { .. })
    ));
    assert!(matches!(
        bus.write8(0x0800_0000, 0),
        Err(Error::RomWrite { .. })
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
        bus.read8(0x0204_0000),
        Err(Error::Unmapped { .. })
    ));
    assert!(matches!(
        bus.read8(0x0E00_0000),
        Err(Error::UnsupportedSave { .. })
    ));
}
