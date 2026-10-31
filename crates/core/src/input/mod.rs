//! Host input model: buttons, key bindings, touch layout, settings and scripts.
//!
//! This module is deliberately platform-neutral and std-only. It maps host
//! keyboard/touch identities to the documented KEYINPUT bits, keeps a persisted
//! settings representation and can replay a scripted input log against the
//! bounded runner. It does not add frame-synchronous sampling or timing
//! guarantees; callers choose when a mask is applied.

mod keys;
mod script;
mod settings;
mod touch;

pub use keys::{BindingError, Key, KeyBindings};
pub use script::{InputEvent, InputScript, ScriptError, run_scripted, run_scripted_audio};
pub use settings::{SETTINGS_SCHEMA, Settings, SettingsError};
pub use touch::{GbaLayout, Rect, TouchLayout};

/// GBA CPU clock in Hz, re-exported for callers of the input model.
pub use crate::perf::GBA_CPU_HZ;

/// One of the ten documented GBA key bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Button {
    A,
    B,
    Select,
    Start,
    Right,
    Left,
    Up,
    Down,
    R,
    L,
}

impl Button {
    /// Stable UI order: the diamond, the two menu keys and the shoulders.
    pub const ALL: [Button; 10] = [
        Button::A,
        Button::B,
        Button::Select,
        Button::Start,
        Button::Right,
        Button::Left,
        Button::Up,
        Button::Down,
        Button::R,
        Button::L,
    ];

    #[must_use]
    pub const fn bit(self) -> u16 {
        use crate::bus::{
            KEY_A, KEY_B, KEY_DOWN, KEY_L, KEY_LEFT, KEY_R, KEY_RIGHT, KEY_SELECT, KEY_START,
            KEY_UP,
        };
        match self {
            Button::A => KEY_A,
            Button::B => KEY_B,
            Button::Select => KEY_SELECT,
            Button::Start => KEY_START,
            Button::Right => KEY_RIGHT,
            Button::Left => KEY_LEFT,
            Button::Up => KEY_UP,
            Button::Down => KEY_DOWN,
            Button::R => KEY_R,
            Button::L => KEY_L,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Button::A => "A",
            Button::B => "B",
            Button::Select => "Select",
            Button::Start => "Start",
            Button::Right => "Right",
            Button::Left => "Left",
            Button::Up => "Up",
            Button::Down => "Down",
            Button::R => "R",
            Button::L => "L",
        }
    }

    /// Parse a case-insensitive button name (`a`, `start`, `up`, `l`, ...).
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "a" => Some(Button::A),
            "b" => Some(Button::B),
            "select" => Some(Button::Select),
            "start" => Some(Button::Start),
            "right" => Some(Button::Right),
            "left" => Some(Button::Left),
            "up" => Some(Button::Up),
            "down" => Some(Button::Down),
            "r" => Some(Button::R),
            "l" => Some(Button::L),
            _ => None,
        }
    }
}

/// Active input state expressed as the host-side pressed mask.
///
/// One bit means pressed. [`InputState::mask`] is accepted directly by
/// [`crate::bus::Bus::set_pressed_keys`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputState {
    mask: u16,
}

impl InputState {
    #[must_use]
    pub const fn new() -> Self {
        Self { mask: 0 }
    }

    #[must_use]
    pub const fn from_mask(mask: u16) -> Self {
        Self {
            mask: mask & crate::bus::KEY_MASK,
        }
    }

    #[must_use]
    pub const fn mask(self) -> u16 {
        self.mask
    }

    #[must_use]
    pub const fn is_pressed(self, button: Button) -> bool {
        self.mask & button.bit() != 0
    }

    pub fn set(&mut self, button: Button, pressed: bool) {
        if pressed {
            self.mask |= button.bit();
        } else {
            self.mask &= !button.bit();
        }
    }

    pub fn clear(&mut self) {
        self.mask = 0;
    }

    /// Apply every host key code that has a binding. Unknown codes are ignored.
    pub fn apply_codes<'a, I>(&mut self, codes: I, bindings: &KeyBindings)
    where
        I: IntoIterator<Item = &'a str>,
    {
        self.mask = bindings.mask_for_codes(codes);
    }
}

/// Previous/current mask pair for host-driven press-edge detection.
///
/// The guest sees only [`InputState`]'s held mask. Edge state is host metadata
/// for a frontend's own repeat/just-pressed handling and is not guest state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputEdges {
    previous: u16,
    current: u16,
}

impl InputEdges {
    pub fn update(&mut self, next: u16) {
        self.previous = self.current;
        self.current = next & crate::bus::KEY_MASK;
    }

    #[must_use]
    pub const fn current(self) -> u16 {
        self.current
    }

    #[must_use]
    pub const fn just_pressed(self) -> u16 {
        self.current & !self.previous
    }

    #[must_use]
    pub const fn just_released(self) -> u16 {
        self.previous & !self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{KEY_A, KEY_B, KEY_START};

    #[test]
    fn state_sets_and_clears_documented_bits_only() {
        let mut state = InputState::new();
        state.set(Button::A, true);
        state.set(Button::Start, true);
        assert_eq!(state.mask(), KEY_A | KEY_START);
        assert!(state.is_pressed(Button::A));
        assert!(!state.is_pressed(Button::B));
        state.set(Button::A, false);
        assert_eq!(state.mask(), KEY_START);
        assert_eq!(InputState::from_mask(0xFFFF).mask(), 0x03FF);
        assert_eq!(InputState::from_mask(KEY_B).mask(), KEY_B);
        state.clear();
        assert_eq!(state.mask(), 0);
    }

    #[test]
    fn edges_report_just_pressed_and_released() {
        let mut edges = InputEdges::default();
        edges.update(KEY_A);
        assert_eq!(edges.just_pressed(), KEY_A);
        assert_eq!(edges.just_released(), 0);
        edges.update(KEY_A | KEY_B);
        assert_eq!(edges.just_pressed(), KEY_B);
        edges.update(0);
        assert_eq!(edges.just_pressed(), 0);
        assert_eq!(edges.just_released(), KEY_A | KEY_B);
        assert_eq!(edges.current(), 0);
    }

    #[test]
    fn apply_codes_uses_bindings() {
        let bindings = KeyBindings::default();
        let mut state = InputState::new();
        state.apply_codes(["KeyZ", "Enter", "NotAKey"], &bindings);
        assert_eq!(state.mask(), KEY_A | KEY_START);
        state.apply_codes(std::iter::empty(), &bindings);
        assert_eq!(state.mask(), 0);
    }
}
