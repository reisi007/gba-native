//! Platform-neutral key identities and the default GBA key bindings.
//!
//! Codes are the canonical `KeyboardEvent.code`/winit `KeyCode` spellings
//! (`KeyZ`, `ArrowUp`, `Enter`, ...). Both frontends normalise their native
//! event into these strings, so the persisted settings file is identical
//! across platforms.

use super::Button;

/// One host key identity. The label is for the settings page only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    code: &'static str,
    label: &'static str,
}

const KEYS: &[(&str, &str)] = &[
    ("KeyA", "A"),
    ("KeyB", "B"),
    ("KeyC", "C"),
    ("KeyD", "D"),
    ("KeyE", "E"),
    ("KeyF", "F"),
    ("KeyG", "G"),
    ("KeyH", "H"),
    ("KeyI", "I"),
    ("KeyJ", "J"),
    ("KeyK", "K"),
    ("KeyL", "L"),
    ("KeyM", "M"),
    ("KeyN", "N"),
    ("KeyO", "O"),
    ("KeyP", "P"),
    ("KeyQ", "Q"),
    ("KeyR", "R"),
    ("KeyS", "S"),
    ("KeyT", "T"),
    ("KeyU", "U"),
    ("KeyV", "V"),
    ("KeyW", "W"),
    ("KeyX", "X"),
    ("KeyY", "Y"),
    ("KeyZ", "Z"),
    ("Digit0", "0"),
    ("Digit1", "1"),
    ("Digit2", "2"),
    ("Digit3", "3"),
    ("Digit4", "4"),
    ("Digit5", "5"),
    ("Digit6", "6"),
    ("Digit7", "7"),
    ("Digit8", "8"),
    ("Digit9", "9"),
    ("ArrowUp", "Up"),
    ("ArrowDown", "Down"),
    ("ArrowLeft", "Left"),
    ("ArrowRight", "Right"),
    ("Enter", "Enter"),
    ("Space", "Space"),
    ("Backspace", "Backspace"),
    ("Tab", "Tab"),
    ("Escape", "Esc"),
    ("F11", "F11"),
    ("ShiftLeft", "L-Shift"),
    ("ShiftRight", "R-Shift"),
];

impl Key {
    /// Resolve a canonical code such as `KeyZ` or `ArrowUp`.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        KEYS.iter()
            .find(|(canonical, _)| *canonical == code)
            .map(|(code, label)| Self { code, label })
    }

    /// Resolve a browser `KeyboardEvent.key` value or a canonical code.
    ///
    /// Single printable characters map to the matching letter/digit key,
    /// `" "` maps to Space and `"Shift"` maps to the left Shift key.
    #[must_use]
    pub fn from_event_key(key: &str) -> Option<Self> {
        if let Some(found) = Self::from_code(key) {
            return Some(found);
        }
        match key {
            " " => return Self::from_code("Space"),
            "Shift" => return Self::from_code("ShiftLeft"),
            _ => {}
        }
        let mut chars = key.chars();
        let (first, rest) = (chars.next()?, chars.next());
        if rest.is_some() {
            return None;
        }
        if first.is_ascii_alphabetic() {
            let upper = first.to_ascii_uppercase();
            let mut buffer = [0u8; 4];
            let suffix = upper.encode_utf8(&mut buffer);
            return Self::from_code(&format!("Key{suffix}"));
        }
        if first.is_ascii_digit() {
            return Self::from_code(&format!("Digit{first}"));
        }
        None
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        self.code
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        self.label
    }
}

/// A rejected binding edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingError {
    Empty,
    Duplicate(Key),
}

/// One binding set for the ten documented buttons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyBindings {
    entries: Vec<(Button, Vec<Key>)>,
}

fn defaults() -> Vec<(Button, Vec<Key>)> {
    vec![
        (Button::A, keys(&["KeyZ", "KeyJ"])),
        (Button::B, keys(&["KeyX", "KeyK"])),
        (Button::Select, keys(&["Backspace"])),
        (Button::Start, keys(&["Enter"])),
        (Button::Right, keys(&["ArrowRight", "KeyD"])),
        (Button::Left, keys(&["ArrowLeft", "KeyA"])),
        (Button::Up, keys(&["ArrowUp", "KeyW"])),
        (Button::Down, keys(&["ArrowDown", "KeyS"])),
        (Button::R, keys(&["KeyE"])),
        (Button::L, keys(&["KeyQ"])),
    ]
}

fn keys(codes: &[&str]) -> Vec<Key> {
    codes
        .iter()
        .map(|code| Key::from_code(code).expect("static binding code"))
        .collect()
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            entries: defaults(),
        }
    }
}

