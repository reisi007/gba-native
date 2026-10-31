//! Host-testable Android shell model.
//!
//! The Android window, native buffer and event loop live in the platform
//! module; every decision that does not need a device lives here: composition
//! of the shared shell/fullscreen/settings screens, touch routing, persisted
//! settings and the shared tone artifact. Because the screens and the tone come
//! from `gba_native::frontend`, the Android app cannot drift from the other GUI
//! targets at the feature level.

use std::path::{Path, PathBuf};

use gba_native::frontend::{self, RgbaImage};
use gba_native::input::{Button, Settings};

/// Composition size shared with the desktop and browser shells.
pub const SCREEN_WIDTH: u32 = frontend::ui::WIDTH;
pub const SCREEN_HEIGHT: u32 = frontend::ui::HEIGHT;

/// Settings-page taps; the desktop reaches the same settings by key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    Close,
    ToggleMute,
    ToggleFullscreen,
    ToggleTouchControls,
    VolumeDown,
    VolumeUp,
}

/// Rectangles in composition pixels for the settings page controls.
const CLOSE_RECT: (u32, u32, u32, u32) = (470, 0, 330, 48);
/// Header band that opens the settings page; the desktop uses F1.
const SETTINGS_BUTTON_RECT: (u32, u32, u32, u32) = (600, 0, 200, 26);
const FULLSCREEN_RECT: (u32, u32, u32, u32) = (40, 455, 280, 40);
const TOUCH_RECT: (u32, u32, u32, u32) = (40, 493, 320, 40);
const MUTE_RECT: (u32, u32, u32, u32) = (470, 500, 140, 40);
const VOLUME_DOWN_RECT: (u32, u32, u32, u32) = (610, 530, 80, 60);
const VOLUME_UP_RECT: (u32, u32, u32, u32) = (690, 530, 100, 60);

fn inside(rect: (u32, u32, u32, u32), x: u32, y: u32) -> bool {
    x >= rect.0 && x < rect.0 + rect.2 && y >= rect.1 && y < rect.1 + rect.3
}

/// The Android application state without any window or device handle.
pub struct AppModel {
    settings: Settings,
    settings_path: PathBuf,
    show_settings: bool,
    pressed: u16,
    frame: RgbaImage,
}

