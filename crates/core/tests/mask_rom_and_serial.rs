//! Documented mask-ROM write semantics and the modelled serial I/O registers.
//!
//! The BIOS is mask ROM and the Game Pak ROM window has no flash command engine
//! here, so a store has no effect. The default stays fail-closed and the
//! no-effect behaviour needs an explicit host opt-in. The serial registers are
//! plain documented storage; a transfer with no connected partner finishes with
//! the documented disconnected state and UART remains an explicit error.

use gba_native_core::bus::{
    BackupPolicy, BiosWritePolicy, Bus, RCNT_ADDR, RomWritePolicy, SIOCNT_ADDR, SIODATA8_ADDR,
    SIODATA32_H_ADDR, SIODATA32_L_ADDR,
};
use gba_native_core::cart::{Cartridge, ROM_BASE};
use gba_native_core::error::Error;

fn cart(len: usize) -> Cartridge {
    Cartridge::from_bytes(vec![0u8; len]).expect("synthetic cart")
}

#[test]
fn bios_write_policy_is_explicit_and_never_stores_bios_bytes() {
    let mut strict = Bus::new(cart(0x200));
    assert_eq!(strict.bios_write_policy(), BiosWritePolicy::Strict);
    assert!(matches!(
        strict.write16(0x0000_0000, 0x1234),
        Err(Error::BiosAccess { .. })
    ));

    let mut lenient =
        Bus::new(cart(0x200)).with_bios_write_policy(BiosWritePolicy::RomWritesDropped);
    assert_eq!(
        lenient.bios_write_policy(),
        BiosWritePolicy::RomWritesDropped
    );
    // Documented mask-ROM behaviour: the store completes with no effect.
    lenient
        .write8(0x0000_0000, 0xFF)
        .expect("dropped byte store");
    lenient
        .write16(0x0000_000A, 0xFFFF)
        .expect("dropped halfword");
    lenient
        .write32(0x0000_0000, 0xFFFF_FFFF)
        .expect("dropped word");
    // No BIOS byte is readable, stored or executable under either policy.
    assert!(matches!(
        lenient.read16(0x0000_000A),
        Err(Error::BiosAccess { .. })
    ));
    assert!(matches!(
        lenient.fetch16(0x0000_0000),
        Err(Error::NonExecutable { .. })
    ));
    // The policy is host configuration, not guest state: a strict bus with the
    // same guest writes has the same digest.
    let mut strict_twin = Bus::new(cart(0x200));
    strict_twin.write8(0x0000_0000, 0xFF).expect_err("strict");
    assert_eq!(lenient.state_digest(), strict_twin.state_digest());
    // Only the BIOS range is affected; the ROM window keeps its own error.
    assert!(matches!(
        lenient.write16(ROM_BASE, 0x1234),
        Err(Error::RomWrite { .. })
    ));
}

#[test]
fn serial_registers_keep_documented_storage_and_report_no_partner() {
    let mut bus = Bus::new(cart(0x200));
    bus.write16(RCNT_ADDR, 0x0100).expect("rcnt");
    assert_eq!(bus.read16(RCNT_ADDR), Ok(0x0100));
    bus.write16(SIOCNT_ADDR, 0x5003).expect("siocnt");
    // Bit 2 is read-only and reports the disconnected port (SI high).
    assert_eq!(bus.read16(SIOCNT_ADDR), Ok(0x5007));
    // Byte lanes follow the documented 8-bit accessibility of the I/O block.
    assert_eq!(bus.read8(SIOCNT_ADDR), Ok(0x07));
    assert_eq!(bus.read8(SIOCNT_ADDR + 1), Ok(0x50));
}

#[test]
fn serial_transfer_without_a_partner_completes_with_received_ones() {
    let mut bus = Bus::new(cart(0x200));
    // Normal 8-bit mode with internal clock and start bit.
    bus.write16(SIOCNT_ADDR, 0x0083).expect("start 8-bit");
    assert_eq!(bus.read16(SIOCNT_ADDR), Ok(0x0007), "start bit clears");
    assert_eq!(bus.read16(SIODATA8_ADDR), Ok(0x00FF));
    // Normal 32-bit mode receives all ones in both halves.
    bus.write16(SIOCNT_ADDR, 0x1083).expect("start 32-bit");
    assert_eq!(bus.read16(SIOCNT_ADDR), Ok(0x1007));
    assert_eq!(bus.read16(SIODATA32_L_ADDR), Ok(0xFFFF));
    assert_eq!(bus.read16(SIODATA32_H_ADDR), Ok(0xFFFF));
    // UART and multiplay starts stay explicit errors instead of fabricated
    // completions.
    for control in [0x3080, 0x2080] {
        assert!(
            matches!(
                bus.write16(SIOCNT_ADDR, control),
                Err(Error::UnsupportedIoWrite { .. })
            ),
            "control {control:#06X}"
        );
    }
    // In general-purpose/JOY-BUS mode SIOCNT is documented as unused, so the
    // write is plain storage and never fabricates a transfer.
    bus.write16(RCNT_ADDR, 0x8000)
        .expect("general purpose mode");
    bus.write16(SIOCNT_ADDR, 0x0083).expect("unused in GP mode");
    assert_eq!(bus.read16(SIOCNT_ADDR), Ok(0x0087));
    // A start with the external shift clock never completes: no partner
    // supplies SC edges, so the start bit stays set and the data is unchanged.
    bus.write16(RCNT_ADDR, 0).expect("normal mode");
    bus.write16(SIODATA8_ADDR, 0x005A).expect("outgoing data");
    bus.write16(SIOCNT_ADDR, 0x0082)
        .expect("externally clocked start is accepted as pending");
    assert_eq!(bus.read16(SIOCNT_ADDR), Ok(0x0086), "start stays set");
    assert_eq!(bus.read16(SIODATA8_ADDR), Ok(0x005A), "no received bits");
    // The data registers are plain storage, so the transfer can be started
    // again with the internal clock and then completes.
    bus.write16(SIOCNT_ADDR, 0x0083)
        .expect("internally clocked start");
    assert_eq!(bus.read16(SIOCNT_ADDR), Ok(0x0007));
    assert_eq!(bus.read16(SIODATA8_ADDR), Ok(0x00FF));
}

