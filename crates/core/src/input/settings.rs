//! Persisted host settings: key bindings, fullscreen and touch preferences.
//!
//! The representation is a small ordered `key=value` text format with no paths,
//! timestamps or platform data. The desktop frontend persists this document;
//! the browser frontend stores the same fields inside a JSON `localStorage`
//! payload and normalises them into the same model. Parsing is strict about
//! known fields and ignores comments/blank lines.

use std::fmt;

use super::{Button, Key, KeyBindings};
use crate::audio::AudioSettings;

pub const SETTINGS_SCHEMA: u32 = 1;

/// A rejected settings document or value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsError(String);

impl SettingsError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SettingsError {}

/// Host preferences that survive a restart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub bindings: KeyBindings,
    pub fullscreen: bool,
    pub touch_controls: bool,
    pub integer_scaling: bool,
    /// Host audio volume in percent (0..=100).
    pub master_volume: u8,
    pub muted: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            bindings: KeyBindings::default(),
            fullscreen: false,
            touch_controls: true,
            integer_scaling: true,
            master_volume: AudioSettings::default().master_volume,
            muted: false,
        }
    }
}

impl Settings {
    /// Parse a settings document. Missing fields keep their default values.
    pub fn parse(text: &str) -> Result<Self, SettingsError> {
        let mut settings = Self::default();
        let mut schema_seen = false;
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let number = index + 1;
            let (name, value) = line.split_once('=').ok_or_else(|| {
                SettingsError::new(format!("line {number}: expected key=value, got {line:?}"))
            })?;
            let name = name.trim();
            let value = value.trim();
            match name {
                "schema" => {
                    let parsed: u32 = value.parse().map_err(|_| {
                        SettingsError::new(format!("line {number}: invalid schema {value:?}"))
                    })?;
                    if parsed != SETTINGS_SCHEMA {
                        return Err(SettingsError::new(format!(
                            "line {number}: unsupported schema {parsed}"
                        )));
                    }
                    schema_seen = true;
                }
                "fullscreen" => settings.fullscreen = parse_bool(number, value)?,
                "touch_controls" => settings.touch_controls = parse_bool(number, value)?,
                "integer_scaling" => settings.integer_scaling = parse_bool(number, value)?,
                "muted" => settings.muted = parse_bool(number, value)?,
                "master_volume" => {
                    let volume: u8 = value.parse().map_err(|_| {
                        SettingsError::new(format!(
                            "line {number}: invalid master_volume {value:?}"
                        ))
                    })?;
                    if volume > 100 {
                        return Err(SettingsError::new(format!(
                            "line {number}: master_volume must be 0..=100"
                        )));
                    }
                    settings.master_volume = volume;
                }
                other => {
                    let Some(button_name) = other.strip_prefix("key.") else {
                        return Err(SettingsError::new(format!(
                            "line {number}: unknown field {other:?}"
                        )));
                    };
                    let button = Button::parse(button_name).ok_or_else(|| {
                        SettingsError::new(format!("line {number}: unknown button {button_name:?}"))
                    })?;
                    let keys = parse_keys(number, value)?;
                    settings
                        .bindings
                        .set(button, keys)
                        .map_err(|error| SettingsError::new(format!("line {number}: {error:?}")))?;
                }
            }
        }
        if !schema_seen
            && text
                .lines()
                .any(|line| line.trim_start().starts_with("schema"))
        {
            return Err(SettingsError::new("schema field could not be parsed"));
        }
        Ok(settings)
    }

    /// Render the canonical document. Field order is stable and tested.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("schema={SETTINGS_SCHEMA}\n"));
        out.push_str(&format!("fullscreen={}\n", self.fullscreen));
        out.push_str(&format!("touch_controls={}\n", self.touch_controls));
        out.push_str(&format!("integer_scaling={}\n", self.integer_scaling));
        out.push_str(&format!("master_volume={}\n", self.master_volume));
        out.push_str(&format!("muted={}\n", self.muted));
        for (button, keys) in self.bindings.entries() {
            let codes: Vec<&str> = keys.iter().map(|key| key.code()).collect();
            out.push_str(&format!("key.{}={}\n", button.name(), codes.join(",")));
        }
        out
    }

    /// Host audio settings derived from this document.
    #[must_use]
    pub fn audio(&self) -> AudioSettings {
        AudioSettings::new(self.master_volume, self.muted)
    }

    /// Rows for the settings page: button name plus its human-readable keys.
    #[must_use]
    pub fn binding_rows(&self) -> Vec<(Button, Vec<&'static str>)> {
        self.bindings
            .entries()
            .iter()
            .map(|(button, keys)| (*button, keys.iter().map(|key| key.label()).collect()))
            .collect()
    }
}

