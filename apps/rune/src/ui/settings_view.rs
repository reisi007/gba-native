use gba_native::input::Button;
use rune::tok::{self, type_};
use rune::{El, col, el, row, text};

use super::play_test_tone;
use super::{AppState, create_tone_artifact, input};

pub(super) fn view(state: &AppState, dark: bool, compact: bool) -> El {
    let theme = tok::theme(dark);
    let settings = state.model.settings();
    let mut bindings = col().key("key-bindings").w_full().gap(6.0);
    for pair in Button::ALL.chunks(if compact { 1 } else { 2 }) {
        let mut binding_row = row().w_full().gap(8.0);
        for button in pair {
            let label = settings
                .bindings
                .keys_for(*button)
                .iter()
                .map(|key| key.label())
                .collect::<Vec<_>>()
                .join(" + ");
            let key = format!("rebind-{}", button.name().to_ascii_lowercase());
            let selected = state.binding_target == Some(*button);
            let current = *button;
            let mut binding = el()
                .key(key)
                .role("button")
                .label(format!(
                    "{} binding: {}{}",
                    button.name(),
                    label,
                    if selected { "; press a key" } else { "" }
                ))
                .focusable(true)
                .on_physical_input(input::physical_input)
                .h(36.0)
                .px(10.0)
                .center()
                .radius(6.0)
                .fill(if selected {
                    tok::accent(0, true)
                } else {
                    theme.card
                })
                .child(
                    text(format!("{}: {}", button.name(), label))
                        .font(type_::LABEL)
                        .color(if selected {
                            super::accent_foreground()
                        } else {
                            theme.text
                        }),
                )
                .on_press(move |state: &mut AppState| state.begin_binding(current));
            if compact {
                binding = binding.w_full();
            }
            binding_row = binding_row.child(binding);
        }
        bindings = bindings.child(binding_row);
    }

    let volume_label = text(format!("Host volume: {}%", settings.master_volume))
        .font(type_::LABEL)
        .color(theme.text);
    let volume = if compact {
        col()
            .w_full()
            .gap(8.0)
            .child(super::button("volume-down", "Volume −10", volume_down).w_full())
            .child(volume_label.w_full().center())
            .child(super::button("volume-up", "Volume +10", volume_up).w_full())
    } else {
        row()
            .gap(8.0)
            .child(super::button("volume-down", "Volume −10", volume_down))
            .child(volume_label)
            .child(super::button("volume-up", "Volume +10", volume_up))
    };
    let artifact_label = if cfg!(target_os = "ios") {
        "Write WAV artifact"
    } else {
        "Download WAV"
    };
    let download = super::button("tone-artifact", artifact_label, create_tone_artifact);
    let play = super::button("tone-play", "Play test tone", play_test_tone);
    let actions = if compact {
        col()
            .w_full()
            .gap(8.0)
            .child(download.w_full())
            .child(play.w_full())
    } else {
        row().gap(8.0).child(download).child(play)
    };
    let status = state
        .binding_status
        .as_deref()
        .or(state.audio_status.as_deref())
        .or(state.settings_status.as_deref())
        .unwrap_or({
            #[cfg(target_os = "ios")]
            {
                "Settings are saved on this device; Play requests the core-generated WAV through AVFoundation. Output is not confirmed."
            }
            #[cfg(not(target_os = "ios"))]
            {
                "Settings are saved on this device; test tone uses the host or browser audio output."
            }
        });
    col()
        .key("settings-controls")
        .w_full()
        .gap(8.0)
        .child(super::button(
            "touch-controls",
            format!("Touch controls: {}", enabled(settings.touch_controls)),
            toggle_touch_controls,
        ))
        .child(super::button(
            "integer-scaling",
            format!("Integer scaling: {}", enabled(settings.integer_scaling)),
            toggle_integer_scaling,
        ))
        .child(super::button(
            "mute",
            format!("Mute: {}", enabled(settings.muted)),
            toggle_mute,
        ))
        .child(
            text("Key bindings · select a button, then press a host key. Escape cancels.")
                .w_full()
                .wrap()
                .font(type_::LABEL)
                .color(theme.text),
        )
        .child(bindings)
        .child(volume)
        .child(actions)
        .child(
            text(status)
                .w_full()
                .wrap_anywhere()
                .font(type_::BODY)
                .color(theme.text_secondary),
        )
}

fn toggle_touch_controls(state: &mut AppState) {
    state
        .model
        .update_settings(|settings| settings.touch_controls = !settings.touch_controls);
    state.persist_settings();
    state.refresh_composition();
}

fn toggle_integer_scaling(state: &mut AppState) {
    state
        .model
        .update_settings(|settings| settings.integer_scaling = !settings.integer_scaling);
    state.persist_settings();
    state.refresh_composition();
}

pub(super) fn toggle_mute(state: &mut AppState) {
    state
        .model
        .update_settings(|settings| settings.muted = !settings.muted);
    state.persist_settings();
    state.refresh_composition();
}

pub(super) fn volume_down(state: &mut AppState) {
    state.model.update_settings(|settings| {
        settings.master_volume = settings.master_volume.saturating_sub(10)
    });
    state.persist_settings();
    state.refresh_composition();
}

pub(super) fn volume_up(state: &mut AppState) {
    state.model.update_settings(|settings| {
        settings.master_volume = settings.master_volume.saturating_add(10).min(100)
    });
    state.persist_settings();
    state.refresh_composition();
}

fn enabled(value: bool) -> &'static str {
    if value { "On" } else { "Off" }
}
