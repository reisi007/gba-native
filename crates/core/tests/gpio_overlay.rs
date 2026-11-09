use gba_native_core::bus::{Bus, GpioPolicy};
use gba_native_core::cart::Cartridge;
use gba_native_core::error::Error;

const GPIO: u32 = 0x0800_00C4;

fn bus() -> Bus {
    let mut image = vec![0_u8; 0x200];
    for (i, byte) in image.iter_mut().enumerate() {
        *byte = i as u8;
    }
    Bus::new(Cartridge::from_bytes(image).expect("synthetic cartridge"))
}

#[test]
fn default_mapping_and_strict_rom_stores_are_unchanged() {
    let mut bus = bus();
    assert_eq!(bus.read16(GPIO).unwrap(), 0xC5C4);
    assert_eq!(bus.read32(GPIO).unwrap(), 0xC7C6_C5C4);
    assert!(matches!(bus.write16(GPIO, 0), Err(Error::RomWrite { .. })));
    assert_eq!(bus.read16(GPIO).unwrap(), 0xC5C4);
}

#[test]
fn enabled_registers_mask_values_and_overlay_reads() {
    let mut bus = bus().with_gpio_policy(GpioPolicy::DocumentedStorage);
    bus.write16(GPIO, 0xFFF5).unwrap();
    bus.write16(GPIO + 2, 0xFFF0).unwrap();
    bus.write16(GPIO + 4, 0xFFFF).unwrap();
    assert_eq!(bus.read16(GPIO + 2).unwrap(), 0);
    assert_eq!(bus.read16(GPIO + 4).unwrap(), 1);
    assert_eq!(bus.read16(GPIO).unwrap(), 0);
    bus.write16(GPIO + 2, 0xFFF5).unwrap();
    assert_eq!(bus.read16(GPIO + 2).unwrap(), 5);
    bus.set_gpio_input(0xFA);
    assert_eq!(bus.read16(GPIO).unwrap(), 0x0F);
    assert_eq!(bus.gpio_data(), 0x0F);
}

#[test]
fn direction_selects_injected_input_or_output_latch() {
    let mut bus = bus().with_gpio_policy(GpioPolicy::DocumentedStorage);
    bus.set_gpio_input(0x0A);
    bus.write16(GPIO, 0x05).unwrap();
    bus.write16(GPIO + 2, 0x05).unwrap();
    bus.write16(GPIO + 4, 1).unwrap();
    assert_eq!(bus.read16(GPIO).unwrap(), 0x0F);
    assert_eq!(bus.gpio_data(), 0x0F);
}

#[test]
fn documented_word_access_composes_data_and_direction() {
    let mut bus = bus().with_gpio_policy(GpioPolicy::DocumentedStorage);
    bus.write32(GPIO, 0x000A_000A).unwrap();
    bus.write16(GPIO + 4, 1).unwrap();
    assert_eq!(bus.read32(GPIO).unwrap(), 0x000A_000A);
}

#[test]
fn control_word_read_composes_with_the_following_rom_halfword() {
    let mut bus = bus().with_gpio_policy(GpioPolicy::DocumentedStorage);
    bus.write16(GPIO + 4, 1).unwrap();
    assert_eq!(bus.read32(GPIO + 4).unwrap(), 0xCBCA_0001);
}

#[test]
fn control_word_store_reports_the_following_strict_rom_store() {
    let mut bus = bus().with_gpio_policy(GpioPolicy::DocumentedStorage);
    assert!(matches!(
        bus.write32(GPIO + 4, 0x1234_0001),
        Err(Error::RomWrite { addr: 0x0800_00CA })
    ));
    assert_eq!(bus.read16(GPIO + 4).unwrap(), 1);
}

#[test]
fn unsupported_gpio_widths_fail_and_control_zero_reads_rom() {
    let mut bus = bus().with_gpio_policy(GpioPolicy::DocumentedStorage);
    assert_eq!(bus.read16(GPIO).unwrap(), 0xC5C4);
    assert!(matches!(
        bus.write8(GPIO, 1),
        Err(Error::UnsupportedCartGpio { width: 8, .. })
    ));
    bus.write16(GPIO + 4, 1).unwrap();
    assert!(matches!(
        bus.read8(GPIO),
        Err(Error::UnsupportedCartGpio { width: 8, .. })
    ));
    assert!(matches!(
        bus.read16(GPIO + 1),
        Err(Error::UnalignedHalfword { .. })
    ));
}

#[test]
fn gpio_offsets_beyond_image_overread_until_overlay_is_selected() {
    let image = vec![0_u8; 192];
    let mut bus = Bus::new(Cartridge::from_bytes(image).unwrap())
        .with_gpio_policy(GpioPolicy::DocumentedStorage);
    assert!(matches!(
        bus.read16(GPIO + 2),
        Err(Error::RomOverread { .. })
    ));
    assert!(matches!(
        bus.read16(GPIO + 4),
        Err(Error::RomOverread { .. })
    ));
    bus.write16(GPIO + 4, 1).unwrap();
    assert_eq!(bus.read16(GPIO + 2).unwrap(), 0);
    assert_eq!(bus.read16(GPIO + 4).unwrap(), 1);
}

#[test]
fn policy_is_configuration_but_gpio_registers_and_injection_are_guest_state() {
    let strict = bus();
    let gpio = bus().with_gpio_policy(GpioPolicy::DocumentedStorage);
    assert_eq!(strict.state_digest(), gpio.state_digest());
    let mut changed = gpio;
    let before = changed.state_digest();
    changed.set_gpio_input(1);
    assert_ne!(before, changed.state_digest());
    let before = changed.state_digest();
    changed.write16(GPIO + 2, 1).unwrap();
    assert_ne!(before, changed.state_digest());
    let reset = bus().with_gpio_policy(GpioPolicy::DocumentedStorage);
    assert_eq!(reset.gpio_data(), 0);
}
