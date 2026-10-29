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
    // An unused I/O address stays unimplemented; BG1-BG3CNT and DISPSTAT/VCOUNT
    // are now modelled.
    assert!(matches!(
        bus.read16(0x0400_0056),
        Err(Error::UnsupportedIoRead { .. })
    ));
    // Unmodelled I/O writes are ignored and recorded, not rejected.
    assert_eq!(bus.write16(0x0400_0056, 0), Ok(()));
    assert_eq!(bus.ignored_io_writes(), &[0x0400_0056]);
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
    // Work RAM mirrors across its 16 MiB window instead of being unmapped.
    assert_eq!(bus.read8(0x0204_0000), Ok(0));
    assert_eq!(bus.read8(0x02FF_FFFF), Ok(0));
    assert_eq!(bus.read8(0x0300_8000), Ok(0));
    assert!(matches!(
        bus.read8(0x0E00_0000),
        Err(Error::UnsupportedSave { .. })
    ));
}

#[test]
fn work_ram_range_validation_accepts_mirrors_and_bank_wraps() {
    assert!(Bus::is_work_ram_range(0x0204_0000, 16));
    assert!(Bus::is_work_ram_range(0x0300_8000, 16));
    assert!(Bus::is_work_ram_range(
        IWRAM_BASE + IWRAM_LEN as u32 - 8,
        16
    ));
    assert!(!Bus::is_work_ram_range(0x03FF_FFF8, 16));
    assert!(!Bus::is_work_ram_range(EWRAM_BASE, 0));
}
