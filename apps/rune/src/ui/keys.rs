use rune::{Key, KeyEvent, Tx};

#[cfg(not(target_os = "ios"))]
use super::pages::play_test_tone;
use super::pages::{show_saves, toggle_fullscreen};
use super::{AppState, Page, settings_view};

pub(super) fn app_key(state: &mut AppState, event: &KeyEvent, tx: &mut Tx) -> bool {
    match &event.key {
        Key::Function(11) => {
            toggle_fullscreen(state, tx);
            true
        }
        Key::Function(1) if crate::rom_binding::BOUND_ROM.is_none() => {
            state.select_page(if state.model.page() == Page::Settings {
                Page::Demo
            } else {
                Page::Settings
            });
            true
        }
        Key::Function(2) if crate::rom_binding::BOUND_ROM.is_none() => {
            if state.model.page() == Page::Saves {
                state.select_page(Page::Demo);
            } else {
                show_saves(state);
            }
            true
        }
        Key::Escape if state.model.page() != Page::Game && state.model.page() != Page::Demo => {
            state.binding_target = None;
            state.select_page(if crate::rom_binding::BOUND_ROM.is_some() {
                Page::Game
            } else {
                Page::Demo
            });
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

pub(super) fn bootstrap_fullscreen(state: &mut AppState, elapsed: f64, tx: &mut Tx) -> bool {
    #[cfg(target_arch = "wasm32")]
    let mut changed = super::browser_saves::poll(state, tx);
    #[cfg(not(target_arch = "wasm32"))]
    let mut changed = false;
    if !state.fullscreen_bootstrap_done {
        if !state.model.settings().fullscreen {
            state.fullscreen_bootstrap_done = true;
        } else if elapsed >= 0.2 && rune::commit_generation() > 0 {
            // Ask exactly once: `toggle_full_screen` flips state, and the shell drops the
            // request while the window is not visible yet, so wait for the first painted
            // frame plus a short settle delay instead of toggling on every poll.
            state.fullscreen_bootstrap_done = true;
            tx.toggle_full_screen();
            changed = true;
        }
    }
    super::game_actions::poll_game(state, elapsed) || changed
}
