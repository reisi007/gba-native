//! Rune's settings and composed UI stay aligned with shared core contracts.

use std::path::PathBuf;

use gba_native::frontend::saves::SaveSlots;
use gba_native::frontend::ui as core_ui;
use gba_native::input::Settings;
use rune::{Env, Runtime};

use super::{AppState, view};
use crate::model::Page;
use crate::saves::SaveStore;

fn temp_root(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "gba-native-rune-contract-{tag}-{}-{}",
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
    // Settle layout after the previous action before resolving bounds.
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

#[test]
fn rune_save_transitions_create_default_export_and_delete_slots() {
    let root = temp_root("save-transitions");
    let mut rune = rune_runtime(root.clone());
    click(&mut rune, "button-saves");
    assert!(rune_slots(&rune).is_empty());

    click(&mut rune, "button-save-new");
    click(&mut rune, "button-save-new");
    assert_eq!(
        rune_slots(&rune),
        vec![
            ("slot1".to_owned(), 512, true),
            ("slot2".to_owned(), 512, false),
        ]
    );

    click(&mut rune, "save-slot-slot2");
    click(&mut rune, "button-save-default");
    assert!(rune_slots(&rune)[1].2);
    click(&mut rune, "button-save-export");
    let exported = std::fs::read(root.join("export-slot2.sav")).expect("Rune export");
    assert_eq!(exported, SaveSlots::synthetic_image(1));

    click(&mut rune, "button-save-delete");
    assert_eq!(rune_slots(&rune), vec![("slot1".to_owned(), 512, true)]);
    std::fs::remove_dir_all(root).expect("remove test-owned save directory");
}

#[test]
fn rune_new_slot_skips_an_occupied_count_based_name_after_deletion() {
    let root = temp_root("save-name-collision");
    let mut rune = rune_runtime(root.clone());
    click(&mut rune, "button-saves");
    click(&mut rune, "button-save-new");
    click(&mut rune, "button-save-new");

    click(&mut rune, "save-slot-slot1");
    click(&mut rune, "button-save-delete");
    assert_eq!(rune_slots(&rune), vec![("slot2".to_owned(), 512, true)]);

    click(&mut rune, "button-save-new");
    assert_eq!(
        rune_slots(&rune),
        vec![
            ("slot2".to_owned(), 512, true),
            ("slot3".to_owned(), 512, false),
        ]
    );
    let slots = SaveSlots::new(root.clone());
    assert_eq!(slots.load("slot2").unwrap(), SaveSlots::synthetic_image(1));
    assert_eq!(slots.load("slot3").unwrap(), SaveSlots::synthetic_image(1));
    std::fs::remove_dir_all(root).expect("remove test-owned save directory");
}

#[test]
fn rune_settings_transitions_match_the_shared_settings_contract() {
    use rune::{Mods, PhysicalInput, PhysicalKeyCode};

    let rune_root = temp_root("settings-transitions");
    let mut rune = rune_runtime(rune_root.clone());
    click(&mut rune, "button-settings");

    let defaults = Settings::default();
    assert_eq!(*rune.state.model.settings(), defaults);

    // Drive the actual Rune controls and compare with the shared Settings value.
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

    let mut expected_settings = defaults;
    expected_settings.touch_controls = false;
    expected_settings.integer_scaling = false;
    expected_settings.muted = true;
    expected_settings.master_volume += 10;
    expected_settings
        .bindings
        .set(
            gba_native::input::Button::A,
            vec![gba_native::input::Key::from_code("KeyC").unwrap()],
        )
        .unwrap();

    assert_eq!(*rune.state.model.settings(), expected_settings);
    assert_eq!(
        Settings::parse(&rune.state.model.settings_document()).unwrap(),
        expected_settings,
        "Rune persistence uses the shared canonical settings document"
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
        "Rune must persist the canonical settings document"
    );

    // The demo page is the shared core shell composition around the raw demo image.
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

    // Settings page composition uses the shared core renderer.
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
