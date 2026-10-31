//! Keyboard, pointer and touch handling for the windowed shell.

use gba_native::frontend::ui::SavesAction;
use gba_native::input::Key;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, KeyEvent, TouchPhase};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::ui;

use super::Frontend;

impl Frontend {
    pub(super) fn handle_key(&mut self, event_loop: &ActiveEventLoop, event: KeyEvent) {
        let pressed = event.state == ElementState::Pressed;
        let PhysicalKey::Code(code) = event.physical_key else {
            return;
        };

        if pressed && !event.repeat {
            if self.saves.is_open() {
                match code {
                    KeyCode::F2 | KeyCode::Escape => self.saves.close(),
                    KeyCode::ArrowDown => self.saves.select(1),
                    KeyCode::ArrowUp => self.saves.select(-1),
                    KeyCode::KeyN => self.saves.apply(SavesAction::New),
                    KeyCode::KeyU => self.saves.apply(SavesAction::Use),
                    KeyCode::KeyD => self.saves.apply(SavesAction::Delete),
                    KeyCode::Enter => self.saves.apply(SavesAction::MakeDefault),
                    _ => {}
                }
                if let Some(status) = self.saves.status() {
                    eprintln!("saves: {status}");
                }
                self.rebuild_display();
                return;
            }
            match code {
                KeyCode::F2 => {
                    self.saves.open();
                    self.rebuild_display();
                    return;
                }
                KeyCode::KeyP => {
                    self.play_tone();
                    return;
                }
                KeyCode::F1 => {
                    self.show_settings = !self.show_settings;
                    self.rebuild_display();
                    return;
                }
                KeyCode::F11 => {
                    self.toggle_fullscreen();
                    return;
                }
                KeyCode::Enter if self.modifiers.alt_key() => {
                    self.toggle_fullscreen();
                    return;
                }
                KeyCode::Escape => {
                    if self.show_settings {
                        self.show_settings = false;
                        self.rebuild_display();
                    } else {
                        self.persist_settings();
                        event_loop.exit();
                    }
                    return;
                }
                KeyCode::KeyM => {
                    self.settings.muted = !self.settings.muted;
                    self.persist_settings();
                    self.rebuild_display();
                    println!("audio: muted={}", self.settings.muted);
                    return;
                }
                KeyCode::Minus | KeyCode::Equal => {
                    let delta = if code == KeyCode::Minus { -10 } else { 10 };
                    let volume = i16::from(self.settings.master_volume) + delta;
                    self.settings.master_volume = volume.clamp(0, 100) as u8;
                    self.persist_settings();
                    self.rebuild_display();
                    println!("audio: volume={}%", self.settings.master_volume);
                    return;
                }
                _ => {}
            }
        }

        let Some(key) = Key::from_code(&format!("{code:?}")) else {
            return;
        };
        if pressed {
            if !self.pressed_keys.contains(&key) {
                self.pressed_keys.push(key);
            }
        } else {
            self.pressed_keys.retain(|candidate| *candidate != key);
        }
        self.sync_input();
    }

    fn toggle_fullscreen(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let next = window.fullscreen().is_none();
        window.set_fullscreen(if next {
            Some(winit::window::Fullscreen::Borderless(None))
        } else {
            None
        });
        self.settings.fullscreen = next;
        self.persist_settings();
        self.show_settings = false;
        self.rebuild_display();
        println!("ui: fullscreen={next}");
    }

    pub(super) fn on_cursor(&mut self, position: PhysicalPosition<f64>) {
        let width = self.surface.0.max(1) as f64;
        let height = self.surface.1.max(1) as f64;
        self.pointer = ((position.x / width) as f32, (position.y / height) as f32);
    }

    pub(super) fn on_pointer(&mut self, state: ElementState) {
        match state {
            ElementState::Pressed => {
                self.pointer_button = ui::hit_test(self.pointer.0, self.pointer.1);
            }
            ElementState::Released => self.pointer_button = None,
        }
        self.sync_input();
    }

    pub(super) fn on_touch(&mut self, phase: TouchPhase, location: PhysicalPosition<f64>) {
        match phase {
            TouchPhase::Started | TouchPhase::Moved => {
                let width = self.surface.0.max(1) as f64;
                let height = self.surface.1.max(1) as f64;
                self.pointer = ((location.x / width) as f32, (location.y / height) as f32);
                self.pointer_button = ui::hit_test(self.pointer.0, self.pointer.1);
            }
            TouchPhase::Ended | TouchPhase::Cancelled => self.pointer_button = None,
        }
        self.sync_input();
    }

    fn sync_input(&mut self) {
        let mut mask = 0u16;
        for key in &self.pressed_keys {
            mask |= self.settings.bindings.mask_for_key(*key);
        }
        if let Some(button) = self.pointer_button {
            mask |= button.bit();
        }
        if mask != self.input_mask {
            self.input_mask = mask;
            self.rebuild_display();
        }
    }

    pub(super) fn rebuild_display(&mut self) {
        let display = self.compose();
        let Some(pixels) = self.pixels.as_mut() else {
            return;
        };
        let frame = pixels.frame_mut();
        if frame.len() == display.rgba().len() {
            frame.copy_from_slice(display.rgba());
            self.request_redraw();
        }
    }
}
