use super::IoState;

const KEY_SELECTION: u16 = 0x03FF;
const KEYCNT_WRITABLE: u16 = 0xC3FF;
const IF_KEYPAD: u16 = 1 << 12;

pub(super) fn write(io: &mut IoState, value: u16, pressed_keys: u16) {
    io.keycnt = value & KEYCNT_WRITABLE;
    evaluate(io, pressed_keys);
}

pub(in crate::bus) fn evaluate(io: &mut IoState, pressed_keys: u16) {
    let selected = io.keycnt & KEY_SELECTION;
    if io.keycnt & (1 << 14) == 0 || selected == 0 {
        return;
    }
    let matched = if io.keycnt & (1 << 15) == 0 {
        selected & pressed_keys != 0
    } else {
        selected & pressed_keys == selected
    };
    if matched {
        io.irq.latch_requests(IF_KEYPAD);
    }
}
