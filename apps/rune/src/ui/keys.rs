use rune::{Key, KeyEvent, Tx};

#[cfg(not(target_os = "ios"))]
use super::play_test_tone;
use super::{AppState, Page, settings_view, show_saves, toggle_fullscreen};

pub(super) fn app_key(state: &mut AppState, event: &KeyEvent, tx: &mut Tx) -> bool {
    match &event.key {
        Key::Function(11) => {
            toggle_fullscreen(state, tx);
            true
        }
        Key::Function(1) => {
            state.select_page(if state.model.page() == Page::Settings {
                Page::Demo
            } else {
                Page::Settings
            });
            true
        }
        Key::Function(2) => {
            if state.model.page() == Page::Saves {
                state.select_page(Page::Demo);
            } else {
                show_saves(state);
            }
            true
        }
        Key::Escape if state.model.page() != Page::Demo => {
            state.binding_target = None;
            state.select_page(Page::Demo);
            true
        }
        #[cfg(not(target_os = "ios"))]
        Key::Text(key) if key.eq_ignore_ascii_case("p") => {
            play_test_tone(state);
            true
        }
        Key::Text(key) if key.eq_ignore_ascii_case("m") => {
            settings_view::toggle_mute(state);
            true
        }
        Key::Text(key) if key == "-" => {
            settings_view::volume_down(state);
            true
        }
        Key::Text(key) if key == "=" => {
            settings_view::volume_up(state);
            true
        }
        _ => false,
    }
}

pub(super) fn bootstrap_fullscreen(state: &mut AppState, _elapsed: f64, tx: &mut Tx) -> bool {
    if state.fullscreen_bootstrap_done {
        return false;
    }
    state.fullscreen_bootstrap_done = true;
    if state.model.settings().fullscreen {
        tx.toggle_full_screen();
    }
    false
}
