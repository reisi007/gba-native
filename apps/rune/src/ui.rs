use rune::tok::{self, type_};
use rune::{AppSpec, Cx, El, Pixels, ScrollSpec, SurfaceFit, col, el, text};

use crate::model::{Page, RuneAppModel};

#[cfg(target_arch = "wasm32")]
mod browser_saves;
mod buttons;
mod game_actions;
mod game_view;
mod input;
mod keys;
mod pages;
mod save_actions;
mod saves_view;
mod settings_view;
mod surface;
use buttons::{accent_foreground, button, state_button};
use pages::toolbar;
use save_actions::{create_save, delete_save, set_default_save};
#[cfg(any(test, target_arch = "wasm32", target_os = "ios"))]
use save_actions::{export_save, import_save};
use surface::{nearest_fit, nearest_scale};
#[cfg(all(test, not(target_arch = "wasm32")))]
mod parity_tests;
#[cfg(test)]
mod tests;

pub struct AppState {
    model: RuneAppModel,
    composed: Pixels,
    source: Option<gba_native::frontend::image::RgbaImage>,
    composition_error: Option<String>,
    game: Option<crate::game::GameSession>,
    game_status: Option<String>,
    pointer_button: Option<gba_native::input::Button>,
    settings_status: Option<String>,
    audio_status: Option<String>,
    save_store: crate::saves::SaveStore,
    save_slots: Vec<gba_native::frontend::saves::SaveSlot>,
    selected_save_slot: usize,
    save_status: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    save_profiles: Vec<String>,
    #[cfg(target_arch = "wasm32")]
    browser_saves: browser_saves::BrowserSaveState,
    binding_target: Option<gba_native::input::Button>,
    binding_status: Option<String>,
    fullscreen_bootstrap_done: bool,
    #[cfg(not(target_arch = "wasm32"))]
    audio_sink: crate::audio_sink::AudioSink,
    /// Wall clock anchor for guest pacing. Rune's frame delta is not a stable
    /// realtime reference on every host (idle frames, display refresh), so the
    /// native app measures its own poll interval.
    #[cfg(not(target_arch = "wasm32"))]
    last_poll: std::time::Instant,
}

impl Default for AppState {
    fn default() -> Self {
        let (settings, settings_status) = crate::storage::load_settings();
        let mut model = RuneAppModel::default();
        model.replace_settings(settings);
        if crate::rom_binding::BOUND_ROM.is_some() {
            // A one-game build opens as a handheld: fullscreen, controls visible.
            model.update_settings(|settings| settings.fullscreen = true);
        }

        let mut state = Self {
            model,
            composed: Pixels::new(),
            source: None,
            composition_error: None,
            game: None,
            game_status: None,
            pointer_button: None,
            settings_status,
            audio_status: None,
            save_store: crate::saves::SaveStore::default(),
            save_slots: Vec::new(),
            selected_save_slot: 0,
            save_status: None,
            #[cfg(not(target_arch = "wasm32"))]
            save_profiles: vec!["default".to_owned()],
            #[cfg(target_arch = "wasm32")]
            browser_saves: browser_saves::BrowserSaveState::default(),
            binding_target: None,
            binding_status: None,
            fullscreen_bootstrap_done: false,
            #[cfg(not(target_arch = "wasm32"))]
            audio_sink: crate::audio_sink::AudioSink::idle(),
            #[cfg(not(target_arch = "wasm32"))]
            last_poll: std::time::Instant::now(),
        };
        if crate::rom_binding::BOUND_ROM.is_some() {
            game_actions::start_bound_game(&mut state);
        }
        state.refresh_composition();
        state
    }
}

impl AppState {
    fn refresh_composition(&mut self) {
        match game_actions::presentation(self) {
            Ok(image) => {
                self.composed
                    .set(image.width(), image.height(), image.rgba().to_vec());
                self.source = Some(image);
                self.composition_error = None;
            }
            Err(error) => {
                self.source = None;
                self.composition_error = Some(error);
            }
        }
    }

    fn persist_settings(&mut self) {
        self.settings_status = crate::storage::save_settings(self.model.settings())
            .err()
            .map(|error| format!("Settings could not be saved: {error}"));
    }

