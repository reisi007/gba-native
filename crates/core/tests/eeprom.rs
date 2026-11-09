//! Serial EEPROM device model reached through the documented window.
//!
//! Every transfer uses the documented DMA3 shape: 16-bit units with an
//! incrementing source and destination. The read request, the 68-bit answer and
//! the write request are built by the test itself, so the expected bytes are
//! independent of the implementation. GBATEK `#gba-cart-backup-eeprom` is the
//! only source; no emulator code was consulted.

use gba_native_core::bus::{BackupPolicy, Bus, EEPROM_BLOCK_LEN, EEPROM_WINDOW_BASE};
use gba_native_core::cart::Cartridge;
use gba_native_core::error::Error;

const DMA3_SAD: u32 = 0x0400_00D4;
const DMA3_DAD: u32 = 0x0400_00D8;
const DMA3_CNT_L: u32 = 0x0400_00DC;
const DMA3_CNT_H: u32 = 0x0400_00DE;
const DMA0_SAD: u32 = 0x0400_00B0;
const DMA0_DAD: u32 = 0x0400_00B4;
const DMA0_CNT_L: u32 = 0x0400_00B8;
const DMA0_CNT_H: u32 = 0x0400_00BA;
const BUF: u32 = 0x0200_0000;
const READ_STREAM_BITS: u32 = 68;

fn cart(len: usize) -> Cartridge {
    Cartridge::from_bytes(vec![0u8; len]).expect("synthetic cart")
}

/// Builds `request` + n address bits (MSB first) + optional 64 data bits + `0`.
fn command(request: u8, address: u32, address_bits: usize, data: Option<u64>) -> Vec<bool> {
    let mut bits = vec![request & 2 != 0, request & 1 != 0];
    for index in (0..address_bits).rev() {
        bits.push((address >> index) & 1 != 0);
    }
    if let Some(value) = data {
        for index in (0..64).rev() {
            bits.push((value >> index) & 1 != 0);
        }
    }
    bits.push(false);
    bits
}

fn load_bits(bus: &mut Bus, bits: &[bool]) {
    for (index, bit) in bits.iter().enumerate() {
        bus.write16(BUF + 2 * index as u32, u16::from(*bit))
            .expect("buffer halfword");
    }
}

fn dma3(bus: &mut Bus, source: u32, destination: u32, units: u32) {
    dma3_result(bus, source, destination, units).expect("16-bit incrementing DMA3");
}

fn dma3_result(
    bus: &mut Bus,
    source: u32,
    destination: u32,
    units: u32,
) -> std::result::Result<(), Error> {
    bus.write32(DMA3_SAD, source).expect("sad");
    bus.write32(DMA3_DAD, destination).expect("dad");
    bus.write16(DMA3_CNT_L, units as u16).expect("count");
    bus.write16(DMA3_CNT_H, 0x8000)
}

/// Collects the 64 data bits that follow the four ignored answer bits.
fn read_data(bus: &Bus) -> u64 {
    let mut value = 0_u64;
    for index in 0..64 {
        let bit = bus.read16(BUF + 2 * (4 + index)).expect("answer halfword") & 1;
        value = (value << 1) | u64::from(bit);
    }
    value
}

#[test]
fn eeprom_512_round_trips_one_block_through_dma3() {
    let mut bus = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom512Volatile);
    assert_eq!(bus.backup_policy(), BackupPolicy::Eeprom512Volatile);
    assert_eq!(bus.backup_image().len(), 512);

    let expected = 0x0123_4567_89AB_CDEF_u64;
    let write = command(0b10, 3, 6, Some(expected));
    load_bits(&mut bus, &write);
    dma3(&mut bus, BUF, EEPROM_WINDOW_BASE, write.len() as u32);
    // A completed write is immediately ready; the documented poll reads 1.
    assert_eq!(bus.read16(EEPROM_WINDOW_BASE), Ok(1));
    assert_eq!(
        &bus.backup_image()[3 * EEPROM_BLOCK_LEN..4 * EEPROM_BLOCK_LEN],
        &expected.to_be_bytes()
    );

    let request = command(0b11, 3, 6, None);
    load_bits(&mut bus, &request);
    dma3(&mut bus, BUF, EEPROM_WINDOW_BASE, request.len() as u32);
    dma3(&mut bus, EEPROM_WINDOW_BASE, BUF, READ_STREAM_BITS);
    assert_eq!(read_data(&bus), expected);
}

