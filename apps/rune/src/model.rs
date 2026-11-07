//! Platform-independent state for the Rune snapshot app.
//!
//! This model reuses the core's synthetic image, shell/settings composition,
//! settings document and host-input types. It has no platform paths, window or
//! audio-device operations. Input is host metadata only: the built-in demo is
//! a static snapshot and does not execute in response to these input changes.

use gba_native::frontend::{self, image::RgbaImage, ui};
use gba_native::input::{Button, InputEdges, InputState, Key, Settings, SettingsError};

/// Which Rune page the host should present.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    /// Core-produced synthetic demo inside the shared shell composition.
    #[default]
    Demo,
    /// Shared core settings-page composition.
    Settings,
    /// Host-side synthetic save-slot manager.
    Saves,
}

/// App state that is independent of a particular Rune scene or platform.
#[derive(Debug)]
pub struct RuneAppModel {
    demo: Result<RgbaImage, String>,
    settings: Settings,
    page: Page,
    details_visible: bool,
    pressed_key_codes: Vec<String>,
    button_input: InputState,
    input: InputState,
    input_edges: InputEdges,
}

impl Default for RuneAppModel {
    fn default() -> Self {
        Self::from_demo_result(frontend::demo_image().map_err(|error| error.to_string()))
    }
}

impl RuneAppModel {
    fn from_demo_result(demo: Result<RgbaImage, String>) -> Self {
        Self {
            demo,
            settings: Settings::default(),
            page: Page::Demo,
            details_visible: false,
            pressed_key_codes: Vec::new(),
            button_input: InputState::new(),
            input: InputState::new(),
            input_edges: InputEdges::default(),
        }
    }

    /// Current page selection.
    #[must_use]
    pub const fn page(&self) -> Page {
        self.page
    }

    pub fn set_page(&mut self, page: Page) {
        self.page = page;
    }

    /// Whether the Rune layer should expose additional snapshot details.
    #[must_use]
    pub const fn details_visible(&self) -> bool {
        self.details_visible
    }

    pub fn set_details_visible(&mut self, visible: bool) {
        self.details_visible = visible;
    }

    pub fn toggle_details(&mut self) {
        self.details_visible = !self.details_visible;
    }

    /// The synthetic source image, or its load diagnostic.
    pub fn demo(&self) -> Result<&RgbaImage, &str> {
        self.demo.as_ref().map_err(String::as_str)
    }

    #[must_use]
    pub const fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Replace settings and recalculate the held-key mask under the new bindings.
    pub fn replace_settings(&mut self, settings: Settings) {
        self.settings = settings;
        self.refresh_input();
    }

    /// Apply an in-memory settings edit and recalculate held-key state.
    pub fn update_settings(&mut self, update: impl FnOnce(&mut Settings)) {
        update(&mut self.settings);
        self.refresh_input();
    }

    /// Parse the core's portable settings document and apply it in memory.
    ///
    /// Selecting a file path and persisting this document remain platform-layer
    /// responsibilities; this method performs no file I/O.
    pub fn apply_settings_document(&mut self, document: &str) -> Result<(), SettingsError> {
        let settings = Settings::parse(document)?;
        self.replace_settings(settings);
        Ok(())
    }

    /// Serialize settings in the core's canonical portable document format.
    #[must_use]
    pub fn settings_document(&self) -> String {
        self.settings.render()
    }

