use rune::tok::{self, type_};
use rune::{El, col, el, row, text};

use super::{AppState, create_save, delete_save, input, set_default_save};
#[cfg(any(test, target_arch = "wasm32", target_os = "ios"))]
use super::{export_save, import_save};

pub(super) fn view(state: &AppState, dark: bool, compact: bool) -> El {
    let theme = tok::theme(dark);
    let mut slots = col().key("save-slot-list").w_full().gap(8.0);
    if state.save_slots.is_empty() {
        slots = slots.child(text("No synthetic save slots yet.").color(theme.text_secondary));
    } else {
        for (index, slot) in state.save_slots.iter().enumerate() {
            let selected = index == state.selected_save_slot;
            let name = slot.name.clone();
            slots = slots.child(
                el().key(format!("save-slot-{name}"))
                    .role("button")
                    .label(format!(
                        "{} · {} bytes{}{}",
                        name,
                        slot.bytes,
                        if slot.default { " · default" } else { "" },
                        if selected { " · selected" } else { "" }
                    ))
                    .focusable(true)
                    .on_physical_input(input::physical_input)
                    .h(44.0)
                    .px(12.0)
                    .center()
                    .radius(6.0)
                    .fill(if selected {
                        tok::accent(0, true)
                    } else {
                        theme.card
                    })
                    .child(
                        text(format!(
                            "{}  ·  {} bytes{}",
                            name,
                            slot.bytes,
                            if slot.default { "  ·  default" } else { "" }
                        ))
                        .font(type_::LABEL)
                        .color(if selected {
                            rune::Color::WHITE
                        } else {
                            theme.text
                        }),
                    )
                    .on_press(move |state: &mut AppState| state.select_save_slot(index)),
            );
        }
    }

    let has_selected = state.save_slots.get(state.selected_save_slot).is_some();
    let selected_is_default = state
        .save_slots
        .get(state.selected_save_slot)
        .is_some_and(|slot| slot.default);
    let new = super::button("save-new", "New synthetic slot", create_save);
    let import = import_button();
    let export = export_button(has_selected, dark);
    let make_default = super::state_button(
        "save-default",
        "Make default",
        set_default_save,
        has_selected && !selected_is_default,
        dark,
    );
    let delete = super::state_button("save-delete", "Delete", delete_save, has_selected, dark);
    let actions = if compact {
        col()
            .w_full()
            .gap(8.0)
            .child(new.w_full())
            .child(import.w_full())
            .child(export.w_full())
            .child(make_default.w_full())
            .child(delete.w_full())
    } else {
        row()
            .w_full()
            .gap(8.0)
            .child(new)
            .child(import)
            .child(export)
            .child(make_default)
            .child(delete)
    };
    #[cfg(target_os = "ios")]
    let status = format!(
        "{}. Files: On My iPhone/iPad > GBA Native Rune > saves. Copy only an exact GBANSV01 fixture there as import.sav; exports are written to this folder. These are not game saves.",
        state
            .save_status
            .as_deref()
            .unwrap_or("Only generated GBANSV01 synthetic fixtures are accepted")
    );
    #[cfg(not(target_os = "ios"))]
    let status = state.save_status.as_deref().unwrap_or(
        "Only generated GBANSV01 synthetic fixtures are accepted; these slots are not game saves.",
    );
    col()
        .key("saves-page")
        .w_full()
        .gap(12.0)
        // A growing slot list must move the action row in the same frame. An
        // animated height puts the buttons on top of a just-added row, so a
        // click on that row would hit Delete instead.
        .instant_changes()
        .child(
            text("Synthetic save slots")
                .font(type_::TITLE)
                .color(theme.text),
        )
        .child(slots)
        .child(actions)
        .child(
            text(status)
                .w_full()
                .wrap_anywhere()
                .font(type_::BODY)
                .color(theme.text_secondary),
        )
}

#[cfg(any(test, target_arch = "wasm32", target_os = "ios"))]
fn import_button() -> El {
    super::button("save-import", "Import fixture", import_save)
}

#[cfg(all(not(test), not(target_arch = "wasm32"), not(target_os = "ios")))]
fn import_button() -> El {
    el().key("button-save-import")
        .role("button")
        .label("Import synthetic save fixture")
        .focusable(true)
        .on_physical_input(input::physical_input)
        .h(40.0)
        .px(14.0)
        .center()
        .radius(8.0)
        .fill(tok::accent(0, true))
        .child(
            text("Import fixture")
                .font(type_::LABEL)
                .color(rune::Color::WHITE),
        )
        .on_press_tx(|_: &mut AppState, tx: &mut rune::Tx| {
            let options = rune::FileDialogOptions {
                title: "Import synthetic save fixture".to_owned(),
                ..Default::default()
            }
            .filter("Synthetic save fixture", &["sav"]);
            tx.open_file(options, |state: &mut AppState, result, _| {
                state.save_status = Some(match result {
                    Ok(Some(path)) => state
                        .save_store
                        .import_path(&path)
                        .unwrap_or_else(|error| error),
                    Ok(None) => "save import cancelled".to_owned(),
                    Err(error) => format!("cannot open save import dialog: {error}"),
                });
                state.reload_save_slots();
            });
        })
}

#[cfg(any(test, target_arch = "wasm32", target_os = "ios"))]
fn export_button(enabled: bool, dark: bool) -> El {
    super::state_button("save-export", "Export", export_save, enabled, dark)
}

#[cfg(all(not(test), not(target_arch = "wasm32"), not(target_os = "ios")))]
fn export_button(enabled: bool, dark: bool) -> El {
    let button = super::buttons::button_base("save-export", "Export", enabled, dark)
        .label("Export selected save slot");
    if !enabled {
        return button;
    }
    button.on_press_tx(|state: &mut AppState, tx: &mut rune::Tx| {
        let Some(slot) = state.save_slots.get(state.selected_save_slot) else {
            state.save_status = Some("select a save slot first".to_owned());
            return;
        };
        let name = slot.name.clone();
        let options = rune::FileDialogOptions {
            title: "Export synthetic save fixture".to_owned(),
            file_name: Some(format!("{name}.sav")),
            ..Default::default()
        }
        .filter("Synthetic save fixture", &["sav"]);
        tx.save_file(options, move |state: &mut AppState, result, _| {
            state.save_status = Some(match result {
                Ok(Some(path)) => state
                    .save_store
                    .export_path(&name, &path)
                    .unwrap_or_else(|error| error),
                Ok(None) => "save export cancelled".to_owned(),
                Err(error) => format!("cannot open save export dialog: {error}"),
            });
        });
    })
}
