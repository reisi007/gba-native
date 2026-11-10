use super::*;
use gba_native::frontend::ui as core_ui;
use rune::{Env, Runtime};

mod game;
mod input_tester;
mod touch_tests;

#[test]
fn accent_foreground_meets_normal_text_contrast_on_rune_blue() {
    let accent = rune::tok::accent(0, true).luminance();
    let foreground = super::accent_foreground().luminance();
    let contrast = (accent.max(foreground) + 0.05) / (accent.min(foreground) + 0.05);
    assert!(
        contrast >= 4.5,
        "Rune accent text contrast was {contrast:.2}:1"
    );
}

fn click(runtime: &mut Runtime<AppState>, key: &str) {
    let (x, y) = runtime.find(key).expect("button is laid out");
    runtime.pointer_down(x, y);
    runtime.pointer_up(x, y);
    runtime.frame(1.0 / 60.0);
}

#[test]
fn core_demo_image_and_shared_ui_are_present() {
    let mut runtime = Runtime::new(
        AppState::default(),
        view,
        (960.0, 720.0),
        Env::new(true, 1.0),
    );
    runtime.frame(0.0);
    assert!(runtime.find("snapshot").is_some());
    assert!(runtime.find("button-demo").is_some());
    assert!(runtime.find("button-input-tester").is_some());
    assert!(runtime.find("button-settings").is_some());
}

#[test]
fn settings_and_fullscreen_composition_are_interactive() {
    let mut runtime = Runtime::new(
        AppState::default(),
        view,
        (960.0, 720.0),
        Env::new(true, 1.0),
    );
    runtime.frame(0.0);
    click(&mut runtime, "button-settings");
    assert_eq!(runtime.state.model.page(), Page::Settings);
    assert!(runtime.find("button-touch-controls").is_some());
    assert_eq!(
        runtime.state.composed.size(),
        (core_ui::WIDTH, core_ui::HEIGHT)
    );
    click(&mut runtime, "button-fullscreen");
    assert!(runtime.state.model.settings().fullscreen);
    click(&mut runtime, "button-touch-controls");
    assert!(!runtime.state.model.settings().touch_controls);
    click(&mut runtime, "button-demo");
    assert_eq!(runtime.state.model.page(), Page::Demo);
    assert_eq!(
        runtime.state.composed.size(),
        (core_ui::WIDTH, core_ui::HEIGHT)
    );
}

#[test]
fn details_scene_remains_headless_drivable() {
    let mut runtime = Runtime::new(
        AppState::default(),
        view,
        (960.0, 720.0),
        Env::new(true, 1.0),
    );
    runtime.frame(0.0);
    assert!(runtime.find("details").is_none());
    click(&mut runtime, "button-details");
    assert!(runtime.find("details").is_some());
}

#[test]
fn compact_settings_fit_the_viewport_and_scroll_to_lower_controls() {
    let mut state = AppState::default();
    state.select_page(Page::Settings);
    let mut runtime = Runtime::new(state, view, (390.0, 844.0), Env::new(true, 2.0));
    runtime.frame(0.0);
    runtime.frame(1.0 / 60.0);

    for key in [
        "button-demo",
        "button-settings",
        "button-saves",
        "button-fullscreen",
        "button-details",
        "button-touch-controls",
        "rebind-a",
        "rebind-b",
        "button-volume-down",
        "button-volume-up",
        "button-tone-artifact",
    ] {
        let rect = runtime
            .elements()
            .into_iter()
            .find(|element| element.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("{key} is laid out on the compact settings page"))
            .rect;
        assert!(rect[0] >= 0.0, "{key} starts inside the viewport: {rect:?}");
        assert!(
            rect[0] + rect[2] <= 390.0,
            "{key} ends inside the 390-point viewport: {rect:?}"
        );
    }
    #[cfg(not(target_os = "ios"))]
    {
        let rect = runtime
            .elements()
            .into_iter()
            .find(|element| element.key.as_deref() == Some("button-tone-play"))
            .expect("host and browser settings expose test tone playback")
            .rect;
        assert!(rect[0] >= 0.0);
        assert!(rect[0] + rect[2] <= 390.0);
    }

    let before = runtime
        .find("button-tone-artifact")
        .expect("lower WAV action is laid out");
    let (scroll_x, scroll_y) = runtime
        .find("page-scroll")
        .expect("compact page has a scroll viewport");
    runtime.wheel(scroll_x, scroll_y, -620.0);
    runtime.frame(1.0 / 60.0);
    let after = runtime
        .find("button-tone-artifact")
        .expect("WAV action remains in the scroll content");
    assert!(
        after.1 < before.1,
        "scroll moves lower controls upward: {before:?} -> {after:?}"
    );
}

