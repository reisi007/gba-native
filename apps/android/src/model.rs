//! Host-testable Android shell model.
//!
//! The Android window, native buffer and event loop live in the platform
//! module; every decision that does not need a device lives here: composition
//! of the shared shell/fullscreen/settings/saves screens, touch routing,
//! persisted settings and the shared tone artifact. Because the screens, the
//! save slots and the tone come from `gba_native::frontend`, the Android app
//! cannot drift from the other GUI targets at the feature level.

use std::path::{Path, PathBuf};

use gba_native::frontend::ui::{self, SavesAction};
use gba_native::frontend::{self, RgbaImage, SaveSlot, SaveSlots};
use gba_native::input::{Button, Settings};

/// Composition size shared with the desktop and browser shells.
pub const SCREEN_WIDTH: u32 = ui::WIDTH;
pub const SCREEN_HEIGHT: u32 = ui::HEIGHT;

/// Which shared page the app is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Shell,
    Settings,
    Saves,
}

/// Settings-page taps; the desktop reaches the same settings by key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    Close,
    OpenSaves,
    ToggleMute,
    ToggleFullscreen,
    ToggleTouchControls,
    VolumeDown,
    VolumeUp,
}

/// Rectangles in composition pixels for the settings page controls.
const FULLSCREEN_RECT: (u32, u32, u32, u32) = (40, 455, 280, 40);
const TOUCH_RECT: (u32, u32, u32, u32) = (40, 493, 320, 40);
const MUTE_RECT: (u32, u32, u32, u32) = (470, 500, 140, 40);
const VOLUME_DOWN_RECT: (u32, u32, u32, u32) = (610, 530, 80, 60);
const VOLUME_UP_RECT: (u32, u32, u32, u32) = (690, 530, 100, 60);
/// Header band that opens the settings page; the desktop uses F1.
const SETTINGS_BUTTON_RECT: (u32, u32, u32, u32) = (600, 0, 200, 26);

fn inside(rect: (u32, u32, u32, u32), x: u32, y: u32) -> bool {
    x >= rect.0 && x < rect.0 + rect.2 && y >= rect.1 && y < rect.1 + rect.3
}

/// The Android application state without any window or device handle.
pub struct AppModel {
    settings: Settings,
    settings_path: PathBuf,
    page: Page,
    pressed: u16,
    frame: RgbaImage,
    slots: SaveSlots,
    slot_list: Vec<SaveSlot>,
    selected: usize,
    status: Option<String>,
}

