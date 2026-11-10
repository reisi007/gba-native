use super::super::{AppState, Page, view};
use rune::{Env, Runtime};

fn game_runtime() -> Runtime<AppState> {
    let rom = {
        let mut bytes = vec![0; 192];
        bytes[..4].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
        bytes
    };
    let mut state = AppState {
        game: Some(
            crate::game::GameSession::from_bytes(rom, gba_native::audio::AudioSettings::default())
                .expect("small synthetic ROM"),
        ),
        game_status: Some("Loaded converted game".to_owned()),
        ..AppState::default()
    };
    state.select_page(Page::Game);
    Runtime::new(state, view, (960.0, 720.0), Env::new(true, 1.0))
}

#[test]
fn a_loaded_game_starts_on_a_direct_play_screen_without_rom_picker() {
    let mut runtime = game_runtime();
    runtime.frame(0.0);
    assert_eq!(runtime.state.model.page(), Page::Game);
    assert!(runtime.find("game-page").is_some());
    assert!(runtime.find("game-screen").is_some());
    assert!(runtime.find("button-rom-open").is_none());
    assert!(runtime.find("button-rom-start").is_none());
}

#[test]
fn bound_game_builds_hide_generic_application_navigation() {
    if crate::rom_binding::BOUND_ROM.is_none() {
        return;
    }
    let mut runtime = game_runtime();
    runtime.frame(0.0);
    assert!(runtime.find("toolbar").is_none());
    assert!(runtime.find("button-demo").is_none());
    assert!(runtime.find("button-settings").is_none());
    assert!(runtime.find("button-saves").is_none());
}