#[test]
fn group_five_reset_clears_the_modelled_serial_registers() {
    let mut bus = Bus::new(cart(0x200));
    bus.write16(SIOCNT_ADDR, 0x5003).expect("siocnt");
    bus.write16(RCNT_ADDR, 0x0100).expect("rcnt");
    bus.write16(SIODATA32_L_ADDR, 0xBEEF).expect("data");
    bus.reset_group5_sio();
    assert_eq!(bus.read16(SIOCNT_ADDR), Ok(0x0004));
    // GBATEK: group 5 also switches to general-purpose mode.
    assert_eq!(bus.read16(RCNT_ADDR), Ok(0x8000));
    assert_eq!(bus.read16(SIODATA32_L_ADDR), Ok(0));
}

#[test]
fn rom_write_policy_is_explicit_and_never_stores_cartridge_bytes() {
    let mut strict = Bus::new(cart(0x200));
    assert_eq!(strict.rom_write_policy(), RomWritePolicy::Strict);
    assert!(matches!(
        strict.write16(ROM_BASE + 0xC8, 1),
        Err(Error::RomWrite { .. })
    ));

    let mut lenient = Bus::new(cart(0x200)).with_rom_write_policy(RomWritePolicy::Dropped);
    lenient
        .write16(ROM_BASE + 0xC8, 1)
        .expect("dropped ROM halfword store");
    lenient
        .write32(ROM_BASE, 0xFFFF_FFFF)
        .expect("dropped ROM word store");
    lenient
        .write8(ROM_BASE + 3, 0xFF)
        .expect("dropped ROM byte store");
    // Nothing is stored: the cartridge image is unchanged.
    assert_eq!(lenient.read16(ROM_BASE).expect("rom"), 0);
    // ROM is still readable through the documented window.
    assert_eq!(lenient.read32(ROM_BASE).expect("rom word"), 0);
    // The save/flash region keeps its own explicit error.
    assert!(matches!(
        lenient.write8(0x0E00_0000, 0x01),
        Err(Error::UnsupportedSave { .. })
    ));
    // The policy is host configuration, not guest state.
    let strict_twin = Bus::new(cart(0x200));
    assert_eq!(lenient.state_digest(), strict_twin.state_digest());
}

#[test]
fn backup_policy_is_explicit_and_volatile() {
    let mut strict = Bus::new(cart(0x200));
    assert_eq!(strict.backup_policy(), BackupPolicy::Strict);
    assert!(matches!(
        strict.read8(0x0E00_0000),
        Err(Error::UnsupportedSave { .. })
    ));
    assert!(matches!(
        strict.write8(0x0E00_0000, 0x12),
        Err(Error::UnsupportedSave { .. })
    ));

    let mut sram = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Sram32KVolatile);
    // Erased on construction.
    assert_eq!(sram.read8(0x0E00_0000).expect("erased"), 0xFF);
    sram.write8(0x0E00_0000, 0x12).expect("sram store");
    assert_eq!(sram.read8(0x0E00_0000).expect("stored"), 0x12);
    // The modelled device is byte wide, so wider accesses stay explicit.
    assert!(matches!(
        sram.read16(0x0E00_0000),
        Err(Error::UnsupportedSave { .. })
    ));
    assert!(matches!(
        sram.write16(0x0E00_0000, 0x1234),
        Err(Error::UnsupportedSave { .. })
    ));
    assert!(matches!(
        sram.write32(0x0E00_0000, 1),
        Err(Error::UnsupportedSave { .. })
    ));
    // Outside the modelled 32 KiB the window stays explicit.
    assert!(matches!(
        sram.read8(0x0E00_8000),
        Err(Error::UnsupportedSave { .. })
    ));
    // The backup contents are guest state and change the digest.
    let fresh = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Sram32KVolatile);
    assert_ne!(sram.state_digest(), fresh.state_digest());
}
