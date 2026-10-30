//! Scripted, instruction-indexed input for bounded runs and local tests.
//!
//! A script is a small text log of `at:KEYS:hold` lines:
//!
//! ```text
//! # press START at guest instruction 460000000 for 20000000 instructions
//! 460000000:START:20000000
//! 700000000:A:15000000
//! ```
//!
//! `KEYS` is `+`-separated (`A+B`) and uses the documented button names. Input
//! is applied between instructions. This is deterministic replay, not
//! frame-synchronous sampling or hardware timing.

use std::fmt;

use super::Button;
use crate::audio::AudioEngine;
use crate::bus::Bus;
use crate::cpu::Cpu;
use crate::error::Result;
use crate::interpret;

/// Instruction chunk used to sample sound registers into the audio engine.
pub const AUDIO_CHUNK_INSTRUCTIONS: u64 = 1024;

/// One held-key interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub at: u64,
    /// Pressed mask, already restricted to the documented KEYINPUT bits.
    pub keys: u16,
    pub hold: u64,
}

impl InputEvent {
    #[must_use]
    pub const fn end(self) -> u64 {
        self.at.saturating_add(self.hold)
    }
}

/// Parse errors for an input script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError(String);

impl ScriptError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScriptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScriptError {}

fn parse_count(number: usize, what: &str, value: &str) -> std::result::Result<u64, ScriptError> {
    value.parse().map_err(|_| {
        ScriptError::new(format!(
            "line {number}: invalid {what} {value:?} (expected an unsigned integer)"
        ))
    })
}

/// An ordered set of held-key intervals.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputScript {
    events: Vec<InputEvent>,
}

impl InputScript {
    #[must_use]
    pub fn from_events(mut events: Vec<InputEvent>) -> Self {
        events.sort_by_key(|event| event.at);
        Self { events }
    }

    /// Parse the line-oriented script format.
    pub fn parse(text: &str) -> std::result::Result<Self, ScriptError> {
        let mut events = Vec::new();
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let number = index + 1;
            let mut fields = line.split(':');
            let at = fields.next().unwrap_or_default();
            let keys = fields.next();
            let hold = fields.next();
            if fields.next().is_some() || keys.is_none() || hold.is_none() {
                return Err(ScriptError::new(format!(
                    "line {number}: expected at:KEYS:hold"
                )));
            }
            let at = parse_count(number, "instruction index", at)?;
            let hold = parse_count(number, "hold length", hold.unwrap_or_default())?;
            if hold == 0 {
                return Err(ScriptError::new(format!(
                    "line {number}: hold must be at least one instruction"
                )));
            }
            if at.checked_add(hold).is_none() {
                return Err(ScriptError::new(format!(
                    "line {number}: instruction index plus hold overflows"
                )));
            }
            let mut mask = 0u16;
            let key_field = keys.unwrap_or_default();
            if key_field.is_empty() {
                return Err(ScriptError::new(format!(
                    "line {number}: at least one button is required"
                )));
            }
            for name in key_field.split('+') {
                let button = Button::parse(name.trim()).ok_or_else(|| {
                    ScriptError::new(format!("line {number}: unknown button {name:?}"))
                })?;
                mask |= button.bit();
            }
            if mask == 0 {
                return Err(ScriptError::new(format!(
                    "line {number}: at least one button is required"
                )));
            }
            events.push(InputEvent {
                at,
                keys: mask,
                hold,
            });
        }
        Ok(Self::from_events(events))
    }

    #[must_use]
    pub fn events(&self) -> &[InputEvent] {
        &self.events
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Held mask at one instruction index. Overlapping events resolve to the
    /// latest one that started; a released event falls back to no keys.
    #[must_use]
    pub fn mask_at(&self, instruction: u64) -> u16 {
        self.events
            .iter()
            .rev()
            .find(|event| instruction >= event.at && instruction < event.end())
            .map_or(0, |event| event.keys)
    }
}

