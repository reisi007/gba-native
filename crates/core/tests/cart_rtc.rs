use gba_native_core::{
    bus::{Bus, GpioPolicy, RtcDateTime},
    cart::Cartridge,
    error::Error,
};

const GPIO_DATA: u32 = 0x0800_00C4;
const GPIO_DIR: u32 = 0x0800_00C6;
const GPIO_CONTROL: u32 = 0x0800_00C8;

fn bus() -> Bus {
    Bus::new(Cartridge::from_bytes(vec![0; 0x200]).expect("synthetic cart"))
        .with_gpio_policy(GpioPolicy::DocumentedRtc)
}

fn datetime(
    year: u16,
    month: u8,
    day: u8,
    weekday: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> RtcDateTime {
    RtcDateTime {
        year,
        month,
        day,
        weekday,
        hour,
        minute,
        second,
    }
}

fn configure(bus: &mut Bus, direction: u16) {
    bus.write16(GPIO_CONTROL, 1).expect("enable GPIO reads");
    bus.write16(GPIO_DIR, direction)
        .expect("set GPIO direction");
    bus.write16(GPIO_DATA, 0).expect("clear GPIO pins");
}

fn command_byte(command: u8, read: bool, reversed: bool) -> (u8, bool) {
    if reversed {
        let reversed_command = ((command & 1) << 2) | (command & 2) | ((command & 4) >> 2);
        (0x60 | (reversed_command << 1) | u8::from(read), true)
    } else {
        ((command << 4) | 0x06 | (u8::from(read) << 7), false)
    }
}

fn write_bit(bus: &mut Bus, bit: bool) -> gba_native_core::Result<()> {
    let data = 4 | (u16::from(bit) << 1);
    bus.write16(GPIO_DATA, data)?;
    bus.write16(GPIO_DATA, data | 1)?;
    bus.write16(GPIO_DATA, data)
}

fn send_byte(bus: &mut Bus, byte: u8, msb_first: bool) -> gba_native_core::Result<()> {
    for index in 0..8 {
        let bit = if msb_first { 7 - index } else { index };
        write_bit(bus, byte & (1 << bit) != 0)?;
    }
    Ok(())
}

fn begin_command(
    bus: &mut Bus,
    command: u8,
    read: bool,
    reversed: bool,
) -> gba_native_core::Result<()> {
    configure(bus, 7);
    bus.write16(GPIO_DATA, 4)?; // CS high, SCK low
    let (byte, msb_first) = command_byte(command, read, reversed);
    send_byte(bus, byte, msb_first)
}

fn write_transaction(
    bus: &mut Bus,
    command: u8,
    reversed: bool,
    bytes: &[u8],
) -> gba_native_core::Result<()> {
    let start = begin_command(bus, command, false, reversed);
    let result = start.and_then(|()| {
        for byte in bytes {
            send_byte(bus, *byte, false)?;
        }
        Ok(())
    });
    let close = bus.write16(GPIO_DATA, 0); // deselect also aborts an incomplete command
    result?;
    close?;
    Ok(())
}

fn read_transaction(
    bus: &mut Bus,
    command: u8,
    reversed: bool,
    byte_count: usize,
) -> gba_native_core::Result<Vec<u8>> {
    let start = begin_command(bus, command, true, reversed);
    let result = start.and_then(|()| {
        bus.write16(GPIO_DIR, 5)?; // SCK/CS output; SIO input
        let mut bytes = vec![0; byte_count];
        for byte in &mut bytes {
            for bit in 0..8 {
                if bus.read16(GPIO_DATA)? & 2 != 0 {
                    *byte |= 1 << bit;
                }
                bus.write16(GPIO_DATA, 5)?; // rising edge: consume sampled bit
                bus.write16(GPIO_DATA, 4)?; // falling edge: present next bit
            }
        }
        Ok(bytes)
    });
    let close = bus.write16(GPIO_DATA, 0);
    let bytes = result?;
    close?;
    Ok(bytes)
}

fn control_read(bus: &mut Bus, reversed: bool) -> gba_native_core::Result<u8> {
    Ok(read_transaction(bus, 4, reversed, 1)?[0])
}

#[test]
fn strict_storage_and_default_rtc_policies_keep_existing_mapping_and_digest() {
    let mut image = vec![0; 0x200];
    image[0xC4..0xC8].copy_from_slice(&[0xC4, 0xC5, 0xC6, 0xC7]);
    let cart = || gba_native_core::cart::Cartridge::from_bytes(image.clone()).unwrap();
    let strict = Bus::new(cart());
    let storage = Bus::new(cart()).with_gpio_policy(GpioPolicy::DocumentedStorage);
    let rtc = Bus::new(cart()).with_gpio_policy(GpioPolicy::DocumentedRtc);
    assert_eq!(strict.read16(GPIO_DATA).unwrap(), 0xC5C4);
    assert_eq!(strict.state_digest(), storage.state_digest());
    assert_eq!(strict.state_digest(), rtc.state_digest());
    assert!(matches!(
        Bus::new(cart()).set_rtc_datetime(datetime(2024, 1, 1, 1, 0, 0, 0)),
        Err(Error::RtcPolicyRequired)
    ));
}

#[test]
fn host_datetime_is_validated_and_is_guest_state() {
    let mut rtc = bus();
    let initial = rtc.state_digest();
    rtc.set_rtc_datetime(datetime(2024, 2, 29, 4, 23, 59, 58))
        .expect("valid leap day");
    let valid_digest = rtc.state_digest();
    assert_ne!(valid_digest, initial);
    for invalid in [
        datetime(2023, 2, 29, 4, 23, 59, 58),
        datetime(2100, 1, 1, 5, 0, 0, 0),
        datetime(2024, 13, 1, 1, 0, 0, 0),
        datetime(2024, 1, 0, 1, 0, 0, 0),
        datetime(2024, 1, 1, 7, 0, 0, 0),
        datetime(2024, 1, 1, 1, 24, 0, 0),
        datetime(2024, 1, 1, 1, 0, 60, 0),
        datetime(2024, 1, 1, 1, 0, 0, 60),
    ] {
        assert!(matches!(
            rtc.set_rtc_datetime(invalid),
            Err(Error::InvalidRtcDate)
        ));
        assert_eq!(
            rtc.state_digest(),
            valid_digest,
            "rejected date mutated RTC"
        );
    }
}

#[test]
fn datetime_reads_use_bcd_and_both_documented_command_orders() {
    for reversed in [false, true] {
        let mut rtc = bus();
        rtc.set_rtc_datetime(datetime(2024, 2, 29, 4, 23, 59, 58))
            .unwrap();
        assert_eq!(
            read_transaction(&mut rtc, 2, reversed, 7).unwrap(),
            [0x24, 0x02, 0x29, 4, 0x91, 0x59, 0x58]
        );
    }
}

#[test]
fn control_mask_and_host_clock_hour_modes_are_applied_to_reads_and_writes() {
    let mut rtc = bus();
    write_transaction(&mut rtc, 4, false, &[0xff]).unwrap();
    assert_eq!(control_read(&mut rtc, false).unwrap(), 0x6a);

    // In the documented 12-hour representation both midnight and noon are
    // 00h, distinguished by the AM/PM bit. The host API remains 24-hour.
    for (hour, twelve_hour) in [(0, 0x00), (11, 0x11), (12, 0x80), (23, 0x91)] {
        rtc.set_rtc_datetime(datetime(2024, 5, 6, 1, hour, 59, 58))
            .unwrap();
        assert_eq!(
            read_transaction(&mut rtc, 6, false, 3).unwrap(),
            [twelve_hour, 0x59, 0x58],
            "12-hour read for host hour {hour}"
        );

        write_transaction(&mut rtc, 6, false, &[twelve_hour, 0x12, 0x34]).unwrap();
        assert_eq!(
            read_transaction(&mut rtc, 6, false, 3).unwrap(),
            [twelve_hour, 0x12, 0x34],
            "12-hour write for host hour {hour}"
        );

        write_transaction(&mut rtc, 4, false, &[0x40]).unwrap();
        let twenty_four_hour = bcd(hour) | if hour >= 12 { 0x80 } else { 0 };
        let contradictory_pm = bcd(hour) | if hour < 12 { 0x80 } else { 0 };
        // In 24-hour mode bit 7 is read-only; the BCD hour determines PM.
        write_transaction(&mut rtc, 6, false, &[contradictory_pm, 0x12, 0x34]).unwrap();
        assert_eq!(
            read_transaction(&mut rtc, 6, false, 3).unwrap(),
            [twenty_four_hour, 0x12, 0x34],
            "24-hour conversion after writing host hour {hour}"
        );
        assert_eq!(
            read_transaction(&mut rtc, 2, false, 7).unwrap(),
            [0x24, 0x05, 0x06, 1, twenty_four_hour, 0x12, 0x34],
            "24-hour datetime read for host hour {hour}"
        );
        write_transaction(&mut rtc, 4, false, &[0]).unwrap();
    }

    rtc.set_rtc_datetime(datetime(2024, 5, 6, 1, 23, 59, 58))
        .unwrap();
    write_transaction(&mut rtc, 4, false, &[0x40]).unwrap();
    write_transaction(&mut rtc, 6, false, &[0x00, 0x01, 0x02]).unwrap();
    assert_eq!(
        read_transaction(&mut rtc, 6, false, 3).unwrap(),
        [0x00, 0x01, 0x02]
    );
}

fn bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

#[test]
fn datetime_write_validates_bcd_before_committing_and_deselect_aborts_partial_data() {
    let mut rtc = bus();
    rtc.set_rtc_datetime(datetime(2024, 1, 2, 1, 3, 4, 5))
        .unwrap();
    write_transaction(&mut rtc, 2, false, &[0x26, 0x03, 0x12, 4, 0x80, 0x34, 0x56]).unwrap();
    assert_eq!(
        read_transaction(&mut rtc, 2, false, 7).unwrap(),
        [0x26, 0x03, 0x12, 4, 0x80, 0x34, 0x56]
    );

    begin_command(&mut rtc, 2, false, false).unwrap();
    send_byte(&mut rtc, 0x27, false).unwrap();
    send_byte(&mut rtc, 0x04, false).unwrap();
    rtc.write16(GPIO_DATA, 0).unwrap(); // incomplete command is aborted
    assert_eq!(
        read_transaction(&mut rtc, 2, false, 7).unwrap(),
        [0x26, 0x03, 0x12, 4, 0x80, 0x34, 0x56]
    );

    assert!(matches!(
        write_transaction(&mut rtc, 2, false, &[0x2a, 0x04, 0x12, 4, 0x92, 0x34, 0x56]),
        Err(Error::InvalidRtcBcd)
    ));
    assert!(matches!(
        write_transaction(&mut rtc, 2, false, &[0x27, 0x04, 0x12, 4, 0x12, 0x34, 0x56]),
        Err(Error::InvalidRtcDate)
    ));
    assert_eq!(
        read_transaction(&mut rtc, 2, false, 7).unwrap(),
        [0x26, 0x03, 0x12, 4, 0x80, 0x34, 0x56]
    );
}

#[test]
fn force_reset_constant_ff_registers_and_force_irq_error_are_explicit() {
    let mut rtc = bus();
    rtc.set_rtc_datetime(datetime(2038, 7, 8, 4, 17, 18, 19))
        .unwrap();
    write_transaction(&mut rtc, 4, false, &[0x40]).unwrap();
    for (command, count) in [(1, 1), (5, 3), (7, 1)] {
        assert_eq!(
            read_transaction(&mut rtc, command, command == 5, count).unwrap(),
            vec![0xff; count]
        );
        write_transaction(&mut rtc, command, false, &vec![0; count]).unwrap();
    }

    assert!(matches!(
        write_transaction(&mut rtc, 3, false, &[]),
        Err(Error::RtcForceIrqUnsupported)
    ));

    configure(&mut rtc, 7);
    rtc.write16(GPIO_DATA, 4).unwrap();
    assert!(matches!(
        send_byte(&mut rtc, 0, false),
        Err(Error::InvalidRtcCommand(0))
    ));
    rtc.write16(GPIO_DATA, 0).unwrap();

    write_transaction(&mut rtc, 0, true, &[]).unwrap();
    assert_eq!(control_read(&mut rtc, false).unwrap(), 0);
    assert_eq!(
        read_transaction(&mut rtc, 2, false, 7).unwrap(),
        [0, 1, 1, 0, 0, 0, 0]
    );
}

#[test]
fn read_data_drives_only_input_sio_and_partial_rtc_state_is_digested() {
    let mut output_sio = bus();
    output_sio
        .set_rtc_datetime(datetime(2024, 1, 1, 1, 0, 0, 0))
        .unwrap();
    configure(&mut output_sio, 7);
    output_sio.write16(GPIO_DATA, 4).unwrap();
    let (command, msb_first) = command_byte(2, true, false);
    send_byte(&mut output_sio, command, msb_first).unwrap();
    assert_eq!(output_sio.gpio_data() & 2, 2); // output latch wins over RTC SIO
    output_sio.write16(GPIO_DATA, 0).unwrap();

    let mut first = bus();
    let mut second = bus();
    configure(&mut first, 7);
    configure(&mut second, 7);
    first.write16(GPIO_DATA, 4).unwrap();
    second.write16(GPIO_DATA, 4).unwrap();
    write_bit(&mut first, true).unwrap();
    write_bit(&mut second, false).unwrap();
    first.write16(GPIO_DATA, 4).unwrap();
    second.write16(GPIO_DATA, 4).unwrap();
    // Make physical GPIO registers identical; the partial serial command still
    // changes guest-visible RTC state and must therefore remain in the digest.
    second.write16(GPIO_DATA, 4).unwrap();
    assert_eq!(first.gpio_data(), second.gpio_data());
    assert_ne!(first.state_digest(), second.state_digest());
}