impl AppModel {
    /// Load persisted settings and the shared synthetic demo frame.
    pub fn new(settings_path: PathBuf) -> Result<Self, String> {
        let (settings, warning) = frontend::settings::load(&settings_path);
        if let Some(warning) = warning {
            eprintln!("settings: {warning}");
        }
        let frame = frontend::demo_image().map_err(|error| error.to_string())?;
        Ok(Self {
            settings,
            settings_path,
            show_settings: false,
            pressed: 0,
            frame,
        })
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn show_settings(&self) -> bool {
        self.show_settings
    }

    pub fn pressed(&self) -> u16 {
        self.pressed
    }

    pub fn frame(&self) -> &RgbaImage {
        &self.frame
    }

    /// Compose the current screen exactly as the desktop shell does.
    pub fn compose(&self) -> RgbaImage {
        if self.show_settings {
            frontend::ui::settings_page(&self.settings)
        } else if self.settings.fullscreen {
            frontend::ui::fullscreen(&self.frame)
        } else {
            frontend::ui::shell(&self.frame, self.pressed)
        }
    }

    /// Press a normalised composition coordinate against the shared controls.
    pub fn touch_down(&mut self, x: f32, y: f32) -> Option<Button> {
        if self.show_settings {
            return None;
        }
        let button = frontend::ui::hit_test(x, y)?;
        self.pressed |= button.bit();
        Some(button)
    }

    /// Release a normalised composition coordinate against the shared controls.
    pub fn touch_up(&mut self, x: f32, y: f32) -> Option<Button> {
        if self.show_settings {
            return None;
        }
        let button = frontend::ui::hit_test(x, y)?;
        self.pressed &= !button.bit();
        Some(button)
    }

    /// Clear every pressed bit, for a cancelled gesture.
    pub fn clear_pressed(&mut self) {
        self.pressed = 0;
    }

    pub fn toggle_settings(&mut self, show: bool) {
        self.show_settings = show;
    }

    /// True for the header band that opens the settings page.
    pub fn header_hit(&self, x: u32, y: u32) -> bool {
        !self.show_settings && inside(SETTINGS_BUTTON_RECT, x, y)
    }

    /// Resolve a settings-page tap in composition pixels.
    pub fn settings_action(&self, x: u32, y: u32) -> Option<SettingsAction> {
        if !self.show_settings {
            return None;
        }
        if inside(CLOSE_RECT, x, y) {
            Some(SettingsAction::Close)
        } else if inside(FULLSCREEN_RECT, x, y) {
            Some(SettingsAction::ToggleFullscreen)
        } else if inside(TOUCH_RECT, x, y) {
            Some(SettingsAction::ToggleTouchControls)
        } else if inside(MUTE_RECT, x, y) {
            Some(SettingsAction::ToggleMute)
        } else if inside(VOLUME_DOWN_RECT, x, y) {
            Some(SettingsAction::VolumeDown)
        } else if inside(VOLUME_UP_RECT, x, y) {
            Some(SettingsAction::VolumeUp)
        } else {
            None
        }
    }

    /// Apply a settings action and persist it through the shared settings file.
    pub fn apply_settings_action(&mut self, action: SettingsAction) -> std::io::Result<()> {
        match action {
            SettingsAction::Close => self.show_settings = false,
            SettingsAction::ToggleMute => self.settings.muted = !self.settings.muted,
            SettingsAction::ToggleFullscreen => {
                self.settings.fullscreen = !self.settings.fullscreen;
            }
            SettingsAction::ToggleTouchControls => {
                self.settings.touch_controls = !self.settings.touch_controls;
            }
            SettingsAction::VolumeDown => {
                self.settings.master_volume = self.settings.master_volume.saturating_sub(10);
            }
            SettingsAction::VolumeUp => {
                self.settings.master_volume =
                    self.settings.master_volume.saturating_add(10).min(100);
            }
        }
        self.persist()
    }

    pub fn persist(&self) -> std::io::Result<()> {
        frontend::settings::save(&self.settings_path, &self.settings)
    }

    /// Write the shared tone WAV next to the settings file.
    pub fn write_tone_artifact(&self, directory: &Path) -> std::io::Result<PathBuf> {
        let path = directory.join("gba-native-tone.wav");
        frontend::tone::write_tone(&path, &self.settings)?;
        Ok(path)
    }
}

/// Normalise a window coordinate into composition space.
pub fn locate(surface: (u32, u32), x: f32, y: f32) -> Option<(f32, f32)> {
    let viewport = frontend::compute_viewport(SCREEN_WIDTH, SCREEN_HEIGHT, surface.0, surface.1);
    if viewport.width == 0 || viewport.height == 0 {
        return None;
    }
    let left = viewport.x as f32;
    let top = viewport.y as f32;
    if x < left || y < top || x >= left + viewport.width as f32 || y >= top + viewport.height as f32
    {
        return None;
    }
    let nx = (x - left) / viewport.width as f32;
    let ny = (y - top) / viewport.height as f32;
    Some((nx, ny))
}

/// Convert a normalised composition coordinate into composition pixels.
pub fn composition_pixel(x: f32, y: f32) -> (u32, u32) {
    let px = (x * SCREEN_WIDTH as f32) as u32;
    let py = (y * SCREEN_HEIGHT as f32) as u32;
    (px.min(SCREEN_WIDTH - 1), py.min(SCREEN_HEIGHT - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gba_native::input::TouchLayout;
    use gba_native::sha256;

    fn temp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gba-native-android-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or(0)
        ))
    }

    fn model(tag: &str) -> (AppModel, PathBuf) {
        let directory = temp(tag);
        std::fs::create_dir_all(&directory).expect("temp dir");
        let path = directory.join("settings.ini");
        (AppModel::new(path).expect("model"), directory)
    }

