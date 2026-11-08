//! Executable parity checks against the existing frontends.
//!
//! The desktop saves page is pure state (`apps/desktop/src/saves.rs`), so the
//! same action sequence can be driven through the real desktop state machine and
//! through Rune's shared UI in one test. Slot sets, default markers and exported
//! bytes match when the count-based candidate name is free. Rune additionally
//! skips an occupied candidate instead of replacing its contents. The other
//! accepted difference is affordance: Rune marks slot-dependent actions disabled,
//! while the desktop page leaves them enabled and treats them as silent no-ops.
//!
//! Settings and demo composition are compared against the shared core contract
//! (`Settings::render`/`parse`, `ui::shell_with_controls`) that the desktop and
//! browser frontends also render.

use std::path::PathBuf;

use gba_native::frontend::saves::SaveSlots;
use gba_native::frontend::ui as core_ui;
use gba_native::input::Settings;
use rune::{Env, Runtime};

use super::{AppState, view};
use crate::model::Page;
use crate::saves::SaveStore;

#[path = "../../../desktop/src/saves.rs"]
// The desktop module is included as the behavioral reference; the parity test
// exercises its state machine, not every public helper.
#[allow(dead_code)]
mod desktop;

use desktop::SavesState;
use gba_native::frontend::ui::SavesAction;

fn temp_root(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "gba-native-rune-parity-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn rune_runtime(root: PathBuf) -> Runtime<AppState> {
    let mut runtime = Runtime::new(
        AppState::default(),
        view,
        (960.0, 720.0),
        Env::new(true, 1.0),
    );
    runtime.state.save_store = SaveStore::at(root);
    runtime.frame(0.0);
    runtime
}

fn click(runtime: &mut Runtime<AppState>, key: &str) {
    // A real user clicks a painted frame. Settle layout after the previous
    // action before resolving bounds, otherwise a just-created row can still
    // carry the previous frame's position.
    runtime.frame(0.0);
    let (x, y) = runtime
        .find(key)
        .unwrap_or_else(|| panic!("missing control {key}"));
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    runtime.frame(1.0 / 60.0);
}

fn rune_slots(runtime: &Runtime<AppState>) -> Vec<(String, usize, bool)> {
    runtime
        .state
        .save_slots
        .iter()
        .map(|slot| (slot.name.clone(), slot.bytes, slot.default))
        .collect()
}

fn desktop_slots(state: &SavesState) -> Vec<(String, usize, bool)> {
    state
        .slots()
        .iter()
        .map(|slot| (slot.name.clone(), slot.bytes, slot.default))
        .collect()
}

#[test]
fn desktop_and_rune_save_transitions_agree() {
    let desktop_root = temp_root("desktop");
    let rune_root = temp_root("rune");
    let mut desktop = SavesState::new(desktop_root.clone());
    desktop.open();
    let mut rune = rune_runtime(rune_root.clone());
    click(&mut rune, "button-saves");
    assert_eq!(desktop_slots(&desktop), rune_slots(&rune));

    desktop.apply(SavesAction::New);
    click(&mut rune, "button-save-new");
    assert_eq!(desktop_slots(&desktop), rune_slots(&rune));
    assert_eq!(
        desktop_slots(&desktop),
        vec![("slot1".to_owned(), 512, true)]
    );

    desktop.apply(SavesAction::New);
    click(&mut rune, "button-save-new");
    assert_eq!(desktop_slots(&desktop), rune_slots(&rune));

    desktop.select(1);
    click(&mut rune, "save-slot-slot2");
    desktop.apply(SavesAction::MakeDefault);
    click(&mut rune, "button-save-default");
    assert_eq!(desktop_slots(&desktop), rune_slots(&rune));
    assert!(
        desktop_slots(&desktop)
            .iter()
            .any(|(name, _, default)| name == "slot2" && *default)
    );

    desktop.apply(SavesAction::Export);
    click(&mut rune, "button-save-export");
    let desktop_bytes =
        std::fs::read(desktop_root.join("export-slot2.sav")).expect("desktop export");
    let rune_bytes = std::fs::read(rune_root.join("export-slot2.sav")).expect("rune export");
    assert_eq!(desktop_bytes, rune_bytes);
    assert_eq!(desktop_bytes.len(), 512);
    assert_eq!(&desktop_bytes[..8], b"GBANSV01");
    // Second created slot, zero-based seed 1: pins the naming/seed convention
    // that the desktop and Android frontends use.
    assert_eq!(rune_bytes, SaveSlots::synthetic_image(1));

    desktop.apply(SavesAction::Delete);
    click(&mut rune, "button-save-delete");
    assert_eq!(desktop_slots(&desktop), rune_slots(&rune));
    assert_eq!(desktop_slots(&desktop).len(), 1);

    std::fs::remove_dir_all(desktop_root).ok();
    std::fs::remove_dir_all(rune_root).ok();
}