    /// Set currently held host keys using canonical codes such as `KeyZ` or
    /// `ArrowUp`. Unknown codes are ignored, as in the core input model.
    pub fn set_pressed_key_codes<'a>(&mut self, codes: impl IntoIterator<Item = &'a str>) {
        self.pressed_key_codes = codes
            .into_iter()
            .filter_map(Key::from_code)
            .map(|key| key.code().to_owned())
            .collect();
        self.refresh_input();
    }

    /// Update one canonical physical key while preserving all other held keys.
    pub fn set_key_code(&mut self, code: &str, pressed: bool) {
        let Some(key) = Key::from_code(code) else {
            return;
        };
        if pressed {
            if !self.pressed_key_codes.iter().any(|held| held == key.code()) {
                self.pressed_key_codes.push(key.code().to_owned());
            }
        } else {
            self.pressed_key_codes.retain(|held| held != key.code());
        }
        self.refresh_input();
    }

    /// Add or remove a host button independently of keyboard bindings (for
    /// example, a host-provided touch control). This only updates model state;
    /// the platform adapter is responsible for honoring `touch_controls`.
    pub fn set_button_pressed(&mut self, button: Button, pressed: bool) {
        self.button_input.set(button, pressed);
        self.refresh_input();
    }

    /// Current held-button mask, suitable for `Bus::set_pressed_keys` if a
    /// future runner explicitly consumes it. The current Rune demo does not.
    #[must_use]
    pub const fn input(&self) -> InputState {
        self.input
    }

    /// Host-side transitions since the preceding input update; not guest state.
    #[must_use]
    pub const fn input_edges(&self) -> InputEdges {
        self.input_edges
    }

    pub fn clear_input(&mut self) {
        self.pressed_key_codes.clear();
        self.button_input.clear();
        self.refresh_input();
    }

    /// Compose the selected page with the core's shared frontend UI.
    ///
    /// `Settings.fullscreen` selects the core's fullscreen *image layout*; it
    /// does not open or resize an operating-system surface. Other settings are
    /// retained and displayed by the settings page, but do not trigger host
    /// effects here. The shared shell always draws touch controls, so the host
    /// must honor `touch_controls` when routing pointer events. This model does
    /// not provide audio playback, settings persistence, save slots or guest
    /// execution.
    pub fn composed_image(&self) -> Result<RgbaImage, String> {
        match self.page {
            Page::Settings => Ok(ui::settings_page(&self.settings)),
            Page::Saves => Ok(ui::saves_page(&[], 0)),
            Page::Demo => {
                let demo = self.demo.as_ref().map_err(|error| error.clone())?;
                if self.settings.fullscreen {
                    Ok(ui::fullscreen(demo))
                } else {
                    Ok(ui::shell_with_controls(
                        demo,
                        self.input.mask(),
                        self.settings.touch_controls,
                    ))
                }
            }
        }
    }

    fn refresh_input(&mut self) {
        let mut keyboard = InputState::new();
        keyboard.apply_codes(
            self.pressed_key_codes.iter().map(String::as_str),
            &self.settings.bindings,
        );
        let next = InputState::from_mask(keyboard.mask() | self.button_input.mask());
        self.input_edges.update(next.mask());
        self.input = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gba_native::frontend::image::RgbaImage;

    fn model_with_demo() -> RuneAppModel {
        let mut rgba = Vec::with_capacity(240 * 160 * 4);
        for _ in 0..240 * 160 {
            rgba.extend_from_slice(&[0x44, 0x88, 0xCC, 0xFF]);
        }
        let demo = RgbaImage::from_rgba(240, 160, rgba, 7).expect("valid test image");
        RuneAppModel::from_demo_result(Ok(demo))
    }

    #[test]
    fn initial_state_uses_core_defaults_and_empty_input() {
        let model = model_with_demo();
        assert_eq!(model.page(), Page::Demo);
        assert!(!model.details_visible());
        assert_eq!(model.settings(), &Settings::default());
        assert_eq!(model.input().mask(), 0);
        assert_eq!(model.demo().unwrap().guest_budget(), 7);
    }

    #[test]
    fn default_loads_the_core_synthetic_demo_image() {
        let model = RuneAppModel::default();
        let demo = model.demo().expect("core demo should load");
        assert_eq!((demo.width(), demo.height()), (240, 160));
        assert_eq!(demo.guest_budget(), gba_native::demo::DEMO_BUDGET);
    }

    #[test]
    fn keyboard_and_button_inputs_combine_and_report_edges() {
        let mut model = model_with_demo();
        model.set_pressed_key_codes(["KeyZ", "ArrowLeft", "unknown"]);
        assert!(model.input().is_pressed(Button::A));
        assert!(model.input().is_pressed(Button::Left));
        assert_eq!(
            model.input_edges().just_pressed(),
            Button::A.bit() | Button::Left.bit()
        );

        model.set_button_pressed(Button::Start, true);
        assert_eq!(model.input_edges().just_pressed(), Button::Start.bit());
        model.set_pressed_key_codes(["ArrowLeft"]);
        assert_eq!(model.input_edges().just_released(), Button::A.bit());
        assert!(model.input().is_pressed(Button::Start));
        model.clear_input();
        assert_eq!(model.input().mask(), 0);
        assert_eq!(
            model.input_edges().just_released(),
            Button::Left.bit() | Button::Start.bit()
        );
    }

    #[test]
    fn settings_edits_recompute_held_keys_and_round_trip_as_text() {
        let mut model = model_with_demo();
        model.set_pressed_key_codes(["KeyZ"]);
        model.update_settings(|settings| {
            let digit = Key::from_code("Digit1").expect("known key");
            settings
                .bindings
                .set(Button::A, vec![digit])
                .expect("valid binding");
        });
        assert_eq!(model.input().mask(), 0);

        let document = model.settings_document();
        let mut restored = model_with_demo();
        restored
            .apply_settings_document(&document)
            .expect("valid settings");
        assert_eq!(restored.settings(), model.settings());
        assert_eq!(restored.settings_document(), document);
    }

    #[test]
    fn invalid_settings_document_leaves_the_model_unchanged() {
        let mut model = model_with_demo();
        let before = model.settings_document();
        assert!(model.apply_settings_document("fullscreen=maybe\n").is_err());
        assert_eq!(model.settings_document(), before);
    }

    #[test]
    fn composed_views_use_core_ui_and_settings_page_survives_demo_error() {
        let model = model_with_demo();
        let shell = model.composed_image().expect("shell composition");
        assert_eq!((shell.width(), shell.height()), (ui::WIDTH, ui::HEIGHT));
        assert_eq!(shell.guest_budget(), 7);

        let mut broken = RuneAppModel::from_demo_result(Err("test fixture error".to_owned()));
        assert_eq!(
            broken.composed_image(),
            Err("test fixture error".to_owned())
        );
        broken.set_page(Page::Settings);
        let settings = broken
            .composed_image()
            .expect("settings page is independent");
        assert_eq!(
            (settings.width(), settings.height()),
            (ui::WIDTH, ui::HEIGHT)
        );
        assert_eq!(settings.guest_budget(), 0);
    }

    #[test]
    fn fullscreen_setting_changes_only_the_selected_composition() {
        let mut model = model_with_demo();
        let shell = model.composed_image().expect("shell");
        model.update_settings(|settings| settings.fullscreen = true);
        let fullscreen_layout = model.composed_image().expect("fullscreen layout");
        assert_eq!(
            (fullscreen_layout.width(), fullscreen_layout.height()),
            (800, 600)
        );
        assert_ne!(shell.rgba(), fullscreen_layout.rgba());
    }

    #[test]
    fn touch_controls_preference_changes_only_the_shared_shell_composition() {
        let mut model = model_with_demo();
        let shown = model.composed_image().expect("shell with touch controls");
        model.update_settings(|settings| settings.touch_controls = false);
        let hidden = model
            .composed_image()
            .expect("shell without touch controls");
        assert_ne!(shown.rgba(), hidden.rgba());
        assert_eq!(shown.guest_budget(), hidden.guest_budget());
    }

    #[test]
    fn details_visibility_is_independent_of_page_and_snapshot() {
        let mut model = RuneAppModel::from_demo_result(Err("unavailable".to_owned()));
        model.toggle_details();
        assert!(model.details_visible());
        model.set_page(Page::Settings);
        model.set_details_visible(false);
        assert!(!model.details_visible());
    }
}