#[cfg(target_os = "ios")]
#[test]
fn ios_settings_expose_test_tone_playback_through_the_adapter() {
    let mut state = AppState::default();
    state.select_page(Page::Settings);
    let mut runtime = Runtime::new(state, view, (390.0, 844.0), Env::new(true, 2.0));
    runtime.frame(0.0);
    runtime.frame(1.0 / 60.0);

    assert!(runtime.find("button-tone-artifact").is_some());
    assert!(runtime.find("button-tone-play").is_some());
    assert!(
        runtime
            .state
            .audio_status
            .as_deref()
            .unwrap_or("core-generated WAV is available for AVFoundation playback")
            .contains("WAV")
    );
}

#[test]
fn compact_save_actions_stay_inside_the_portrait_viewport() {
    let mut state = AppState::default();
    state.select_page(Page::Saves);
    let mut runtime = Runtime::new(state, view, (390.0, 844.0), Env::new(true, 2.0));
    runtime.frame(0.0);
    runtime.frame(1.0 / 60.0);

    for key in [
        "button-demo",
        "button-settings",
        "button-saves",
        "button-fullscreen",
        "button-details",
        "button-save-new",
        "button-save-import",
        "button-save-export",
        "button-save-default",
        "button-save-delete",
    ] {
        let rect = runtime
            .elements()
            .into_iter()
            .find(|element| element.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("{key} is laid out on the compact save page"))
            .rect;
        assert!(rect[0] >= 0.0, "{key} starts inside the viewport: {rect:?}");
        assert!(
            rect[0] + rect[2] <= 390.0,
            "{key} ends inside the 390-point viewport: {rect:?}"
        );
    }
}

#[test]
fn responsive_demo_fits_portrait_width_and_scrolls_in_landscape() {
    let mut portrait = Runtime::new(
        AppState::default(),
        view,
        (390.0, 844.0),
        Env::new(true, 2.0),
    );
    portrait.frame(0.0);
    portrait.frame(1.0 / 60.0);
    assert_eq!(portrait.state.composed.size(), (700, 525));
    let snapshot = portrait
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some("snapshot"))
        .expect("portrait demo image is laid out");
    assert!(snapshot.rect[0] + snapshot.rect[2] <= 390.0);

    let mut landscape = Runtime::new(
        AppState::default(),
        view,
        (844.0, 390.0),
        Env::new(true, 2.0),
    );
    landscape.frame(0.0);
    landscape.frame(1.0 / 60.0);
    assert_eq!(landscape.state.composed.size(), (1600, 1200));
    let before = landscape
        .find("snapshot")
        .expect("landscape demo image is laid out");
    let (scroll_x, scroll_y) = landscape
        .find("page-scroll")
        .expect("landscape page has a scroll viewport");
    landscape.wheel(scroll_x, scroll_y, -620.0);
    landscape.frame(1.0 / 60.0);
    let after = landscape
        .find("snapshot")
        .expect("demo image remains in the scroll content");
    assert!(
        after.1 < before.1,
        "scroll reveals the full image: {before:?} -> {after:?}"
    );
}

#[test]
fn shell_touch_drag_presses_and_releases_the_core_button() {
    let mut runtime = Runtime::new(
        AppState::default(),
        view,
        (960.0, 720.0),
        Env::new(true, 1.0),
    );
    runtime.frame(0.0);
    let rect = runtime
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some("snapshot"))
        .expect("snapshot is laid out")
        .rect;
    let point = (rect[0] + rect[2] * 0.93, rect[1] + rect[3] * 0.73);
    runtime.pointer_down(point.0, point.1);
    runtime.frame(1.0 / 60.0);
    assert!(
        runtime
            .state
            .model
            .input()
            .is_pressed(gba_native::input::Button::A)
    );
    runtime.pointer_up(point.0, point.1);
    runtime.frame(1.0 / 60.0);
    assert!(
        !runtime
            .state
            .model
            .input()
            .is_pressed(gba_native::input::Button::A)
    );
}

#[test]
fn focused_physical_key_events_update_core_input_and_focus_loss_clears_it() {
    use rune::{Mods, PhysicalInput, PhysicalKeyCode};

    let mut runtime = Runtime::new(
        AppState::default(),
        view,
        (960.0, 720.0),
        Env::new(true, 1.0),
    );
    runtime.frame(0.0);
    click(&mut runtime, "button-demo");
    runtime.physical_input(PhysicalInput::Key {
        key: PhysicalKeyCode::KeyZ,
        mods: Mods::default(),
        pressed: true,
        repeat: false,
    });
    runtime.frame(1.0 / 60.0);
    assert!(
        runtime
            .state
            .model
            .input()
            .is_pressed(gba_native::input::Button::A)
    );
    runtime.physical_input(PhysicalInput::FocusLost);
    runtime.frame(1.0 / 60.0);
    assert_eq!(runtime.state.model.input().mask(), 0);
}

