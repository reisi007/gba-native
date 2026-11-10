use rune::{Cx, El, SurfaceFit, el, text};

use super::{AppState, input};

/// The one-game play interface: the Game Boy surface and nothing else.
pub(super) fn view(state: &AppState, _cx: &mut Cx<AppState>) -> El {
    // The 800x600 handheld composition is fitted by the host GPU instead of
    // being upscaled per frame on the CPU, which keeps input responsive.
    let surface = el()
        .key("game-screen")
        .role("img")
        .label("Game screen with GBA directional pad, A/B, Start/Select and shoulder controls")
        .w_full()
        .grow(1.0)
        .focusable(true)
        .pixels_fit(&state.composed, SurfaceFit::Fit)
        .on_physical_input(input::physical_input)
        .on_drag(input::touch_drag);

    let mut root = el()
        .key("game-page")
        .w_full()
        .h_full()
        .focusable(true)
        .on_physical_input(input::physical_input)
        .child(surface);

    if let Some(error) = state
        .game
        .as_ref()
        .and_then(|game| game.error())
        // A missing session reports its specific status below instead of the
        // generic "not loaded" composition error.
        .or_else(|| state.game.as_ref().and(state.composition_error.as_ref()))
    {
        root = root.child(
            el().key("game-error")
                .w_full()
                .child(text(format!("Execution stopped: {error}")).wrap_anywhere()),
        );
    } else if state.game.is_none() {
        let message = state.game_status.clone().unwrap_or_else(|| {
            "No game is bound to this build. Set GBA_NATIVE_ROM_PATH at build time.".to_owned()
        });
        root = root.child(
            el().key("game-status")
                .w_full()
                .child(text(message).wrap_anywhere()),
        );
    }
    root
}
