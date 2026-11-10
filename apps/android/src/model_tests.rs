use super::*;
use gba_native::frontend::SaveSlots;
use gba_native::frontend::ui::SavesAction;
use gba_native::input::{GbaLayout, TouchLayout};
use gba_native::sha256;

fn temp(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "gba-native-android-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0)
    ))
}

fn model(tag: &str) -> (AppModel, PathBuf) {
    let directory = temp(tag);
    std::fs::create_dir_all(&directory).expect("temp dir");
    let path = directory.join("settings.ini");
    (AppModel::new(path).expect("model"), directory)
}

#[test]
fn demo_frame_matches_the_shared_frontend_digest() {
    let (model, directory) = model("demo");
    let shared = frontend::demo_image().expect("shared demo");
    assert_eq!(model.frame().width(), shared.width());
    assert_eq!(model.frame().height(), shared.height());
    assert_eq!(model.frame().rgb_sha256(), shared.rgb_sha256());
    assert_eq!(model.frame().rgba(), shared.rgba());
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn shell_and_fullscreen_use_the_shared_screens() {
    let (mut model, directory) = model("screens");
    let expected = frontend::ui::shell(model.frame(), 0);
    assert_eq!(model.compose().rgba(), expected.rgba());
    assert_eq!(model.compose().rgb_sha256(), expected.rgb_sha256());

    model.settings.fullscreen = true;
    let expected = frontend::ui::fullscreen(model.frame());
    assert_eq!(model.compose().rgba(), expected.rgba());
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn every_touch_button_is_reachable_and_released() {
    let (mut model, directory) = model("touch");
    let layout = TouchLayout::landscape();
    for button in Button::ALL {
        let (x, y, width, height) = layout
            .button_pixels(button, SCREEN_WIDTH, SCREEN_HEIGHT)
            .expect("button rect");
        let nx = (x as f32 + width as f32 / 2.0) / SCREEN_WIDTH as f32;
        let ny = (y as f32 + height as f32 / 2.0) / SCREEN_HEIGHT as f32;
        assert!(model.pointer_down(1, Some((nx, ny))));
        assert_eq!(model.pressed() & button.bit(), button.bit());
        assert!(model.pointer_up(1));
        assert_eq!(model.pressed() & button.bit(), 0);
    }
    assert_eq!(model.pressed(), 0);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn one_pointer_releasing_a_button_does_not_release_another_holders_button() {
    let (mut model, directory) = model("shared-button");
    let (x, y) = button_center(&TouchLayout::landscape(), Button::A);

    assert!(model.pointer_down(10, Some((x, y))));
    assert!(!model.pointer_down(11, Some((x, y))));
    assert_eq!(model.pressed(), Button::A.bit());

    assert!(!model.pointer_up(10));
    assert_eq!(model.pressed(), Button::A.bit());
    assert!(model.pointer_up(11));
    assert_eq!(model.pressed(), 0);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn pointer_motion_transfers_and_releases_only_its_own_button() {
    let (mut model, directory) = model("touch-motion");
    let layout = TouchLayout::landscape();
    let (ax, ay) = button_center(&layout, Button::A);
    let (bx, by) = button_center(&layout, Button::B);
    let (start, start_y) = button_center(&layout, Button::Start);

    assert!(model.pointer_down(1, Some((ax, ay))));
    assert!(model.pointer_down(2, Some((bx, by))));
    assert_eq!(model.pressed(), Button::A.bit() | Button::B.bit());

    assert!(model.pointer_move(1, Some((start, start_y))));
    assert_eq!(model.pressed(), Button::B.bit() | Button::Start.bit());
    assert!(model.pointer_move(1, None));
    assert_eq!(model.pressed(), Button::B.bit());
    assert!(!model.pointer_up(1));
    assert_eq!(model.pressed(), Button::B.bit());

    assert!(!model.pointer_move(99, Some((ax, ay))));
    assert!(!model.pointer_up(99));
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn cancel_and_page_changes_clear_pointer_ownership() {
    let (mut model, directory) = model("touch-cancel");
    let (x, y) = button_center(&TouchLayout::landscape(), Button::A);

    assert!(model.pointer_down(7, Some((x, y))));
    assert!(model.clear_pressed());
    assert!(!model.pointer_move(7, Some((x, y))));
    assert!(!model.pointer_up(7));
    assert_eq!(model.pressed(), 0);

    assert!(model.pointer_down(7, Some((x, y))));
    model.show_page(Page::Settings);
    assert_eq!(model.pressed(), 0);
    assert!(!model.pointer_move(7, Some((x, y))));
    assert!(!model.pointer_up(7));
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn taps_on_the_guest_screen_produce_no_button() {
    let (mut model, directory) = model("screen-taps");
    let layout = TouchLayout::landscape();
    let (sx, sy, sw, sh) = layout.screen.pixels(SCREEN_WIDTH, SCREEN_HEIGHT);
    let nx = (sx as f32 + sw as f32 / 2.0) / SCREEN_WIDTH as f32;
    let ny = (sy as f32 + sh as f32 / 2.0) / SCREEN_HEIGHT as f32;
    assert!(!model.pointer_down(1, Some((nx, ny))));
    assert_eq!(model.pressed(), 0);
    assert!(!model.pointer_up(1));
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn settings_taps_change_state_and_persist_through_the_shared_file() {
    let (mut model, directory) = model("settings");
    model.toggle_settings(true);
    assert_eq!(
        model.settings_action(500, 520),
        Some(SettingsAction::ToggleMute)
    );
    model
        .apply_settings_action(SettingsAction::ToggleMute)
        .expect("mute");
    assert!(model.settings().muted);

    let before = model.settings().master_volume;
    model
        .apply_settings_action(SettingsAction::VolumeUp)
        .expect("volume up");
    assert_eq!(model.settings().master_volume, (before + 10).min(100));
    model
        .apply_settings_action(SettingsAction::VolumeDown)
        .expect("volume down");
    assert_eq!(model.settings().master_volume, before);

    model
        .apply_settings_action(SettingsAction::ToggleFullscreen)
        .expect("fullscreen");
    model
        .apply_settings_action(SettingsAction::ToggleTouchControls)
        .expect("touch");
    model
        .apply_settings_action(SettingsAction::Close)
        .expect("close");
    assert!(!model.show_settings());
    assert_eq!(model.page(), Page::Shell);

    let (reloaded, warning) = frontend::settings::load(&model.settings_path);
    assert!(warning.is_none());
    assert_eq!(reloaded, *model.settings());
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn header_band_opens_the_settings_page() {
    let (mut model, directory) = model("header");
    assert!(model.header_hit(700, 10));
    assert!(!model.header_hit(10, 10));
    assert!(!model.header_hit(700, 100));
    model.toggle_settings(true);
    assert!(!model.header_hit(700, 10));
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn settings_page_offers_saves_and_every_control_has_a_tap_zone() {
    let zones = [
        ((360, 20), SettingsAction::OpenSaves),
        ((100, 430), SettingsAction::PlayTone),
        ((150, 470), SettingsAction::ToggleFullscreen),
        ((150, 510), SettingsAction::ToggleTouchControls),
        ((500, 520), SettingsAction::ToggleMute),
        ((640, 550), SettingsAction::VolumeDown),
        ((730, 550), SettingsAction::VolumeUp),
        ((700, 20), SettingsAction::Close),
    ];
    let (mut model, directory) = model("zones");
    model.toggle_settings(true);
    for ((x, y), expected) in zones {
        assert_eq!(model.settings_action(x, y), Some(expected));
    }
    assert_eq!(model.settings_action(1, 599), None);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn saves_page_creates_uses_defaults_and_deletes_slots() {
    let (mut model, directory) = model("saves");
    model.show_page(Page::Settings);
    model
        .apply_settings_action(SettingsAction::OpenSaves)
        .expect("open saves");
    assert_eq!(model.page(), Page::Saves);

    model
        .apply_saves_action(SavesAction::New)
        .expect("create first");
    model
        .apply_saves_action(SavesAction::New)
        .expect("create second");
    assert_eq!(model.slots().len(), 2);
    assert_eq!(model.slots()[0].name, "slot1");
    assert!(model.slots()[0].default);

    assert_eq!(model.saves_row(60, 104), Some(0));
    assert_eq!(model.saves_row(60, 138), Some(1));
    assert_eq!(model.saves_row(10, 104), None);

    model
        .apply_saves_action(SavesAction::MakeDefault)
        .expect("default");
    assert!(model.slots()[1].default);

    model
        .apply_saves_action(SavesAction::Delete)
        .expect("delete");
    assert_eq!(model.slots().len(), 1);
    assert_eq!(model.slots()[0].name, "slot1");
    assert!(model.slots()[0].default);

    let imported_bytes = SaveSlots::synthetic_image(9);
    std::fs::create_dir_all(model.slots_root()).expect("saves dir");
    std::fs::write(
        model.slots_root().join(frontend::IMPORT_FILE),
        &imported_bytes,
    )
    .expect("import file");
    model
        .apply_saves_action(SavesAction::Import)
        .expect("import");
    let imported_index = model
        .slots()
        .iter()
        .position(|slot| slot.name == "imported")
        .expect("imported slot");
    model.select_slot(imported_index);
    model
        .apply_saves_action(SavesAction::Export)
        .expect("export");
    assert_eq!(
        std::fs::read(model.slots_root().join("export-imported.sav")).expect("exported"),
        imported_bytes
    );

    model.apply_saves_action(SavesAction::Use).expect("use");
    assert_eq!(model.page(), Page::Shell);

    // The slots survive a model restart through the shared settings directory.
    let (reloaded, _) = {
        let path = model.settings_path.clone();
        (AppModel::new(path).expect("reload"), ())
    };
    assert_eq!(reloaded.slots().len(), 2);
    assert!(reloaded.slots().iter().any(|slot| slot.name == "slot1"));
    assert!(reloaded.slots().iter().any(|slot| slot.name == "imported"));
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn window_coordinates_map_through_the_shared_letterbox() {
    assert_eq!(locate((1600, 1200), 0.0, 0.0), Some((0.0, 0.0)));
    assert_eq!(locate((1600, 1200), 800.0, 600.0), Some((0.5, 0.5)));
    assert_eq!(locate((2000, 1200), 199.0, 0.0), None);
    let (nx, _) = locate((2000, 1200), 200.0, 0.0).expect("inside");
    assert!(nx.abs() < 1e-6);
    assert_eq!(locate((0, 0), 0.0, 0.0), None);
}

fn button_center(layout: &GbaLayout, button: Button) -> (f32, f32) {
    let (x, y, width, height) = layout
        .button_pixels(button, SCREEN_WIDTH, SCREEN_HEIGHT)
        .expect("button rect");
    (
        (x as f32 + width as f32 / 2.0) / SCREEN_WIDTH as f32,
        (y as f32 + height as f32 / 2.0) / SCREEN_HEIGHT as f32,
    )
}

#[test]
fn tone_artifact_is_identical_to_the_shared_tone() {
    let (model, directory) = model("tone");
    let path = model.write_tone_artifact(&directory).expect("tone");
    let written = std::fs::read(&path).expect("read tone");
    let shared = frontend::tone::tone_wav(model.settings());
    assert_eq!(
        sha256::hex(&sha256::digest(&written)),
        sha256::hex(&sha256::digest(&shared))
    );
    assert_eq!(written, shared);
    let _ = std::fs::remove_dir_all(directory);
}
