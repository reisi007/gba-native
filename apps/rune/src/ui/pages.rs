//! Non-game pages, their actions and the shared toolbar.

use rune::tok::{self, type_};
use rune::{El, Tx, col, el, row, text};

use super::AppState;
use super::buttons::{accent_foreground, button};
use super::input;
use crate::model::Page;

pub(super) fn show_demo(state: &mut AppState) {
    state.select_page(Page::Demo);
}

pub(super) fn show_input_tester(state: &mut AppState) {
    state.select_page(Page::InputTester);
}

pub(super) fn show_settings(state: &mut AppState) {
    state.select_page(Page::Settings);
}

pub(super) fn show_saves(state: &mut AppState) {
    state.select_page(Page::Saves);
    #[cfg(target_arch = "wasm32")]
    state
        .browser_saves
        .show(&state.save_store, &mut state.save_status);
    #[cfg(not(target_arch = "wasm32"))]
    state.reload_save_slots();
}

pub(super) fn toggle_fullscreen(state: &mut AppState, tx: &mut Tx) {
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

pub(super) fn create_tone_artifact(state: &mut AppState) {
    state.audio_status =
        Some(crate::audio::create_artifact(state.model.settings()).unwrap_or_else(|error| error));
}

pub(super) fn play_test_tone(state: &mut AppState) {
    state.audio_status =
        Some(crate::audio::play(state.model.settings()).unwrap_or_else(|error| error));
}

pub(super) fn details_scene(state: &mut AppState) {
    state.model.set_details_visible(true);
}

pub(super) fn toolbar(state: &AppState, compact: bool) -> El {
    let demo = button("demo", "Demo", show_demo);
    let settings = button("settings", "Settings", show_settings);
    let saves = button("saves", "Saves", show_saves);
    let input_test = button("input-tester", "Input", show_input_tester);
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
            .gap(6.0)
            .child(demo)
            .child(input_test)
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
            .child(input_test)
            .child(settings)
            .child(saves)
            .child(fullscreen)
            .child(details)
    }
}