    #[test]
    fn demo_frame_matches_the_shared_frontend_digest() {
        let (model, directory) = model("demo");
        let shared = frontend::demo_image().expect("shared demo");
        assert_eq!(model.frame().width(), shared.width());
        assert_eq!(model.frame().height(), shared.height());
        assert_eq!(model.frame().rgb_sha256(), shared.rgb_sha256());
        assert_eq!(model.frame().rgba(), shared.rgba());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn shell_and_fullscreen_use_the_shared_screens() {
        let (mut model, directory) = model("screens");
        let expected = frontend::ui::shell(model.frame(), 0);
        assert_eq!(model.compose().rgba(), expected.rgba());
        assert_eq!(model.compose().rgb_sha256(), expected.rgb_sha256());

        model.settings.fullscreen = true;
        let expected = frontend::ui::fullscreen(model.frame());
        assert_eq!(model.compose().rgba(), expected.rgba());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn every_touch_button_is_reachable_and_released() {
        let (mut model, directory) = model("touch");
        let layout = TouchLayout::landscape();
        for button in Button::ALL {
            let (x, y, width, height) = layout
                .button_pixels(button, SCREEN_WIDTH, SCREEN_HEIGHT)
                .expect("button rect");
            let nx = (x as f32 + width as f32 / 2.0) / SCREEN_WIDTH as f32;
            let ny = (y as f32 + height as f32 / 2.0) / SCREEN_HEIGHT as f32;
            assert_eq!(model.touch_down(nx, ny), Some(button));
            assert_eq!(model.pressed() & button.bit(), button.bit());
            assert_eq!(model.touch_up(nx, ny), Some(button));
            assert_eq!(model.pressed() & button.bit(), 0);
        }
        assert_eq!(model.pressed(), 0);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn taps_on_the_guest_screen_produce_no_button() {
        let (mut model, directory) = model("screen-taps");
        let layout = TouchLayout::landscape();
        let (sx, sy, sw, sh) = layout.screen.pixels(SCREEN_WIDTH, SCREEN_HEIGHT);
        let nx = (sx as f32 + sw as f32 / 2.0) / SCREEN_WIDTH as f32;
        let ny = (sy as f32 + sh as f32 / 2.0) / SCREEN_HEIGHT as f32;
        assert_eq!(model.touch_down(nx, ny), None);
        assert_eq!(model.pressed(), 0);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn settings_taps_change_state_and_persist_through_the_shared_file() {
        let (mut model, directory) = model("settings");
        model.toggle_settings(true);
        assert_eq!(
            model.settings_action(500, 520),
            Some(SettingsAction::ToggleMute)
        );
        model
            .apply_settings_action(SettingsAction::ToggleMute)
            .expect("mute");
        assert!(model.settings().muted);

        let before = model.settings().master_volume;
        model
            .apply_settings_action(SettingsAction::VolumeUp)
            .expect("volume up");
        assert_eq!(model.settings().master_volume, (before + 10).min(100));
        model
            .apply_settings_action(SettingsAction::VolumeDown)
            .expect("volume down");
        assert_eq!(model.settings().master_volume, before);

        model
            .apply_settings_action(SettingsAction::ToggleFullscreen)
            .expect("fullscreen");
        model
            .apply_settings_action(SettingsAction::ToggleTouchControls)
            .expect("touch");
        model
            .apply_settings_action(SettingsAction::Close)
            .expect("close");
        assert!(!model.show_settings());

        let (reloaded, warning) = frontend::settings::load(&model.settings_path);
        assert!(warning.is_none());
        assert_eq!(reloaded, *model.settings());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn header_band_opens_the_settings_page() {
        let (mut model, directory) = model("header");
        assert!(model.header_hit(700, 10));
        assert!(!model.header_hit(10, 10));
        assert!(!model.header_hit(700, 100));
        model.toggle_settings(true);
        assert!(!model.header_hit(700, 10));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn every_settings_control_has_a_tap_zone() {
        let actions = [
            (CLOSE_RECT, SettingsAction::Close),
            (FULLSCREEN_RECT, SettingsAction::ToggleFullscreen),
            (TOUCH_RECT, SettingsAction::ToggleTouchControls),
            (MUTE_RECT, SettingsAction::ToggleMute),
            (VOLUME_DOWN_RECT, SettingsAction::VolumeDown),
            (VOLUME_UP_RECT, SettingsAction::VolumeUp),
        ];
        let (mut model, directory) = model("zones");
        model.toggle_settings(true);
        for (rect, expected) in actions {
            let x = rect.0 + rect.2 / 2;
            let y = rect.1 + rect.3 / 2;
            assert_eq!(model.settings_action(x, y), Some(expected));
        }
        assert_eq!(model.settings_action(1, 599), None);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn window_coordinates_map_through_the_shared_letterbox() {
        // 1600x1200 is exactly 2x the 800x600 composition.
        assert_eq!(locate((1600, 1200), 0.0, 0.0), Some((0.0, 0.0)));
        assert_eq!(locate((1600, 1200), 800.0, 600.0), Some((0.5, 0.5)));
        // A 2000x1200 surface letterboxes by 200 pixels on both sides.
        assert_eq!(locate((2000, 1200), 199.0, 0.0), None);
        let (nx, _) = locate((2000, 1200), 200.0, 0.0).expect("inside");
        assert!(nx.abs() < 1e-6);
        assert_eq!(locate((0, 0), 0.0, 0.0), None);
    }

    #[test]
    fn tone_artifact_is_identical_to_the_shared_tone() {
        let (model, directory) = model("tone");
        let path = model.write_tone_artifact(&directory).expect("tone");
        let written = std::fs::read(&path).expect("read tone");
        let shared = frontend::tone::tone_wav(model.settings());
        assert_eq!(
            sha256::hex(&sha256::digest(&written)),
            sha256::hex(&sha256::digest(&shared))
        );
        assert_eq!(written, shared);
        let _ = std::fs::remove_dir_all(directory);
    }
}
