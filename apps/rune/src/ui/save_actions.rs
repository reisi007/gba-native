use super::AppState;

pub(super) fn create_save(state: &mut AppState) {
    #[cfg(target_arch = "wasm32")]
    state
        .browser_saves
        .create_slot(&state.save_store, &mut state.save_status);
    #[cfg(not(target_arch = "wasm32"))]
    {
        state.save_status = state.save_store.create().err();
        if state.save_status.is_none() {
            state.save_status = Some("created a synthetic save slot".to_owned());
        }
        state.reload_save_slots();
    }
}

#[cfg(any(test, target_arch = "wasm32", target_os = "ios"))]
pub(super) fn import_save(state: &mut AppState) {
    #[cfg(target_arch = "wasm32")]
    state
        .browser_saves
        .import(&state.save_store, &mut state.save_status);
    #[cfg(not(target_arch = "wasm32"))]
    {
        state.save_status = Some(match state.save_store.import_selected() {
            Ok(status) | Err(status) => status,
        });
        state.reload_save_slots();
    }
}

#[cfg(any(test, target_arch = "wasm32", target_os = "ios"))]
pub(super) fn export_save(state: &mut AppState) {
    let Some(name) = state
        .save_slots
        .get(state.selected_save_slot)
        .map(|slot| slot.name.clone())
    else {
        state.save_status = Some("select a save slot first".to_owned());
        return;
    };
    #[cfg(target_arch = "wasm32")]
    state
        .browser_saves
        .export(&state.save_store, &name, &mut state.save_status);
    #[cfg(not(target_arch = "wasm32"))]
    {
        state.save_status = Some(state.save_store.export(&name).unwrap_or_else(|error| error));
    }
}

pub(super) fn set_default_save(state: &mut AppState) {
    let Some(name) = state
        .save_slots
        .get(state.selected_save_slot)
        .map(|slot| slot.name.clone())
    else {
        state.save_status = Some("select a save slot first".to_owned());
        return;
    };
    #[cfg(target_arch = "wasm32")]
    state
        .browser_saves
        .set_default(&state.save_store, &name, &mut state.save_status);
    #[cfg(not(target_arch = "wasm32"))]
    {
        state.save_status = Some(
            state
                .save_store
                .set_default(&name)
                .map(|()| format!("{name} is now the default slot"))
                .unwrap_or_else(|error| error),
        );
        state.reload_save_slots();
    }
}

pub(super) fn delete_save(state: &mut AppState) {
    let Some(name) = state
        .save_slots
        .get(state.selected_save_slot)
        .map(|slot| slot.name.clone())
    else {
        state.save_status = Some("select a save slot first".to_owned());
        return;
    };
    #[cfg(target_arch = "wasm32")]
    state
        .browser_saves
        .delete_slot(&state.save_store, &name, &mut state.save_status);
    #[cfg(not(target_arch = "wasm32"))]
    {
        state.save_status = Some(
            state
                .save_store
                .delete(&name)
                .map(|()| format!("deleted {name}"))
                .unwrap_or_else(|error| error),
        );
        state.reload_save_slots();
    }
}