impl KeyBindings {
    #[must_use]
    pub fn keys_for(&self, button: Button) -> &[Key] {
        self.entries
            .iter()
            .find(|(candidate, _)| *candidate == button)
            .map(|(_, keys)| keys.as_slice())
            .unwrap_or(&[])
    }

    #[must_use]
    pub fn entries(&self) -> &[(Button, Vec<Key>)] {
        &self.entries
    }

    /// Replace one button's keys. Empty sets and cross-button duplicates are
    /// rejected so a host key never fires two guest buttons.
    pub fn set(&mut self, button: Button, new_keys: Vec<Key>) -> Result<(), BindingError> {
        if new_keys.is_empty() {
            return Err(BindingError::Empty);
        }
        for (index, key) in new_keys.iter().enumerate() {
            if new_keys[..index].contains(key) {
                return Err(BindingError::Duplicate(*key));
            }
            let clash = self
                .entries
                .iter()
                .any(|(candidate, keys)| *candidate != button && keys.contains(key));
            if clash {
                return Err(BindingError::Duplicate(*key));
            }
        }
        if let Some((_, keys)) = self
            .entries
            .iter_mut()
            .find(|(candidate, _)| *candidate == button)
        {
            *keys = new_keys;
        } else {
            self.entries.push((button, new_keys));
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.entries = defaults();
    }

    #[must_use]
    pub fn mask_for_key(&self, key: Key) -> u16 {
        self.entries
            .iter()
            .filter(|(_, keys)| keys.contains(&key))
            .fold(0, |mask, (button, _)| mask | button.bit())
    }

    #[must_use]
    pub fn mask_for_codes<'a, I>(&self, codes: I) -> u16
    where
        I: IntoIterator<Item = &'a str>,
    {
        codes
            .into_iter()
            .filter_map(Key::from_code)
            .fold(0, |mask, key| mask | self.mask_for_key(key))
    }

    /// Find the first binding assigned to `key`, for the settings page.
    #[must_use]
    pub fn button_for_key(&self, key: Key) -> Option<Button> {
        self.entries
            .iter()
            .find(|(_, keys)| keys.contains(&key))
            .map(|(button, _)| *button)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings_cover_every_button_once() {
        let bindings = KeyBindings::default();
        for button in Button::ALL {
            assert!(
                !bindings.keys_for(button).is_empty(),
                "missing default for {}",
                button.name()
            );
        }
        let mut seen = Vec::new();
        for (_, keys) in bindings.entries() {
            for key in keys {
                assert!(!seen.contains(key), "duplicate default key {}", key.code());
                seen.push(*key);
            }
        }
    }

    #[test]
    fn codes_and_event_keys_resolve() {
        assert_eq!(Key::from_code("KeyZ").unwrap().label(), "Z");
        assert_eq!(Key::from_code("Nope"), None);
        assert_eq!(Key::from_event_key("z").unwrap().code(), "KeyZ");
        assert_eq!(Key::from_event_key(" ").unwrap().code(), "Space");
        assert_eq!(Key::from_event_key("Shift").unwrap().code(), "ShiftLeft");
        assert_eq!(Key::from_event_key("ArrowUp").unwrap().code(), "ArrowUp");
        assert_eq!(Key::from_event_key("!!"), None);
    }

    #[test]
    fn masks_follow_the_documented_keyinput_bits() {
        use crate::bus::{KEY_A, KEY_LEFT, KEY_START};
        let bindings = KeyBindings::default();
        let mask = bindings.mask_for_codes(["KeyZ", "ArrowLeft", "Enter"]);
        assert_eq!(mask, KEY_A | KEY_LEFT | KEY_START);
        assert_eq!(bindings.mask_for_codes(["unknown"]), 0);
        assert_eq!(
            bindings.mask_for_key(Key::from_code("KeyQ").unwrap()),
            1 << 9
        );
    }

    #[test]
    fn edits_reject_empty_and_duplicate_keys() {
        let mut bindings = KeyBindings::default();
        assert_eq!(bindings.set(Button::A, vec![]), Err(BindingError::Empty));
        let key = Key::from_code("KeyX").unwrap();
        assert_eq!(
            bindings.set(Button::A, vec![key]),
            Err(BindingError::Duplicate(key))
        );
        let custom = Key::from_code("Digit1").unwrap();
        bindings.set(Button::A, vec![custom]).unwrap();
        assert_eq!(bindings.keys_for(Button::A), &[custom]);
        assert_eq!(bindings.button_for_key(custom), Some(Button::A));
        bindings.reset();
        assert_eq!(
            bindings.keys_for(Button::A),
            &[
                Key::from_code("KeyZ").unwrap(),
                Key::from_code("KeyJ").unwrap()
            ]
        );
    }
}