impl AppModel {
    /// Load persisted settings, save slots and the shared demo frame.
    pub fn new(settings_path: PathBuf) -> Result<Self, String> {
        let (settings, warning) = frontend::settings::load(&settings_path);
        if let Some(warning) = warning {
            eprintln!("settings: {warning}");
        }
        let frame = frontend::demo_image().map_err(|error| error.to_string())?;
        let saves_root = settings_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("saves");
        let mut model = Self {
            settings,
            settings_path,
            page: Page::Shell,
            pressed: 0,
            frame,
            slots: SaveSlots::new(saves_root),
            slot_list: Vec::new(),
            selected: 0,
            status: None,
        };
        model.reload_slots();
        Ok(model)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn page(&self) -> Page {
        self.page
    }

    pub fn show_settings(&self) -> bool {
        self.page == Page::Settings
    }

    pub fn pressed(&self) -> u16 {
        self.pressed
    }

    pub fn frame(&self) -> &RgbaImage {
        &self.frame
    }

    pub fn slots(&self) -> &[SaveSlot] {
        &self.slot_list
    }

    pub fn selected_slot(&self) -> usize {
        self.selected
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Compose the current screen exactly as the desktop shell does.
    pub fn compose(&self) -> RgbaImage {
        match self.page {
            Page::Saves => ui::saves_page(&self.slot_list, self.selected),
            Page::Settings => ui::settings_page(&self.settings),
            Page::Shell if self.settings.fullscreen => ui::fullscreen(&self.frame),
            Page::Shell => ui::shell(&self.frame, self.pressed),
        }
    }

    /// Press a normalised composition coordinate against the shared controls.
    pub fn touch_down(&mut self, x: f32, y: f32) -> Option<Button> {
        if self.page != Page::Shell {
            return None;
        }
        let button = ui::hit_test(x, y)?;
        self.pressed |= button.bit();
        Some(button)
    }

    /// Release a normalised composition coordinate against the shared controls.
    pub fn touch_up(&mut self, x: f32, y: f32) -> Option<Button> {
        if self.page != Page::Shell {
            return None;
        }
        let button = ui::hit_test(x, y)?;
        self.pressed &= !button.bit();
        Some(button)
    }

    /// Clear every pressed bit, for a cancelled gesture.
    pub fn clear_pressed(&mut self) {
        self.pressed = 0;
    }

    pub fn show_page(&mut self, page: Page) {
        self.page = page;
    }

    pub fn toggle_settings(&mut self, show: bool) {
        self.page = if show { Page::Settings } else { Page::Shell };
    }

    /// True for the header band that opens the settings page.
    pub fn header_hit(&self, x: u32, y: u32) -> bool {
        self.page == Page::Shell && inside(SETTINGS_BUTTON_RECT, x, y)
    }

    /// Resolve a settings-page tap in composition pixels.
    pub fn settings_action(&self, x: u32, y: u32) -> Option<SettingsAction> {
        if self.page != Page::Settings {
            return None;
        }
        if ui::settings_saves_at(x, y) {
            Some(SettingsAction::OpenSaves)
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
        } else if x >= 640 && y < 48 {
            Some(SettingsAction::Close)
        } else {
            None
        }
    }

    /// Apply a settings action and persist it through the shared settings file.
    pub fn apply_settings_action(&mut self, action: SettingsAction) -> std::io::Result<()> {
        match action {
            SettingsAction::Close => self.page = Page::Shell,
            SettingsAction::OpenSaves => {
                self.reload_slots();
                self.page = Page::Saves;
            }
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

    /// Resolve a saves-page tap to a row selection or an action.
    pub fn saves_row(&mut self, x: u32, y: u32) -> Option<usize> {
        if self.page != Page::Saves {
            return None;
        }
        let row = ui::saves_row_at(x, y)?;
        if row < self.slot_list.len() {
            self.selected = row;
            Some(row)
        } else {
            None
        }
    }

    /// Apply a saves-page action through the shared slot library.
    pub fn apply_saves_action(&mut self, action: SavesAction) -> std::io::Result<()> {
        match action {
            SavesAction::Close => self.page = Page::Settings,
            SavesAction::New => {
                let seed = self.slot_list.len() as u8;
                let name = format!("slot{}", self.slot_list.len() + 1);
                let result = self.slots.store(&name, &SaveSlots::synthetic_image(seed));
                if let Err(error) = result {
                    self.status = Some(format!("cannot create {name}: {error}"));
                }
            }
            SavesAction::Use | SavesAction::MakeDefault => {
                if let Some(slot) = self.slot_list.get(self.selected) {
                    if let Err(error) = self.slots.set_default(&slot.name) {
                        self.status = Some(format!("cannot select {}: {error}", slot.name));
                    } else if action == SavesAction::Use {
                        self.page = Page::Shell;
                    }
                }
            }
            SavesAction::Delete => {
                if let Some(slot) = self.slot_list.get(self.selected) {
                    if let Err(error) = self.slots.delete(&slot.name) {
                        self.status = Some(format!("cannot delete {}: {error}", slot.name));
                    }
                }
            }
        }
        self.reload_slots();
        Ok(())
    }

    fn reload_slots(&mut self) {
        match self.slots.slots() {
            Ok(slots) => {
                self.slot_list = slots;
                if self.selected >= self.slot_list.len() {
                    self.selected = self.slot_list.len().saturating_sub(1);
                }
            }
            Err(error) => {
                self.slot_list.clear();
                self.status = Some(format!("cannot list save slots: {error}"));
            }
        }
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
#[path = "model_tests.rs"]
mod tests;
