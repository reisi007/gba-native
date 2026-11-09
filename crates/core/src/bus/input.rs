use super::{Bus, KEY_MASK, io::keypad};

impl Bus {
    /// Sets the host-injected GBA key mask, where one means pressed.
    ///
    /// KEYINPUT reads expose this state active-low. Bits outside [`KEY_MASK`]
    /// are ignored. This does not provide frame-synchronous sampling or timing.
    pub fn set_pressed_keys(&mut self, pressed: u16) {
        let pressed = pressed & KEY_MASK;
        if pressed != self.pressed_keys {
            self.pressed_keys = pressed;
            keypad::evaluate(&mut self.io, pressed);
        }
    }
}