#[test]
fn eeprom_8k_masks_the_address_to_its_lower_ten_bits() {
    let mut bus = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom8KVolatile);
    assert_eq!(bus.backup_image().len(), 8 * 1024);

    // 0x401 has the documented 14-bit width; only the lower 10 bits address
    // the part, so this is block 1.
    let expected = 0xDEAD_BEEF_0123_4567_u64;
    let write = command(0b10, 0x401, 14, Some(expected));
    load_bits(&mut bus, &write);
    dma3(&mut bus, BUF, EEPROM_WINDOW_BASE, write.len() as u32);
    assert_eq!(
        &bus.backup_image()[EEPROM_BLOCK_LEN..2 * EEPROM_BLOCK_LEN],
        &expected.to_be_bytes()
    );

    // Reading through the masked address returns the same block.
    let request = command(0b11, 1, 14, None);
    load_bits(&mut bus, &request);
    dma3(&mut bus, BUF, EEPROM_WINDOW_BASE, request.len() as u32);
    dma3(&mut bus, EEPROM_WINDOW_BASE, BUF, READ_STREAM_BITS);
    assert_eq!(read_data(&bus), expected);
}

#[test]
fn malformed_read_and_write_terminators_are_rejected_without_data_effects() {
    let mut write_bus = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom512Volatile);
    let erased = write_bus.backup_image().to_vec();
    let mut malformed_write = command(0b10, 0, 6, Some(0x0123_4567_89AB_CDEF));
    *malformed_write.last_mut().expect("write terminator") = true;
    load_bits(&mut write_bus, &malformed_write);
    assert!(matches!(
        dma3_result(
            &mut write_bus,
            BUF,
            EEPROM_WINDOW_BASE,
            malformed_write.len() as u32
        ),
        Err(Error::UnsupportedSave { .. })
    ));
    assert_eq!(write_bus.backup_image(), erased);

    let mut read_bus = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom512Volatile);
    let mut malformed_read = command(0b11, 0, 6, None);
    *malformed_read.last_mut().expect("read terminator") = true;
    load_bits(&mut read_bus, &malformed_read);
    assert!(matches!(
        dma3_result(
            &mut read_bus,
            BUF,
            EEPROM_WINDOW_BASE,
            malformed_read.len() as u32
        ),
        Err(Error::UnsupportedSave { .. })
    ));
    // A rejected read did not leave a response stream; idle EEPROM polls ready.
    assert_eq!(read_bus.read16(EEPROM_WINDOW_BASE), Ok(1));
}

#[test]
fn manual_stores_are_rejected_and_ready_poll_is_allowed() {
    let mut bus = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom512Volatile);
    // GBATEK: manual LDRH/STRH do not keep the chip selected.
    assert!(matches!(
        bus.write16(EEPROM_WINDOW_BASE, 1),
        Err(Error::UnsupportedSave { .. })
    ));
    assert_eq!(bus.read16(EEPROM_WINDOW_BASE), Ok(1));
    // Only the 16-bit serial path is documented; byte and word accesses stay
    // explicit errors instead of aliasing the serial bit.
    assert!(matches!(
        bus.read8(EEPROM_WINDOW_BASE),
        Err(Error::UnsupportedSave { .. })
    ));
    assert!(matches!(
        bus.write8(EEPROM_WINDOW_BASE, 1),
        Err(Error::UnsupportedSave { .. })
    ));
    assert!(matches!(
        bus.read32(EEPROM_WINDOW_BASE),
        Err(Error::UnsupportedSave { .. })
    ));
    assert!(matches!(
        bus.fetch16(EEPROM_WINDOW_BASE),
        Err(Error::NonExecutable { .. })
    ));
    assert!(matches!(
        bus.fetch32(EEPROM_WINDOW_BASE),
        Err(Error::NonExecutable { .. })
    ));
    assert!(matches!(
        bus.write32(EEPROM_WINDOW_BASE, 0),
        Err(Error::UnsupportedSave { .. })
    ));

    // Manual reads cannot consume a DMA read stream either. The answer remains
    // intact for the subsequent documented DMA3 transfer.
    let request = command(0b11, 0, 6, None);
    load_bits(&mut bus, &request);
    dma3(&mut bus, BUF, EEPROM_WINDOW_BASE, request.len() as u32);
    assert!(matches!(
        bus.read16(EEPROM_WINDOW_BASE),
        Err(Error::UnsupportedSave { .. })
    ));
    dma3(&mut bus, EEPROM_WINDOW_BASE, BUF, READ_STREAM_BITS);
    assert_eq!(read_data(&bus), u64::MAX);
}

