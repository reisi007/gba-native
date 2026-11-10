use gba_native::frontend::ui as core_ui;
use gba_native::input::Button;
use rune::{Drag, Phase, PhysicalInput, Tx};

use super::{AppState, Page};

pub(super) fn physical_input(state: &mut AppState, event: &PhysicalInput, _tx: &mut Tx) -> bool {
    match event {
        PhysicalInput::Key { key, pressed, .. } => {
            let code = format!("{key:?}");
            if *pressed && state.bind_key(&code) {
                return true;
            }
            state.model.set_key_code(&code, *pressed);
            state.refresh_composition();
        }
        PhysicalInput::FocusLost => {
            state.pointer_button = None;
            state.model.clear_input();
            state.refresh_composition();
        }
        PhysicalInput::Modifiers(_) => {}
    }
    false
}

pub(super) fn touch_drag(state: &mut AppState, drag: &mut Drag, _tx: &mut Tx) {
    if drag.phase == Phase::Up {
        set_pointer_button(state, None);
        return;
    }

    let enabled = matches!(state.model.page(), Page::Demo | Page::InputTester)
        && !state.model.settings().fullscreen
        && state.model.settings().touch_controls
        && drag.w > 0.0
        && drag.h > 0.0;
    let next = enabled
        .then(|| core_ui::hit_test(drag.x / drag.w, drag.y / drag.h))
        .flatten();
    set_pointer_button(state, next);
}

pub(super) fn release_pointer_button(state: &mut AppState) {
    set_pointer_button(state, None);
}

fn set_pointer_button(state: &mut AppState, next: Option<Button>) {
    if state.pointer_button == next {
        return;
    }
    if let Some(previous) = state.pointer_button {
        state.model.set_button_pressed(previous, false);
    }
    if let Some(button) = next {
        state.model.set_button_pressed(button, true);
    }
    state.pointer_button = next;
    state.refresh_composition();
}