    fn ensure_integer_device_pixels(&self, device_scale: f32, root: Option<(f32, f32)>) {
        let Some(source) = &self.source else { return };
        let maximum_factor = (8192 / source.width()).min(8192 / source.height());
        let requested_factor = if self.model.settings().integer_scaling {
            device_scale.floor().max(1.0) as u32
        } else {
            1
        };
        let padding = if crate::rom_binding::BOUND_ROM.is_some() {
            0.0
        } else {
            40.0
        };
        let available = root.map(|(width, height)| {
            (
                ((width - padding).max(1.0) * device_scale).floor() as u32,
                ((height - padding).max(1.0) * device_scale).floor() as u32,
            )
        });
        let available_width = available.map_or(source.width() * requested_factor, |size| size.0);
        // Only the handheld page is constrained by the window height; the
        // scrollable demo/settings pages intentionally grow past it.
        let available_height = if crate::rom_binding::BOUND_ROM.is_some() {
            available.map_or(source.height() * requested_factor, |size| size.1)
        } else {
            u32::MAX
        };
        if crate::rom_binding::BOUND_ROM.is_some() {
            // The handheld composition stays at its native 800x600 and the host
            // GPU fits it to the window; CPU upscaling every frame stalls input.
            if self.composed.size() != (source.width(), source.height()) {
                self.composed
                    .set(source.width(), source.height(), source.rgba().to_vec());
            }
            return;
        }
        let factor = (available_width / source.width())
            .min(available_height / source.height())
            .min(requested_factor)
            .min(maximum_factor)
            .max(1);
        let (width, height) =
            if available_width >= source.width() && available_height >= source.height() {
                (source.width() * factor, source.height() * factor)
            } else {
                // A window smaller than the shell falls back to a bounded fit.
                let width = available_width.clamp(1, 8192);
                let height = ((u64::from(source.height()) * u64::from(width)
                    + u64::from(source.width()) / 2)
                    / u64::from(source.width()))
                .min(8192) as u32;
                (width, height.max(1))
            };
        if self.composed.size() == (width, height) {
            return;
        }
        let rgba = if available_width >= source.width() {
            nearest_scale(source.rgba(), source.width(), source.height(), factor)
        } else {
            nearest_fit(
                source.rgba(),
                source.width(),
                source.height(),
                width,
                height,
            )
        };
        self.composed.set(width, height, rgba);
    }

    fn select_page(&mut self, page: Page) {
        if self.model.page() != page {
            input::release_pointer_button(self);
        }
        self.model.set_page(page);
        self.refresh_composition();
    }

    fn toggle_fullscreen_layout(&mut self) {
        let fullscreen = !self.model.settings().fullscreen;
        self.model
            .update_settings(|settings| settings.fullscreen = fullscreen);
        self.persist_settings();
        self.refresh_composition();
    }