/// Run the reference interpreter with this script's input schedule.
///
/// The bus is left with no pressed keys so the final state digest does not
/// depend on whether the last scripted interval was still open.
pub fn run_scripted(
    cpu: &mut Cpu,
    bus: &mut Bus,
    budget: u64,
    script: &InputScript,
) -> Result<u64> {
    drive_with(cpu, bus, budget, script, |_, _| {})
}

/// Run the reference interpreter and feed sound-register snapshots to a
/// stateful mixer every [`AUDIO_CHUNK_INSTRUCTIONS`] instructions.
pub fn run_scripted_audio(
    cpu: &mut Cpu,
    bus: &mut Bus,
    budget: u64,
    script: &InputScript,
    engine: &mut AudioEngine,
) -> Result<u64> {
    drive_with(cpu, bus, budget, script, |bus, instructions| {
        engine.advance(&bus.sound_registers(), instructions);
    })
}

fn drive_with<F>(
    cpu: &mut Cpu,
    bus: &mut Bus,
    budget: u64,
    script: &InputScript,
    mut observe: F,
) -> Result<u64>
where
    F: FnMut(&Bus, u64),
{
    let mut used = 0u64;
    let mut next = 0usize;
    let mut keys = 0u16;
    let mut until = 0u64;
    let mut chunk = 0u64;
    while used < budget {
        while let Some(event) = script.events.get(next) {
            if event.at > used {
                break;
            }
            keys = event.keys;
            until = event.end();
            next += 1;
        }
        if keys != 0 && used >= until {
            keys = 0;
        }
        bus.set_pressed_keys(keys);
        interpret::deliver_pending_irq(cpu, bus)?;
        interpret::step(cpu, bus)?;
        bus.tick_frame();
        used += 1;
        chunk += 1;
        if chunk == AUDIO_CHUNK_INSTRUCTIONS {
            observe(bus, chunk);
            chunk = 0;
        }
    }
    if chunk > 0 {
        observe(bus, chunk);
    }
    bus.set_pressed_keys(0);
    Ok(used)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{KEY_A, KEY_START};

    #[test]
    fn parses_events_comments_and_combined_buttons() {
        let script =
            InputScript::parse("# comment\n\n460000000:START:20000000\n700000000:A+START:5\n")
                .unwrap();
        assert_eq!(script.events().len(), 2);
        assert_eq!(script.events()[0].keys, KEY_START);
        assert_eq!(script.events()[1].keys, KEY_A | KEY_START);
        assert_eq!(script.events()[0].end(), 480000000);
    }

    #[test]
    fn mask_at_respects_start_and_hold() {
        let script = InputScript::parse("10:A:5\n20:START:3\n").unwrap();
        assert_eq!(script.mask_at(9), 0);
        assert_eq!(script.mask_at(10), KEY_A);
        assert_eq!(script.mask_at(14), KEY_A);
        assert_eq!(script.mask_at(15), 0);
        assert_eq!(script.mask_at(20), KEY_START);
        assert_eq!(script.mask_at(23), 0);
    }

    #[test]
    fn invalid_scripts_are_rejected() {
        assert!(
            InputScript::parse("1:A")
                .unwrap_err()
                .message()
                .contains("expected")
        );
        assert!(
            InputScript::parse("x:A:1")
                .unwrap_err()
                .message()
                .contains("instruction index")
        );
        assert!(
            InputScript::parse("1:A:1.5")
                .unwrap_err()
                .message()
                .contains("hold length")
        );
        assert!(
            InputScript::parse("1:A:0")
                .unwrap_err()
                .message()
                .contains("at least one")
        );
        assert!(
            InputScript::parse("1:Z:1")
                .unwrap_err()
                .message()
                .contains("unknown button")
        );
        assert!(
            InputScript::parse("1::1")
                .unwrap_err()
                .message()
                .contains("at least one")
        );
        assert!(
            InputScript::parse("18446744073709551615:A:2")
                .unwrap_err()
                .message()
                .contains("overflows")
        );
    }

    #[test]
    fn empty_script_is_a_no_op() {
        let script = InputScript::parse("").unwrap();
        assert!(script.is_empty());
        assert_eq!(script.mask_at(0), 0);
    }
}