#[test]
fn dma_shape_and_other_regions_stay_explicit() {
    // Any DMA channel other than DMA3 must reject reads from the EEPROM
    // overlay, rather than interpreting the idle-ready bit as transferred data.
    let mut wrong_channel =
        Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom512Volatile);
    wrong_channel
        .write32(DMA0_SAD, EEPROM_WINDOW_BASE)
        .expect("sad");
    wrong_channel.write32(DMA0_DAD, BUF).expect("dad");
    wrong_channel.write16(DMA0_CNT_L, 1).expect("count");
    assert!(matches!(
        wrong_channel.write16(DMA0_CNT_H, 0x8000),
        Err(Error::UnsupportedDma { .. })
    ));
    assert_eq!(wrong_channel.read16(BUF), Ok(0));

    // Transfers starting outside the overlay are still rejected if later
    // incrementing units cross into its address range.
    let mut crossing = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom512Volatile);
    crossing
        .write32(DMA0_SAD, EEPROM_WINDOW_BASE - 2)
        .expect("sad");
    crossing.write32(DMA0_DAD, BUF).expect("dad");
    crossing.write16(DMA0_CNT_L, 2).expect("count");
    assert!(matches!(
        crossing.write16(DMA0_CNT_H, 0x8000),
        Err(Error::UnsupportedDma { .. })
    ));
    assert_eq!(crossing.read16(BUF), Ok(0));
    // Keep the failed enable from affecting the following independent checks.
    wrong_channel.write16(DMA0_CNT_H, 0).expect("disable DMA0");
    crossing.write16(DMA0_CNT_H, 0).expect("disable DMA0");

    // 32-bit DMA into the serial window is not the documented shape.
    let mut word = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom512Volatile);
    word.write32(DMA3_SAD, BUF).expect("sad");
    word.write32(DMA3_DAD, EEPROM_WINDOW_BASE).expect("dad");
    word.write16(DMA3_CNT_L, 1).expect("count");
    assert!(matches!(
        word.write16(DMA3_CNT_H, 0x8400),
        Err(Error::UnsupportedDma { .. })
    ));
    // A fixed destination is rejected too.
    let mut fixed = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom512Volatile);
    fixed.write32(DMA3_SAD, BUF).expect("sad");
    fixed.write32(DMA3_DAD, EEPROM_WINDOW_BASE).expect("dad");
    fixed.write16(DMA3_CNT_L, 1).expect("count");
    assert!(matches!(
        fixed.write16(DMA3_CNT_H, 0x8040),
        Err(Error::UnsupportedDma { .. })
    ));

    // A partial DMA transaction is an explicit error and releases serial
    // framing; the next DMA starts at its own first request bit.
    let mut partial = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom512Volatile);
    load_bits(&mut partial, &[true]);
    partial.write32(DMA3_SAD, BUF).expect("sad");
    partial.write32(DMA3_DAD, EEPROM_WINDOW_BASE).expect("dad");
    partial.write16(DMA3_CNT_L, 1).expect("count");
    assert!(matches!(
        partial.write16(DMA3_CNT_H, 0x8000),
        Err(Error::UnsupportedSave { .. })
    ));
    partial.write16(DMA3_CNT_H, 0).expect("disable failed DMA");
    let complete = command(0b10, 2, 6, Some(0xA5A5_5A5A_C3C3_3C3C));
    load_bits(&mut partial, &complete);
    dma3(&mut partial, BUF, EEPROM_WINDOW_BASE, complete.len() as u32);
    assert_eq!(
        &partial.backup_image()[2 * EEPROM_BLOCK_LEN..3 * EEPROM_BLOCK_LEN],
        &0xA5A5_5A5A_C3C3_3C3C_u64.to_be_bytes()
    );

    // The SRAM/flash window is not an EEPROM device, and the ROM outside the
    // serial window still reads normally.
    let bus = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom512Volatile);
    assert!(matches!(
        bus.read8(0x0E00_0000),
        Err(Error::UnsupportedSave { .. })
    ));
    assert_eq!(bus.read16(0x0800_0000), Ok(0));
    // Without the device opt-in, the same address stays on the ordinary ROM
    // path (and this short synthetic image naturally overreads it).
    let strict = Bus::new(cart(0x200));
    assert!(matches!(
        strict.read16(EEPROM_WINDOW_BASE),
        Err(Error::RomOverread { .. })
    ));
    // The window is the last 256 bytes of the ROM address space; the byte
    // before it is ordinary ROM again.
    assert_eq!(
        bus.read16(EEPROM_WINDOW_BASE - 2),
        Err(Error::RomOverread {
            addr: EEPROM_WINDOW_BASE - 2,
            len: 0x200,
        })
    );
}

#[test]
fn eeprom_state_is_guest_state_and_the_policy_is_not() {
    let strict = Bus::new(cart(0x200));
    let eeprom = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom512Volatile);
    // An equally untouched device has the same guest-state digest.
    assert_eq!(strict.state_digest(), eeprom.state_digest());

    let mut written = Bus::new(cart(0x200)).with_backup_policy(BackupPolicy::Eeprom512Volatile);
    let write = command(0b10, 0, 6, Some(u64::MAX));
    load_bits(&mut written, &write);
    dma3(&mut written, BUF, EEPROM_WINDOW_BASE, write.len() as u32);
    assert_ne!(written.state_digest(), eeprom.state_digest());
}
