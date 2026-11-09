use rune::tok::{self, type_};
use rune::{AppSpec, Cx, El, Pixels, ScrollSpec, SurfaceFit, Tx, col, el, row, text};

use crate::model::{Page, RuneAppModel};

#[cfg(target_arch = "wasm32")]
mod browser_saves;
mod buttons;
mod input;
mod keys;
mod save_actions;
mod saves_view;
mod settings_view;
mod surface;
use buttons::{accent_foreground, button, state_button};
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
    pointer_button: Option<gba_native::input::Button>,
    settings_status: Option<String>,
    audio_status: Option<String>,
    save_store: crate::saves::SaveStore,
    save_slots: Vec<gba_native::frontend::saves::SaveSlot>,
    selected_save_slot: usize,
    save_status: Option<String>,
    #[cfg(target_arch = "wasm32")]
    browser_saves: browser_saves::BrowserSaveState,
    binding_target: Option<gba_native::input::Button>,
    binding_status: Option<String>,
    fullscreen_bootstrap_done: bool,
}

impl Default for AppState {
    fn default() -> Self {
        let (settings, settings_status) = crate::storage::load_settings();
        let mut model = RuneAppModel::default();
        model.replace_settings(settings);
        let mut state = Self {
            model,
            composed: Pixels::new(),
            source: None,
            composition_error: None,
            pointer_button: None,
            settings_status,
            audio_status: None,
            save_store: crate::saves::SaveStore::default(),
            save_slots: Vec::new(),
            selected_save_slot: 0,
            save_status: None,
            #[cfg(target_arch = "wasm32")]
            browser_saves: browser_saves::BrowserSaveState::default(),
            binding_target: None,
            binding_status: None,
            fullscreen_bootstrap_done: false,
        };
        state.refresh_composition();
        state
    }
}

impl AppState {
    fn refresh_composition(&mut self) {
        match self.model.composed_image() {
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

    fn ensure_integer_device_pixels(&self, device_scale: f32, root_width: Option<f32>) {
        let Some(source) = &self.source else { return };
        let maximum_factor = (8192 / source.width()).min(8192 / source.height());
        let requested_factor = if self.model.settings().integer_scaling {
            device_scale.floor().max(1.0) as u32
        } else {
            1
        };
        let available_width = root_width.map_or(source.width() * requested_factor, |width| {
            ((width - 40.0).max(1.0) * device_scale).floor() as u32
        });
        let factor = (available_width / source.width())
            .min(requested_factor)
            .min(maximum_factor)
            .max(1);
        let (width, height) = if available_width >= source.width() {
            (source.width() * factor, source.height() * factor)
        } else {
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

fn show_demo(state: &mut AppState) {
    state.select_page(Page::Demo);
}

fn show_settings(state: &mut AppState) {
    state.select_page(Page::Settings);
}

fn show_saves(state: &mut AppState) {
    state.select_page(Page::Saves);
    #[cfg(target_arch = "wasm32")]
    state
        .browser_saves
        .show(&state.save_store, &mut state.save_status);
    #[cfg(not(target_arch = "wasm32"))]
    state.reload_save_slots();
}

fn toggle_fullscreen(state: &mut AppState, tx: &mut Tx) {
    state.toggle_fullscreen_layout();
    tx.toggle_full_screen();
}

fn fullscreen_button() -> El {
    el().key("button-fullscreen")
        .role("button")
        .label("Toggle fullscreen")
        .focusable(true)
        .on_physical_input(input::physical_input)
        .h(40.0)
        .px(14.0)
        .center()
        .radius(8.0)
        .fill(tok::accent(0, true))
        .child(
            text("Fullscreen")
                .font(type_::LABEL)
                .color(accent_foreground()),
        )
        .on_press_tx(toggle_fullscreen)
}

fn toggle_details(state: &mut AppState) {
    state.toggle_details();
}

fn create_tone_artifact(state: &mut AppState) {
    state.audio_status =
        Some(crate::audio::create_artifact(state.model.settings()).unwrap_or_else(|error| error));
}

fn play_test_tone(state: &mut AppState) {
    state.audio_status =
        Some(crate::audio::play(state.model.settings()).unwrap_or_else(|error| error));
}

fn details_scene(state: &mut AppState) {
    state.model.set_details_visible(true);
}

pub(crate) fn view(state: &AppState, cx: &mut Cx<AppState>) -> El {
    let root_width = cx.layout_with("root", |rect| rect[2]);
    state.ensure_integer_device_pixels(cx.env().scale, root_width);
    let theme = tok::theme(cx.system_dark());
    let compact = root_width.is_some_and(|width| width < 640.0);
    let page_title = match state.model.page() {
        Page::Demo => "Synthetic demo",
        Page::Settings => "Settings",
        Page::Saves => "Save slots",
    };
    let toolbar = toolbar(state, compact);

    let image = if state.model.page() == Page::Settings {
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
            .role("image")
            .label("Synthetic GBA demo shell with static 240 by 160 fixture and host controls")
            .w(surface_width as f32 / scale)
            .h(surface_height as f32 / scale)
            .focusable(true)
            .pixels_fit(&state.composed, SurfaceFit::Fit)
            .surface_unscaled(true)
            .on_physical_input(input::physical_input)
            .on_drag(input::touch_drag)
    };

    let mut content = col().key("page-content").w_full().gap(12.0);
    if state.model.details_visible() {
        let details = match state.model.demo() {
            Ok(demo) => format!(
                "Synthetic fixture · {}×{} · {} · {} guest instructions (not frames or cycles)",
                demo.width(),
                demo.height(),
                demo.rgb_sha256(),
                demo.guest_budget()
            ),
            Err(error) => error.to_owned(),
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

    col()
        .key("root")
        .w_full()
        .h_full()
        .gap(12.0)
        .padding(20.0)
        .fill(theme.bg)
        .focusable(true)
        .on_physical_input(input::physical_input)
        .child(
            text(page_title)
                .role("heading")
                .level(1)
                .font(type_::TITLE)
                .color(theme.text),
        )
        .child(toolbar)
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

fn toolbar(state: &AppState, compact: bool) -> El {
    let demo = button("demo", "Demo", show_demo);
    let settings = button("settings", "Settings", show_settings);
    let saves = button("saves", "Saves", show_saves);
    let fullscreen = fullscreen_button();
    let details = button(
        "details",
        if state.model.details_visible() {
            "Hide details"
        } else {
            "Details"
        },
        toggle_details,
    );
    if compact {
        let navigation = row()
            .w_full()
            .gap(8.0)
            .child(demo)
            .child(settings)
            .child(saves);
        let display = row().w_full().gap(8.0).child(fullscreen).child(details);
        col()
            .key("toolbar")
            .w_full()
            .gap(8.0)
            .child(navigation)
            .child(display)
    } else {
        row()
            .key("toolbar")
            .w_full()
            .gap(8.0)
            .child(demo)
            .child(settings)
            .child(saves)
            .child(fullscreen)
            .child(details)
    }
}

pub fn app_spec() -> AppSpec<AppState> {
    AppSpec {
        scenes: &[("details", details_scene)],
        on_key: Some(keys::app_key),
        poll: Some(keys::bootstrap_fullscreen),
        ..AppSpec::new("GBA snapshot", (960.0, 720.0), AppState::default, view)
    }
}