    fn toggle_details(&mut self) {
        self.model.toggle_details();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn reload_save_slots(&mut self) {
        self.save_profiles = vec!["default".to_owned()];
        match self.save_store.profiles() {
            Ok(profiles) => self.save_profiles.extend(profiles),
            Err(error) => self.save_status = Some(error),
        }
        let selected_name = self
            .save_slots
            .get(self.selected_save_slot)
            .map(|slot| slot.name.clone());
        match self.save_store.slots() {
            Ok(slots) => {
                self.selected_save_slot = selected_name
                    .and_then(|name| slots.iter().position(|slot| slot.name == name))
                    .unwrap_or_else(|| self.selected_save_slot.min(slots.len().saturating_sub(1)));
                self.save_slots = slots;
            }
            Err(error) => {
                self.save_slots.clear();
                self.save_status = Some(error);
            }
        }
    }

    fn select_save_slot(&mut self, index: usize) {
        if index < self.save_slots.len() {
            self.selected_save_slot = index;
        }
    }

    fn begin_binding(&mut self, button: gba_native::input::Button) {
        self.binding_target = Some(button);
        self.binding_status = Some(format!("Press a key for {}; Escape cancels", button.name()));
    }

    fn bind_key(&mut self, code: &str) -> bool {
        let Some(button) = self.binding_target else {
            return false;
        };
        if code == "Escape" {
            self.binding_target = None;
            self.binding_status = Some("key binding cancelled".to_owned());
            return true;
        }
        let Some(key) = gba_native::input::Key::from_code(code) else {
            self.binding_status = Some(format!("{code} cannot be used as a GBA binding"));
            return true;
        };
        let mut settings = self.model.settings().clone();
        match settings.bindings.set(button, vec![key]) {
            Ok(()) => {
                self.model.replace_settings(settings);
                self.binding_target = None;
                self.binding_status =
                    Some(format!("{} is now bound to {}", button.name(), key.label()));
                self.persist_settings();
                self.refresh_composition();
            }
            Err(error) => self.binding_status = Some(format!("cannot bind key: {error:?}")),
        }
        true
    }
}

pub(crate) fn view(state: &AppState, cx: &mut Cx<AppState>) -> El {
    let root = cx.layout_with("root", |rect| (rect[2], rect[3]));
    let root_width = root.map(|size| size.0);
    state.ensure_integer_device_pixels(cx.env().scale, root);
    let theme = tok::theme(cx.system_dark());
    let compact = root_width.is_some_and(|width| width < 640.0);
    let bound_game = crate::rom_binding::BOUND_ROM;
    let page_title = if state.model.page() == Page::Game {
        bound_game.map_or("Game", |binding| binding.title)
    } else {
        match state.model.page() {
            Page::Demo => "Synthetic demo",
            Page::Settings => "Settings",
            Page::Saves => "Save slots",
            Page::InputTester => "Input tester",
            Page::Game => "Play local ROM",
        }
    };

    let image = if state.model.page() == Page::Game {
        game_view::view(state, cx)
    } else if state.model.page() == Page::Settings {
        settings_view::view(state, cx.system_dark(), compact)
    } else if state.model.page() == Page::Saves {
        saves_view::view(state, cx.system_dark(), compact)
    } else if let Some(error) = &state.composition_error {
        el().key("snapshot-error")
            .w_full()
            .h(120.0)
            .center()
            .fill(theme.card)
            .child(text(format!("Synthetic image unavailable: {error}")).color(theme.text))
    } else {
        let scale = cx.env().scale.max(1.0);
        let (surface_width, surface_height) = state.composed.size();
        el().key("snapshot")
            .role("img")
            .label(if state.model.page() == Page::InputTester {
                "Synthetic guest input test framebuffer"
            } else {
                "Synthetic GBA demo shell with static 240 by 160 fixture and host controls"
            })
            .w(surface_width as f32 / scale)
            .h(surface_height as f32 / scale)
            .focusable(true)
            .pixels_fit(&state.composed, SurfaceFit::Fit)
            .surface_unscaled(true)
            .on_physical_input(input::physical_input)
            .on_drag(input::touch_drag)
    };

    if bound_game.is_some() {
        return col()
            .key("root")
            .w_full()
            .h_full()
            .padding(0.0)
            .fill(tok::theme(true).bg)
            .focusable(true)
            .on_physical_input(input::physical_input)
            .child(image);
    }

    let mut content = col().key("page-content").w_full().gap(12.0);
    if state.model.details_visible() {
        let shown = if state.model.page() == Page::Game {
            None
        } else if state.model.page() == Page::InputTester {
            state.model.input_tester()
        } else {
            Some(state.model.demo())
        };
        let details = match shown {
            Some(Ok(demo)) => format!(
                "{} · {}×{} · {} · {} guest instructions (not frames or cycles)",
                if state.model.page() == Page::InputTester {
                    "Synthetic input tester"
                } else {
                    "Synthetic fixture"
                },
                demo.width(),
                demo.height(),
                demo.rgb_sha256(),
                demo.guest_budget()
            ),
            Some(Err(error)) => error.to_owned(),
            None if state.model.page() == Page::Game => state.game.as_ref().map_or_else(
                || "No game image is loaded".to_owned(),
                |game| {
                    format!(
                        "{} · {} guest instructions",
                        game.identity(),
                        game.instructions()
                    )
                },
            ),
            None => "Synthetic input tester has not run".to_owned(),
        };
        content = content.child(
            text(details)
                .key("details")
                .w_full()
                .wrap_anywhere()
                .font(type_::BODY)
                .color(theme.text_secondary),
        );
    }
    content = content.child(image);

    let root = col()
        .key("root")
        .w_full()
        .h_full()
        .gap(12.0)
        .padding(20.0)
        .fill(theme.bg)
        .focusable(true)
        .on_physical_input(input::physical_input);
    root.child(
        text(page_title)
            .role("heading")
            .level(1)
            .font(type_::TITLE)
            .color(theme.text),
    )
    .child(toolbar(state, compact))
    .child(
        col()
            .key("page-scroll")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(content),
    )
}

pub fn app_spec() -> AppSpec<AppState> {
    AppSpec {
        scenes: &[("details", pages::details_scene)],
        on_key: Some(keys::app_key),
        poll: Some(keys::bootstrap_fullscreen),
        ..AppSpec::new(
            crate::rom_binding::BOUND_ROM.map_or("GBA snapshot", |binding| binding.title),
            (960.0, 720.0),
            AppState::default,
            view,
        )
    }
}