#[test]
fn rune_creation_after_deletion_skips_the_occupied_count_based_slot() {
    let desktop_root = temp_root("desktop-recreate");
    let rune_root = temp_root("rune-recreate");
    let mut desktop = SavesState::new(desktop_root.clone());
    desktop.open();
    let mut rune = rune_runtime(rune_root.clone());
    click(&mut rune, "button-saves");

    desktop.apply(SavesAction::New);
    click(&mut rune, "button-save-new");
    desktop.apply(SavesAction::New);
    click(&mut rune, "button-save-new");
    assert_eq!(desktop_slots(&desktop), rune_slots(&rune));

    // Leave only slot2, then repeat New. The count-based candidate is occupied:
    // desktop recreates slot2, while Rune preserves it and advances to slot3.
    desktop.select(0);
    click(&mut rune, "save-slot-slot1");
    desktop.apply(SavesAction::Delete);
    click(&mut rune, "button-save-delete");
    assert_eq!(desktop_slots(&desktop), rune_slots(&rune));
    assert_eq!(desktop_slots(&desktop)[0].0, "slot2");

    desktop.apply(SavesAction::New);
    click(&mut rune, "button-save-new");
    assert_eq!(
        desktop_slots(&desktop),
        vec![("slot2".to_owned(), 512, true)]
    );
    assert_eq!(
        rune_slots(&rune),
        vec![
            ("slot2".to_owned(), 512, true),
            ("slot3".to_owned(), 512, false)
        ]
    );
    desktop.apply(SavesAction::Export);
    click(&mut rune, "button-save-export");
    assert_eq!(
        std::fs::read(desktop_root.join("export-slot2.sav")).unwrap(),
        std::fs::read(rune_root.join("export-slot2.sav")).unwrap()
    );
    assert_eq!(
        std::fs::read(rune_root.join("export-slot2.sav")).unwrap(),
        SaveSlots::synthetic_image(1)
    );
    assert_eq!(
        SaveSlots::new(rune_root.clone()).load("slot3").unwrap(),
        SaveSlots::synthetic_image(1)
    );

    std::fs::remove_dir_all(desktop_root).ok();
    std::fs::remove_dir_all(rune_root).ok();
}

#[test]
fn unavailable_save_actions_are_inert_in_both_frontends() {
    let desktop_root = temp_root("desktop-empty");
    let rune_root = temp_root("rune-empty");
    let mut desktop = SavesState::new(desktop_root.clone());
    desktop.open();
    let mut rune = rune_runtime(rune_root.clone());
    click(&mut rune, "button-saves");

    for action in [
        SavesAction::Export,
        SavesAction::MakeDefault,
        SavesAction::Delete,
    ] {
        desktop.apply(action);
    }
    for key in [
        "button-save-export",
        "button-save-default",
        "button-save-delete",
    ] {
        click(&mut rune, key);
    }

    assert_eq!(desktop_slots(&desktop), rune_slots(&rune));
    assert!(rune_slots(&rune).is_empty());
    assert_eq!(desktop.status(), None);
    assert_eq!(rune.state.save_status, None);

    // Accepted affordance divergence, asserted so it cannot drift silently: the
    // desktop page keeps these controls enabled, Rune marks them disabled.
    for key in [
        "button-save-export",
        "button-save-default",
        "button-save-delete",
    ] {
        let element = rune
            .elements()
            .into_iter()
            .find(|element| element.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("missing control {key}"));
        assert!(!element.focusable, "{key} must be disabled in Rune");
    }

    std::fs::remove_dir_all(desktop_root).ok();
    std::fs::remove_dir_all(rune_root).ok();
}

