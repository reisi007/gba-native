use super::super::{AppState, Page, view};
use rune::{Env, Mods, PhysicalInput, PhysicalKeyCode, Runtime};

fn a_pressed(runtime: &Runtime<AppState>) -> bool {
    runtime
        .state
        .model
        .input()
        .is_pressed(gba_native::input::Button::A)
}

#[test]
fn changing_pages_releases_touch_but_preserves_a_held_keyboard_source() {
    let mut runtime = Runtime::new(
        AppState::default(),
        view,
        (960.0, 720.0),
        Env::new(true, 1.0),
    );
    runtime.frame(0.0);
    let rect = runtime
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some("snapshot"))
        .expect("snapshot is laid out")
        .rect;
    let point = (rect[0] + rect[2] * 0.82, rect[1] + rect[3] * 0.64);
    runtime.pointer_down(point.0, point.1);
    runtime.frame(1.0 / 60.0);
    runtime.physical_input(PhysicalInput::Key {
        key: PhysicalKeyCode::KeyZ,
        mods: Mods::default(),
        pressed: true,
        repeat: false,
    });
    runtime.frame(1.0 / 60.0);
    assert!(a_pressed(&runtime));

    runtime.state.select_page(Page::Settings);
    runtime.frame(1.0 / 60.0);
    assert_eq!(runtime.state.pointer_button, None);
    assert!(a_pressed(&runtime));

    runtime.physical_input(PhysicalInput::Key {
        key: PhysicalKeyCode::KeyZ,
        mods: Mods::default(),
        pressed: false,
        repeat: false,
    });
    runtime.frame(1.0 / 60.0);
    assert!(!a_pressed(&runtime));
}

#[test]
fn game_page_touch_controls_feed_the_loaded_guest() {
    let mut rom = vec![0; 192];
    rom[..4].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
    let mut state = AppState {
        game: Some(
            crate::game::GameSession::from_bytes(rom, gba_native::audio::AudioSettings::default())
                .expect("small synthetic ROM"),
        ),
        ..AppState::default()
    };
    state.select_page(Page::Game);
    let mut runtime = Runtime::new(state, view, (960.0, 720.0), Env::new(true, 1.0));
    runtime.frame(0.0);
    let rect = runtime
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some("game-screen"))
        .expect("live game screen is laid out")
        .rect;
    let point = (rect[0] + rect[2] * 0.93, rect[1] + rect[3] * 0.73);
    runtime.pointer_down(point.0, point.1);
    runtime.frame(1.0 / 60.0);
    assert!(
        runtime
            .state
            .model
            .input()
            .is_pressed(gba_native::input::Button::A),
        "rect={rect:?} pointer={:?} touch={}",
        runtime.state.pointer_button,
        runtime.state.model.settings().touch_controls
    );
    runtime.pointer_up(point.0, point.1);
    runtime.frame(1.0 / 60.0);
    assert!(
        !runtime
            .state
            .model
            .input()
            .is_pressed(gba_native::input::Button::A)
    );
}
