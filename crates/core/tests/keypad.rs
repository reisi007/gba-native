use gba_native_core::bus::{Bus, KEY_A, KEY_B, KEYCNT_ADDR};
use gba_native_core::cart::Cartridge;

const IF_ADDR: u32 = 0x0400_0202;
const IF_KEYPAD: u16 = 1 << 12;

fn bus() -> Bus {
    Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"))
}

#[test]
fn keycnt_events_mask_controls_and_latch_or_and_matches() {
    let mut bus = bus();
    assert_eq!(bus.read16(KEYCNT_ADDR), Ok(0));
    bus.set_pressed_keys(KEY_A | KEY_B);
    bus.write16(KEYCNT_ADDR, 0x7C01).expect("KEYCNT");
    assert_eq!(bus.read16(KEYCNT_ADDR), Ok(0x4001));
    assert_eq!(bus.read16(IF_ADDR), Ok(IF_KEYPAD));
    assert!(!bus.irq_pending(), "IE/IME gate delivery, not IF latching");

    bus.write16(IF_ADDR, IF_KEYPAD).expect("clear IF");
    bus.set_pressed_keys(KEY_A | KEY_B);
    assert_eq!(bus.read16(IF_ADDR), Ok(0), "same state is not a new event");
    bus.set_pressed_keys(KEY_B);
    assert_eq!(bus.read16(IF_ADDR), Ok(0));
    bus.write16(KEYCNT_ADDR, 0xC003).expect("AND configuration");
    assert_eq!(bus.read16(IF_ADDR), Ok(0));
    bus.set_pressed_keys(KEY_A | KEY_B);
    assert_eq!(bus.read16(IF_ADDR), Ok(IF_KEYPAD));

    bus.write16(IF_ADDR, IF_KEYPAD).expect("clear IF");
    bus.set_pressed_keys(0);
    for (control, expected) in [(0x7C00, 0x4000), (0xFC00, 0xC000)] {
        bus.write16(KEYCNT_ADDR, control)
            .expect("empty key selection");
        assert_eq!(bus.read16(KEYCNT_ADDR), Ok(expected));
        assert_eq!(bus.read16(IF_ADDR), Ok(0), "empty selection never matches");
    }
}

#[test]
fn keycnt_state_is_hashed_and_cleared_by_group_seven_reset() {
    let mut bus = bus();
    let initial = bus.state_digest();
    bus.write16(KEYCNT_ADDR, 1).expect("select A");
    assert_ne!(bus.state_digest(), initial);
    bus.write16(KEYCNT_ADDR, 0).expect("clear KEYCNT");
    assert_eq!(bus.state_digest(), initial);

    bus.set_pressed_keys(KEY_A);
    bus.write16(KEYCNT_ADDR, KEY_A).expect("disabled A IRQ");
    assert_eq!(bus.read16(IF_ADDR), Ok(0));
    bus.write16(KEYCNT_ADDR, 0x4001).expect("enable A IRQ");
    assert_eq!(bus.read16(IF_ADDR), Ok(IF_KEYPAD));
    bus.reset_group7_io();
    assert_eq!(bus.read16(KEYCNT_ADDR), Ok(0));
    assert_eq!(bus.read16(IF_ADDR), Ok(0));
}

#[test]
fn keycnt_or_match_latches_when_host_keys_change() {
    let mut bus = bus();
    bus.write16(KEYCNT_ADDR, 0x4002).expect("select B");
    bus.set_pressed_keys(KEY_A);
    assert_eq!(bus.read16(IF_ADDR), Ok(0));
    bus.set_pressed_keys(KEY_A | KEY_B);
    assert_eq!(bus.read16(IF_ADDR), Ok(IF_KEYPAD));
}