#[test]
fn rune_settings_transitions_match_the_shared_desktop_settings_contract() {
    use rune::{Mods, PhysicalInput, PhysicalKeyCode};

    let rune_root = temp_root("settings-transitions");
    let mut rune = rune_runtime(rune_root.clone());
    click(&mut rune, "button-settings");

    let defaults = Settings::default();
    assert_eq!(*rune.state.model.settings(), defaults);

    // Drive the actual Rune controls; the desktop stores the same shared
    // Settings value, so the equivalent desktop transition is applied below.
    click(&mut rune, "button-touch-controls");
    click(&mut rune, "button-integer-scaling");
    click(&mut rune, "button-mute");
    click(&mut rune, "button-volume-up");
    click(&mut rune, "rebind-a");
    rune.physical_input(PhysicalInput::Key {
        key: PhysicalKeyCode::KeyC,
        mods: Mods::default(),
        pressed: true,
        repeat: false,
    });
    rune.frame(1.0 / 60.0);

    let mut desktop_settings = defaults;
    desktop_settings.touch_controls = false;
    desktop_settings.integer_scaling = false;
    desktop_settings.muted = true;
    desktop_settings.master_volume += 10;
    desktop_settings
        .bindings
        .set(
            gba_native::input::Button::A,
            vec![gba_native::input::Key::from_code("KeyC").unwrap()],
        )
        .unwrap();

    assert_eq!(*rune.state.model.settings(), desktop_settings);
    assert_eq!(
        Settings::parse(&rune.state.model.settings_document()).unwrap(),
        desktop_settings,
        "Rune persistence uses the same canonical document the desktop reads"
    );

    std::fs::remove_dir_all(rune_root).ok();
}

#[test]
fn rune_settings_and_demo_stay_on_the_shared_core_contract() {
    let defaults = Settings::default();
    let document = defaults.render();
    assert_eq!(
        Settings::parse(&document).expect("core parses its document"),
        defaults
    );

    let rune_root = temp_root("rune-contract");
    let mut rune = rune_runtime(rune_root.clone());
    assert_eq!(*rune.state.model.settings(), defaults);
    assert_eq!(
        rune.state.model.settings().render(),
        defaults.render(),
        "Rune must persist the same settings document as the other frontends"
    );

    // The demo page is the shared core shell composition around the raw demo
    // image, exactly what the desktop render path produces.
    let demo = rune
        .state
        .model
        .demo()
        .expect("Rune loads the core synthetic demo image")
        .clone();
    let expected = core_ui::shell_with_controls(&demo, 0, true);
    let composed = rune
        .state
        .model
        .composed_image()
        .expect("Rune composes the demo page");
    assert_eq!(composed.rgba(), expected.rgba());

    // Pressed input must reach the same shared shell renderer.
    rune.state
        .model
        .set_button_pressed(gba_native::input::Button::A, true);
    let expected_pressed =
        core_ui::shell_with_controls(&demo, gba_native::input::Button::A.bit(), true);
    let composed_pressed = rune
        .state
        .model
        .composed_image()
        .expect("Rune composes the pressed demo page");
    assert_eq!(composed_pressed.rgba(), expected_pressed.rgba());
    rune.state
        .model
        .set_button_pressed(gba_native::input::Button::A, false);

    // Settings page composition is the same core renderer for every frontend.
    rune.state.model.set_page(Page::Settings);
    let expected_settings = core_ui::settings_page(rune.state.model.settings());
    let composed_settings = rune
        .state
        .model
        .composed_image()
        .expect("Rune composes the settings page");
    assert_eq!(composed_settings.rgba(), expected_settings.rgba());

    // Fullscreen demo composition is shared too.
    rune.state.model.set_page(Page::Demo);
    rune.state
        .model
        .update_settings(|settings| settings.fullscreen = true);
    let expected_fullscreen = core_ui::fullscreen(&demo);
    let composed_fullscreen = rune
        .state
        .model
        .composed_image()
        .expect("Rune composes the fullscreen demo page");
    assert_eq!(composed_fullscreen.rgba(), expected_fullscreen.rgba());

    std::fs::remove_dir_all(rune_root).ok();
}