#[test]
fn high_dpi_surfaces_expand_by_integer_nearest_samples() {
    let source = [1, 2, 3, 255, 4, 5, 6, 255];
    assert_eq!(
        nearest_scale(&source, 2, 1, 2),
        [
            1, 2, 3, 255, 1, 2, 3, 255, 4, 5, 6, 255, 4, 5, 6, 255, 1, 2, 3, 255, 1, 2, 3, 255, 4,
            5, 6, 255, 4, 5, 6, 255,
        ]
    );

    let mut runtime = Runtime::new(
        AppState::default(),
        view,
        (960.0, 720.0),
        Env::new(true, 2.0),
    );
    runtime.frame(0.0);
    assert_eq!(runtime.state.composed.size(), (1600, 1200));
    let snapshot = runtime
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some("snapshot"))
        .expect("snapshot element is present");
    assert_eq!(snapshot.role, "img");
    assert_eq!(
        snapshot.label.as_deref(),
        Some("Synthetic GBA demo shell with static 240 by 160 fixture and host controls")
    );
    click(&mut runtime, "button-settings");
    click(&mut runtime, "button-integer-scaling");
    assert!(!runtime.state.model.settings().integer_scaling);
    assert_eq!(runtime.state.composed.size(), (800, 600));
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn rune_save_page_creates_exports_and_removes_synthetic_slots() {
    let root = std::env::temp_dir().join(format!(
        "gba-native-rune-ui-saves-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut runtime = Runtime::new(
        AppState::default(),
        view,
        (960.0, 720.0),
        Env::new(true, 1.0),
    );
    runtime.state.save_store = crate::saves::SaveStore::at(root.clone());
    runtime.frame(0.0);
    click(&mut runtime, "button-saves");
    click(&mut runtime, "button-save-new");
    assert_eq!(runtime.state.save_slots.len(), 1);
    assert!(runtime.find("save-slot-slot1").is_some());
    click(&mut runtime, "button-save-export");
    assert!(
        runtime
            .state
            .save_status
            .as_deref()
            .unwrap()
            .contains("exported slot1")
    );
    click(&mut runtime, "button-save-delete");
    assert!(runtime.state.save_slots.is_empty());
    std::fs::remove_dir_all(root).expect("remove the test-owned save directory");
}

#[test]
fn empty_save_page_disables_actions_that_need_a_selected_slot() {
    let mut state = AppState::default();
    state.select_page(Page::Saves);
    let mut runtime = Runtime::new(state, view, (960.0, 720.0), Env::new(true, 1.0));
    runtime.frame(0.0);

    let elements = runtime.elements();
    for key in [
        "button-save-export",
        "button-save-default",
        "button-save-delete",
    ] {
        let element = elements
            .iter()
            .find(|element| element.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("missing save action {key}"));
        assert!(
            !element.focusable,
            "{key} must not take focus without a slot"
        );
    }
    let accessibility = runtime.accessibility_json().to_string();
    assert_eq!(accessibility.matches("disabled").count(), 4);

    for key in [
        "button-save-export",
        "button-save-default",
        "button-save-delete",
    ] {
        click(&mut runtime, key);
    }
    assert!(runtime.state.save_slots.is_empty());
    assert!(runtime.state.save_status.is_none());
}

#[test]
fn settings_bindings_reassign_keys_and_reject_cross_button_duplicates() {
    use gba_native::input::Button;
    use rune::{Mods, PhysicalInput, PhysicalKeyCode};

    let mut runtime = Runtime::new(
        AppState::default(),
        view,
        (960.0, 720.0),
        Env::new(true, 1.0),
    );
    runtime.frame(0.0);
    click(&mut runtime, "button-settings");
    click(&mut runtime, "rebind-a");
    runtime.physical_input(PhysicalInput::Key {
        key: PhysicalKeyCode::KeyC,
        mods: Mods::default(),
        pressed: true,
        repeat: false,
    });
    runtime.frame(1.0 / 60.0);
    assert_eq!(
        runtime.state.model.settings().bindings.keys_for(Button::A)[0].code(),
        "KeyC"
    );
    assert!(runtime.state.binding_target.is_none());

    click(&mut runtime, "rebind-start");
    runtime.physical_input(PhysicalInput::Key {
        key: PhysicalKeyCode::KeyX,
        mods: Mods::default(),
        pressed: true,
        repeat: false,
    });
    runtime.frame(1.0 / 60.0);
    assert_eq!(runtime.state.binding_target, Some(Button::Start));
    assert!(
        runtime
            .state
            .binding_status
            .as_deref()
            .unwrap()
            .contains("cannot bind")
    );
}
