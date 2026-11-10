use gba_native::frontend::ui as core_ui;
use gba_native::input::Button;
use rune::{Drag, Phase, PhysicalInput, Tx};

use super::{AppState, Page};

pub(super) fn physical_input(state: &mut AppState, event: &PhysicalInput, _tx: &mut Tx) -> bool {
    // Browsers only allow audio to start from a user gesture; the first input
    // opens the shared AudioContext so later guest samples are not blocked.
    #[cfg(target_arch = "wasm32")]
    crate::audio::start_guest_audio();
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

    let bound_game = crate::rom_binding::BOUND_ROM.is_some();
    let enabled = matches!(
        state.model.page(),
        Page::Demo | Page::InputTester | Page::Game
    ) && (!state.model.settings().fullscreen || bound_game)
        && (state.model.settings().touch_controls || bound_game)
        && drag.w > 0.0
        && drag.h > 0.0;
    let next = if !enabled {
        None
    } else {
        match (normalized_x(state, drag), normalized_y(state, drag)) {
            (Some(x), Some(y)) => core_ui::hit_test(x, y),
            _ => None,
        }
    };
    set_pointer_button(state, next);
}

/// Drawn image rectangle inside an element that fits it with letterboxing.
fn fitted_rect(state: &AppState, drag: &Drag) -> (f32, f32, f32, f32) {
    let (image_width, image_height) = state.composed.size();
    if image_width == 0 || image_height == 0 || drag.w <= 0.0 || drag.h <= 0.0 {
        return (0.0, 0.0, drag.w.max(1.0), drag.h.max(1.0));
    }
    let image_aspect = image_width as f32 / image_height as f32;
    let box_aspect = drag.w / drag.h;
    if box_aspect > image_aspect {
        let drawn_width = drag.h * image_aspect;
        ((drag.w - drawn_width) / 2.0, 0.0, drawn_width, drag.h)
    } else {
        let drawn_height = drag.w / image_aspect;
        (0.0, (drag.h - drawn_height) / 2.0, drag.w, drawn_height)
    }
}

fn normalized_x(state: &AppState, drag: &Drag) -> Option<f32> {
    let (left, _, width, _) = fitted_rect(state, drag);
    let x = (drag.x - left) / width.max(1.0);
    (0.0..=1.0).contains(&x).then_some(x)
}

fn normalized_y(state: &AppState, drag: &Drag) -> Option<f32> {
    let (_, top, _, height) = fitted_rect(state, drag);
    let y = (drag.y - top) / height.max(1.0);
    (0.0..=1.0).contains(&y).then_some(y)
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
