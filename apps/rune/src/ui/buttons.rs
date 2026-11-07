use rune::tok::{self, type_};
use rune::{El, el, text};

use super::{AppState, input};

pub(super) fn button(key: &'static str, label: impl Into<String>, action: fn(&mut AppState)) -> El {
    state_button(key, label, action, true, true)
}

pub(super) fn state_button(
    key: &'static str,
    label: impl Into<String>,
    action: fn(&mut AppState),
    enabled: bool,
    dark: bool,
) -> El {
    let button = button_base(key, label, enabled, dark);
    if enabled {
        button.on_press(action)
    } else {
        button
    }
}

pub(super) fn button_base(
    key: &'static str,
    label: impl Into<String>,
    enabled: bool,
    dark: bool,
) -> El {
    let label = label.into();
    let theme = tok::theme(dark);
    let button = el()
        .key(format!("button-{key}"))
        .role("button")
        .label(label.clone())
        .focusable(enabled)
        .disabled(!enabled)
        .h(40.0)
        .px(14.0)
        .center()
        .radius(8.0)
        .fill(if enabled {
            tok::accent(0, true)
        } else {
            theme.card
        })
        .child(text(label).font(type_::LABEL).color(if enabled {
            rune::Color::WHITE
        } else {
            theme.text_secondary
        }));
    if enabled {
        button.on_physical_input(input::physical_input)
    } else {
        button
    }
}
