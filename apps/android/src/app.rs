//! Android-only native window and event loop.
//!
//! The shared frontend composes 800x600 screens; this module copies them into
//! the `ANativeWindow` buffer with integer letterboxing and routes touch
//! events back into the host-testable model.

use std::time::Duration;

use android_activity::input::{Axis, InputEvent, MotionAction};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use ndk::hardware_buffer_format::HardwareBufferFormat;
use ndk::native_window::NativeWindow;

use gba_native::frontend::ui;

use crate::model::{AppModel, Page, SettingsAction, composition_pixel, locate};

/// `android-activity` entry point; it may run again if the activity is recreated.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    if let Err(message) = run(app) {
        eprintln!("gba-native-android: {message}");
    }
}

fn run(app: AndroidApp) -> Result<(), String> {
    let data = app
        .internal_data_path()
        .ok_or_else(|| "the activity has no internal data path".to_string())?;
    let mut model = AppModel::new(data.join("settings.ini"))?;
    match model.write_tone_artifact(&data) {
        Ok(path) => println!(
            "audio_out={} volume={} muted={}",
            path.display(),
            model.settings().master_volume,
            model.settings().muted
        ),
        Err(error) => eprintln!("audio_out: cannot write the tone artifact: {error}"),
    }

    let mut surface = (0u32, 0u32);
    let mut dirty = true;
    loop {
        let mut exit = false;
        app.poll_events(Some(Duration::from_millis(250)), |event| match event {
            PollEvent::Main(MainEvent::InitWindow { .. }) => dirty = true,
            PollEvent::Main(MainEvent::WindowResized { .. }) => dirty = true,
            PollEvent::Main(MainEvent::RedrawNeeded { .. }) => dirty = true,
            PollEvent::Main(MainEvent::InputAvailable) => {
                dirty |= handle_input(&app, &mut model, surface);
            }
            PollEvent::Main(MainEvent::Destroy) => exit = true,
            _ => {}
        });
        if exit {
            break;
        }
        if dirty {
            if let Some(window) = app.native_window() {
                surface = (
                    u32::try_from(window.width()).unwrap_or(0),
                    u32::try_from(window.height()).unwrap_or(0),
                );
                draw(&window, &model.compose())?;
            }
            dirty = false;
        }
    }
    Ok(())
}

fn handle_input(app: &AndroidApp, model: &mut AppModel, surface: (u32, u32)) -> bool {
    let Ok(mut events) = app.input_events_iter() else {
        return false;
    };
    let mut changed = false;
    while events.next(|event| {
        if let InputEvent::MotionEvent(motion) = event {
            let pointer = motion.pointer_at_index(motion.pointer_index());
            let x = pointer.axis_value(Axis::X);
            let y = pointer.axis_value(Axis::Y);
            match motion.action() {
                MotionAction::Down | MotionAction::PointerDown => {
                    changed |= on_down(model, surface, x, y);
                }
                MotionAction::Up | MotionAction::PointerUp => {
                    changed |= on_up(model, surface, x, y);
                }
                MotionAction::Cancel => {
                    model.clear_pressed();
                    changed = true;
                }
                _ => {}
            }
        }
        InputStatus::Handled
    }) {}
    changed
}

fn on_down(model: &mut AppModel, surface: (u32, u32), x: f32, y: f32) -> bool {
    let Some((nx, ny)) = locate(surface, x, y) else {
        return false;
    };
    let (px, py) = composition_pixel(nx, ny);
    match model.page() {
        Page::Shell => {
            if model.header_hit(px, py) {
                model.toggle_settings(true);
                return true;
            }
            model.touch_down(nx, ny).is_some()
        }
        Page::Settings => {
            let Some(action) = model.settings_action(px, py) else {
                return false;
            };
            if action == SettingsAction::Close {
                model.toggle_settings(false);
            } else if let Err(error) = model.apply_settings_action(action) {
                eprintln!("settings: cannot persist {action:?}: {error}");
            }
            true
        }
        Page::Saves => {
            if let Some(action) = ui::saves_action_at(px, py) {
                if let Err(error) = model.apply_saves_action(action) {
                    eprintln!("saves: cannot apply {action:?}: {error}");
                }
                return true;
            }
            model.saves_row(px, py).is_some()
        }
    }
}

fn on_up(model: &mut AppModel, surface: (u32, u32), x: f32, y: f32) -> bool {
    let Some((nx, ny)) = locate(surface, x, y) else {
        return false;
    };
    model.touch_up(nx, ny).is_some()
}

fn draw(window: &NativeWindow, image: &gba_native::frontend::RgbaImage) -> Result<(), String> {
    let width = u32::try_from(window.width()).unwrap_or(0);
    let height = u32::try_from(window.height()).unwrap_or(0);
    if width == 0 || height == 0 {
        return Ok(());
    }
    window
        .set_buffers_geometry(
            i32::try_from(width).unwrap_or(0),
            i32::try_from(height).unwrap_or(0),
            Some(HardwareBufferFormat::R8G8B8A8_UNORM),
        )
        .map_err(|error| format!("cannot set the window geometry: {error}"))?;
    let mut buffer = window
        .lock(None)
        .map_err(|error| format!("cannot lock the window buffer: {error}"))?;
    let stride = buffer.stride();
    let viewport =
        gba_native::frontend::compute_viewport(image.width(), image.height(), width, height);
    let Some(bytes) = buffer.bytes() else {
        return Err("the window buffer has no 32-bit pixel format".to_string());
    };
    let mut row = vec![0u8; width as usize * 4];
    for y in 0..height {
        let inside_rows = y >= viewport.y && y < viewport.y + viewport.height;
        if inside_rows {
            let sy = (y - viewport.y) * image.height() / viewport.height.max(1);
            for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
                let x = x as u32;
                if x >= viewport.x && x < viewport.x + viewport.width {
                    let sx = (x - viewport.x) * image.width() / viewport.width.max(1);
                    let index = ((sy * image.width() + sx) * 4) as usize;
                    pixel.copy_from_slice(&[
                        image.rgba()[index],
                        image.rgba()[index + 1],
                        image.rgba()[index + 2],
                        0xFF,
                    ]);
                } else {
                    pixel.copy_from_slice(&[0, 0, 0, 0xFF]);
                }
            }
        } else {
            row.fill(0);
            for pixel in row.chunks_exact_mut(4) {
                pixel[3] = 0xFF;
            }
        }
        let base = y as usize * stride * 4;
        for (slot, value) in bytes[base..base + width as usize * 4]
            .iter_mut()
            .zip(row.iter())
        {
            slot.write(*value);
        }
    }
    Ok(())
}
