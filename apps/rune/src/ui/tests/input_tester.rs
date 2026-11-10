use super::*;
use rune::{Env, Runtime};

#[test]
fn page_routes_touch_into_the_generated_guest() {
    let mut runtime = Runtime::new(
        AppState::default(),
        view,
        (960.0, 720.0),
        Env::new(true, 1.0),
    );
    runtime.frame(0.0);
    click(&mut runtime, "button-input-tester");
    assert_eq!(runtime.state.model.page(), Page::InputTester);
    let idle = runtime
        .state
        .model
        .input_tester()
        .expect("page has guest image")
        .expect("guest image rendered")
        .rgb_sha256()
        .to_owned();

    let rect = runtime
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some("snapshot"))
        .expect("input tester snapshot is laid out")
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
    let pressed = runtime
        .state
        .model
        .input_tester()
        .expect("page has guest image")
        .expect("guest image rendered");
    assert_ne!(idle, pressed.rgb_sha256());
    assert_eq!(
        pressed.rgb_sha256(),
        gba_native::frontend::input_tester_image(gba_native::input::Button::A.bit())
            .unwrap()
            .rgb_sha256()
    );

    runtime.pointer_up(point.0, point.1);
    runtime.frame(1.0 / 60.0);
    assert_eq!(
        runtime
            .state
            .model
            .input_tester()
            .unwrap()
            .unwrap()
            .rgb_sha256(),
        idle
    );
}
