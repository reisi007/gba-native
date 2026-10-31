//! Desktop screen composition.
//!
//! The shell, fullscreen frame, settings page and saves page all come from the
//! shared `gba_native::frontend::ui` module; this module only selects which one
//! the window currently shows.

use gba_native::frontend::ui;

use super::Frontend;
use crate::presentation::RgbaImage;

impl Frontend {
    pub(super) fn compose(&self) -> RgbaImage {
        if self.saves.is_open() {
            self.saves.compose()
        } else if self.show_settings {
            ui::settings_page(&self.settings)
        } else if self.fullscreen() {
            ui::fullscreen(&self.image)
        } else {
            ui::shell(&self.image, self.input_mask)
        }
    }

    pub(super) fn fullscreen(&self) -> bool {
        self.window
            .as_ref()
            .is_some_and(|window| window.fullscreen().is_some())
            || (self.window.is_none() && self.settings.fullscreen)
    }
}