fn parse_bool(number: usize, value: &str) -> Result<bool, SettingsError> {
    match value {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(SettingsError::new(format!(
            "line {number}: expected true or false, got {value:?}"
        ))),
    }
}

fn parse_keys(number: usize, value: &str) -> Result<Vec<Key>, SettingsError> {
    let mut keys = Vec::new();
    for code in value
        .split(',')
        .map(str::trim)
        .filter(|code| !code.is_empty())
    {
        let key = Key::from_code(code).ok_or_else(|| {
            SettingsError::new(format!("line {number}: unknown key code {code:?}"))
        })?;
        keys.push(key);
    }
    if keys.is_empty() {
        return Err(SettingsError::new(format!(
            "line {number}: a button needs at least one key"
        )));
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_render_and_round_trip() {
        let settings = Settings::default();
        let text = settings.render();
        assert!(text.starts_with("schema=1\nfullscreen=false\n"));
        assert!(text.contains("master_volume=80\n"));
        assert!(text.contains("muted=false\n"));
        assert!(text.contains("key.A=KeyZ,KeyJ\n"));
        assert!(text.contains("key.Start=Enter\n"));
        assert_eq!(Settings::parse(&text).unwrap(), settings);
    }

    #[test]
    fn missing_fields_keep_defaults_and_comments_are_ignored() {
        let settings = Settings::parse("# host prefs\n\nfullscreen=true\n").unwrap();
        assert!(settings.fullscreen);
        assert_eq!(settings.bindings, KeyBindings::default());
        assert_eq!(Settings::parse("").unwrap(), Settings::default());
    }

    #[test]
    fn invalid_values_are_rejected_with_line_numbers() {
        let error = Settings::parse("schema=2\n").unwrap_err();
        assert!(error.message().contains("unsupported schema"));
        let error = Settings::parse("fullscreen=maybe\n").unwrap_err();
        assert!(error.message().contains("line 1"));
        let error = Settings::parse("key.Z=KeyA\n").unwrap_err();
        assert!(error.message().contains("unknown button"));
        let error = Settings::parse("key.A=NotAKey\n").unwrap_err();
        assert!(error.message().contains("unknown key code"));
        let error = Settings::parse("no-equals\n").unwrap_err();
        assert!(error.message().contains("expected key=value"));
    }

    #[test]
    fn binding_rows_match_the_binding_table() {
        let rows = Settings::default().binding_rows();
        assert_eq!(rows.len(), 10);
        assert_eq!(rows[0].0, Button::A);
        assert_eq!(rows[0].1, vec!["Z", "J"]);
        assert_eq!(rows[3].0, Button::Start);
        assert_eq!(rows[3].1, vec!["Enter"]);
    }

    #[test]
    fn audio_volume_and_mute_round_trip_and_validate() {
        let settings = Settings {
            master_volume: 37,
            muted: true,
            ..Settings::default()
        };
        let parsed = Settings::parse(&settings.render()).unwrap();
        assert_eq!(parsed, settings);
        assert_eq!(parsed.audio().master_volume, 37);
        assert!(parsed.audio().muted);
        assert_eq!(parsed.audio().gain(), 0.0);
        let error = Settings::parse("master_volume=101\n").unwrap_err();
        assert!(error.message().contains("0..=100"));
        let error = Settings::parse("master_volume=loud\n").unwrap_err();
        assert!(error.message().contains("invalid master_volume"));
    }

    #[test]
    fn rebinding_a_button_persists() {
        let mut settings = Settings::default();
        let key = Key::from_code("Digit1").unwrap();
        settings.bindings.set(Button::A, vec![key]).unwrap();
        let text = settings.render();
        let parsed = Settings::parse(&text).unwrap();
        assert_eq!(parsed.bindings.keys_for(Button::A), &[key]);
        assert_eq!(parsed.bindings.keys_for(Button::B).len(), 2);
    }
}
