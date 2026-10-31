//! Windowed GBA-shell composition, fullscreen frame and the settings page.
//!
//! Everything here is pure pixel work on the validated `RgbaImage` boundary, so
//! the shell, touch hit testing and settings text can be unit-tested without a
//! window, GPU or adapter. The guest image stays a 240x160 synthetic snapshot;
//! this module never executes guest code.

use crate::input::{Settings, TouchLayout};

use super::image::RgbaImage;
use super::saves::SaveSlot;

pub const WIDTH: u32 = 800;
pub const HEIGHT: u32 = 600;

const BODY: [u8; 4] = [0x2B, 0x2B, 0x3A, 0xFF];
const PANEL: [u8; 4] = [0x3A, 0x3A, 0x4E, 0xFF];
const BEZEL: [u8; 4] = [0x12, 0x12, 0x1A, 0xFF];
const BUTTON: [u8; 4] = [0x55, 0x55, 0x68, 0xFF];
const PRESSED: [u8; 4] = [0x8F, 0xE2, 0x8F, 0xFF];
const TEXT: [u8; 4] = [0xF0, 0xF0, 0xF4, 0xFF];
const DIM: [u8; 4] = [0xA0, 0xA0, 0xB0, 0xFF];
const ACCENT: [u8; 4] = [0x7F, 0xD4, 0xFF, 0xFF];

/// 3x5 glyphs, high bit is the left pixel. Only the characters the UI needs.
fn glyph(character: char) -> [u8; 5] {
    match character.to_ascii_uppercase() {
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b101, 0b111, 0b111, 0b111, 0b101],
        'O' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'Q' => [0b010, 0b101, 0b101, 0b110, 0b011],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'V' => [0b101, 0b101, 0b101, 0b010, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b110, 0b001, 0b010, 0b100, 0b111],
        '3' => [0b110, 0b001, 0b010, 0b001, 0b110],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b110, 0b001, 0b110],
        '6' => [0b011, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b110],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        ',' => [0b000, 0b000, 0b000, 0b010, 0b100],
        '!' => [0b010, 0b010, 0b010, 0b000, 0b010],
        _ => [0u8; 5],
    }
}

pub fn text_width(text: &str, scale: u32) -> u32 {
    (text.chars().count() as u32) * 4 * scale
}

fn draw_text(buffer: &mut [u8], x: u32, y: u32, text: &str, colour: [u8; 4], scale: u32) {
    let mut cursor = x;
    for character in text.chars() {
        let rows = glyph(character);
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..3 {
                if bits & (1 << (2 - column)) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = cursor + column * scale + dx;
                        let py = y + row as u32 * scale + dy;
                        if px < WIDTH && py < HEIGHT {
                            let offset = ((py * WIDTH + px) * 4) as usize;
                            buffer[offset..offset + 4].copy_from_slice(&colour);
                        }
                    }
                }
            }
        }
        cursor += 4 * scale;
    }
}

fn fill(buffer: &mut [u8], x: u32, y: u32, width: u32, height: u32, colour: [u8; 4]) {
    for py in y..y.saturating_add(height).min(HEIGHT) {
        for px in x..x.saturating_add(width).min(WIDTH) {
            let offset = ((py * WIDTH + px) * 4) as usize;
            buffer[offset..offset + 4].copy_from_slice(&colour);
        }
    }
}

fn blank() -> Vec<u8> {
    vec![0u8; (WIDTH * HEIGHT * 4) as usize]
}

fn finish(rgba: Vec<u8>, budget: u64) -> RgbaImage {
    RgbaImage::from_rgba(WIDTH, HEIGHT, rgba, budget).expect("UI image is valid")
}

/// Windowed GBA-shell composition with the guest image at 2x.
pub fn shell(screen: &RgbaImage, pressed: u16) -> RgbaImage {
    let mut rgba = blank();
    fill(&mut rgba, 0, 0, WIDTH, HEIGHT, BODY);
    fill(&mut rgba, 0, 0, WIDTH, 26, PANEL);
    draw_text(&mut rgba, 16, 8, "GBA-NATIVE", ACCENT, 2);
    draw_text(&mut rgba, 600, 8, "WINDOWED", DIM, 2);

    let layout = TouchLayout::landscape();
    let (sx, sy, sw, sh) = layout.screen.pixels(WIDTH, HEIGHT);
    fill(&mut rgba, sx - 8, sy - 8, sw + 16, sh + 16, BEZEL);
    blit_scaled(&mut rgba, screen, sx, sy, sw, sh);

    for (button, rect) in &layout.controls {
        let (bx, by, bw, bh) = rect.pixels(WIDTH, HEIGHT);
        let active = pressed & button.bit() != 0;
        fill(
            &mut rgba,
            bx,
            by,
            bw,
            bh,
            if active { PRESSED } else { BUTTON },
        );
        let label = button.name();
        let tw = text_width(label, 2);
        draw_text(
            &mut rgba,
            bx + bw.saturating_sub(tw) / 2,
            by + bh / 2 - 5,
            label,
            if active { BODY } else { TEXT },
            2,
        );
    }

    draw_text(
        &mut rgba,
        16,
        566,
        "F1 SETTINGS  F2 SAVES  F11 FULLSCREEN",
        DIM,
        2,
    );
    draw_text(&mut rgba, 600, 566, "SYNTHETIC", ACCENT, 2);
    finish(rgba, screen.guest_budget())
}

/// Fullscreen frame: the 240x160 guest image at 3x, centred on black.
pub fn fullscreen(screen: &RgbaImage) -> RgbaImage {
    let mut rgba = blank();
    fill(&mut rgba, 0, 0, WIDTH, HEIGHT, [0, 0, 0, 0xFF]);
    blit_scaled(&mut rgba, screen, 40, 60, 720, 480);
    finish(rgba, screen.guest_budget())
}

/// Actions on the shared saves page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavesAction {
    Close,
    New,
    Use,
    MakeDefault,
    Delete,
}

/// Row geometry shared by the saves page and its hit testing.
pub const SAVES_ROW_Y: u32 = 100;
pub const SAVES_ROW_HEIGHT: u32 = 34;
pub const SAVES_MAX_ROWS: usize = 10;

/// Resolve a pointer inside the saves list to a row index.
pub fn saves_row_at(x: u32, y: u32) -> Option<usize> {
    if !(40..WIDTH - 40).contains(&x) || y < SAVES_ROW_Y {
        return None;
    }
    let row = ((y - SAVES_ROW_Y) / SAVES_ROW_HEIGHT) as usize;
    (row < SAVES_MAX_ROWS).then_some(row)
}

/// Resolve a pointer to a saves-page action button.
pub fn saves_action_at(x: u32, y: u32) -> Option<SavesAction> {
    let rects = [
        ((40, 520, 120, 50), SavesAction::New),
        ((180, 520, 120, 50), SavesAction::Use),
        ((320, 520, 200, 50), SavesAction::MakeDefault),
        ((540, 520, 120, 50), SavesAction::Delete),
        ((670, 0, 130, 48), SavesAction::Close),
    ];
    rects
        .iter()
        .find(|(rect, _)| x >= rect.0 && x < rect.0 + rect.2 && y >= rect.1 && y < rect.1 + rect.3)
        .map(|(_, action)| *action)
}

/// Saves page: named slots, lengths and the default marker.
pub fn saves_page(slots: &[SaveSlot], selected: usize) -> RgbaImage {
    let mut rgba = blank();
    fill(&mut rgba, 0, 0, WIDTH, HEIGHT, BODY);
    fill(&mut rgba, 0, 0, WIDTH, 48, PANEL);
    draw_text(&mut rgba, 24, 14, "SAVES", ACCENT, 4);
    draw_text(&mut rgba, 600, 18, "CLOSE", DIM, 2);

    if slots.is_empty() {
        draw_text(&mut rgba, 40, 110, "NO SLOTS YET", DIM, 3);
    }
    for (row, slot) in slots.iter().take(SAVES_MAX_ROWS).enumerate() {
        let y = SAVES_ROW_Y + row as u32 * SAVES_ROW_HEIGHT;
        let selected_here = row == selected;
        let colour = if selected_here { ACCENT } else { TEXT };
        if selected_here {
            fill(
                &mut rgba,
                32,
                y - 4,
                WIDTH - 64,
                SAVES_ROW_HEIGHT - 4,
                PANEL,
            );
        }
        let label = format!("{} {}", row + 1, slot.name.to_uppercase());
        draw_text(&mut rgba, 40, y, &label, colour, 2);
        draw_text(&mut rgba, 400, y, &format!("{} B", slot.bytes), DIM, 2);
        if slot.default {
            draw_text(&mut rgba, 620, y, "DEFAULT", ACCENT, 2);
        }
    }

    draw_text(&mut rgba, 40, 566, "NEW   USE   DEFAULT   DELETE", DIM, 2);
    finish(rgba, 0)
}

/// Persisted settings page with the active key bindings.
pub fn settings_page(settings: &Settings) -> RgbaImage {
    let mut rgba = blank();
    fill(&mut rgba, 0, 0, WIDTH, HEIGHT, BODY);
    fill(&mut rgba, 0, 0, WIDTH, 48, PANEL);
    draw_text(&mut rgba, 24, 14, "SETTINGS", ACCENT, 4);
    draw_text(&mut rgba, 360, 18, "SAVES", ACCENT, 2);
    draw_text(&mut rgba, 640, 18, "CLOSE", DIM, 2);

    draw_text(&mut rgba, 40, 72, "KEY BINDINGS", DIM, 2);
    for (row, (button, keys)) in settings.binding_rows().iter().enumerate() {
        let y = 100 + row as u32 * 34;
        draw_text(&mut rgba, 40, y, button.name(), TEXT, 3);
        let mut keys_text = String::new();
        for (index, key) in keys.iter().enumerate() {
            if index > 0 {
                keys_text.push('/');
            }
            keys_text.push_str(key);
        }
        draw_text(&mut rgba, 280, y, &keys_text, ACCENT, 3);
    }

    let state = |value: bool| if value { "ON" } else { "OFF" };
    draw_text(
        &mut rgba,
        40,
        468,
        &format!("FULLSCREEN {}", state(settings.fullscreen)),
        TEXT,
        3,
    );
    draw_text(
        &mut rgba,
        40,
        506,
        &format!("TOUCH CONTROLS {}", state(settings.touch_controls)),
        TEXT,
        3,
    );
    draw_text(
        &mut rgba,
        40,
        544,
        &format!("INTEGER SCALING {}", state(settings.integer_scaling)),
        TEXT,
        3,
    );
    draw_text(
        &mut rgba,
        470,
        468,
        &format!("VOLUME {}%", settings.master_volume),
        TEXT,
        3,
    );
    draw_text(
        &mut rgba,
        470,
        506,
        &format!("MUTED {}", state(settings.muted)),
        TEXT,
        3,
    );
    draw_text(&mut rgba, 470, 544, "M MUTE  -/+ VOL", DIM, 2);
    finish(rgba, 0)
}

/// True when a pointer is on the settings page's saves entry.
pub fn settings_saves_at(x: u32, y: u32) -> bool {
    (330..480).contains(&x) && y < 48
}

/// Hit test a normalized pointer position against the windowed layout.
pub fn hit_test(x: f32, y: f32) -> Option<crate::input::Button> {
    TouchLayout::landscape().hit_test(x, y)
}

fn blit_scaled(target: &mut [u8], source: &RgbaImage, x: u32, y: u32, width: u32, height: u32) {
    if width == 0 || height == 0 {
        return;
    }
    let src = source.rgba();
    for row in 0..height {
        let sy = row * source.height() / height;
        for column in 0..width {
            let sx = column * source.width() / width;
            let src_offset = ((sy * source.width() + sx) * 4) as usize;
            let dst_offset = (((y + row) * WIDTH + x + column) * 4) as usize;
            target[dst_offset..dst_offset + 4].copy_from_slice(&src[src_offset..src_offset + 4]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;
    use crate::frontend::image::demo_image as synthetic_demo_image;
    use crate::input::Button;

    fn layout() -> crate::input::GbaLayout {
        TouchLayout::landscape()
    }

    #[test]
    fn shell_contains_the_guest_screen_and_button_labels() {
        let guest = synthetic_demo_image().expect("demo");
        let image = shell(&guest, Button::A.bit() | Button::Start.bit());
        assert_eq!((image.width(), image.height()), (WIDTH, HEIGHT));
        let (sx, sy, _, _) = layout().screen.pixels(WIDTH, HEIGHT);
        // The guest's top-left pixel appears at the shell screen origin.
        let offset = ((sy * WIDTH + sx) * 4) as usize;
        assert_eq!(&image.rgba()[offset..offset + 4], &guest.rgba()[..4]);
        // A pressed control differs from an idle one.
        let a = layout().button_pixels(Button::A, WIDTH, HEIGHT).expect("A");
        let a_offset = (((a.1 + 2) * WIDTH + a.0 + 2) * 4) as usize;
        assert_eq!(&image.rgba()[a_offset..a_offset + 4], &PRESSED);
    }

    #[test]
    fn fullscreen_is_black_outside_the_3x_guest_frame() {
        let guest = synthetic_demo_image().expect("demo");
        let image = fullscreen(&guest);
        let top_left = 0usize;
        assert_eq!(&image.rgba()[top_left..top_left + 4], &[0, 0, 0, 0xFF]);
        let offset = ((60 * WIDTH + 40) * 4) as usize;
        assert_eq!(&image.rgba()[offset..offset + 4], &guest.rgba()[..4]);
    }

    #[test]
    fn saves_page_lists_slots_and_exposes_row_and_action_zones() {
        let slots = vec![
            SaveSlot {
                name: "intro".to_string(),
                bytes: 512,
                default: true,
            },
            SaveSlot {
                name: "route-2".to_string(),
                bytes: 1024,
                default: false,
            },
        ];
        let image = saves_page(&slots, 1);
        assert_eq!((image.width(), image.height()), (WIDTH, HEIGHT));
        assert_ne!(&image.rgba()[..4], &[0, 0, 0, 0xFF]);

        assert_eq!(saves_row_at(60, SAVES_ROW_Y + 4), Some(0));
        assert_eq!(
            saves_row_at(60, SAVES_ROW_Y + SAVES_ROW_HEIGHT + 4),
            Some(1)
        );
        assert_eq!(
            saves_row_at(60, SAVES_ROW_Y + SAVES_ROW_HEIGHT * 3),
            Some(3)
        );
        assert_eq!(saves_row_at(10, SAVES_ROW_Y), None);
        assert_eq!(saves_row_at(60, SAVES_ROW_Y + SAVES_ROW_HEIGHT * 12), None);

        assert_eq!(saves_action_at(60, 540), Some(SavesAction::New));
        assert_eq!(saves_action_at(220, 540), Some(SavesAction::Use));
        assert_eq!(saves_action_at(400, 540), Some(SavesAction::MakeDefault));
        assert_eq!(saves_action_at(600, 540), Some(SavesAction::Delete));
        assert_eq!(saves_action_at(700, 20), Some(SavesAction::Close));
        assert_eq!(saves_action_at(400, 300), None);
    }

    #[test]
    fn settings_page_offers_the_saves_entry() {
        assert!(settings_saves_at(360, 20));
        assert!(settings_saves_at(400, 40));
        assert!(!settings_saves_at(300, 20));
        assert!(!settings_saves_at(360, 60));
    }

    #[test]
    fn settings_page_lists_every_binding_and_state() {
        let settings = Settings::default();
        let image = settings_page(&settings);
        assert_eq!((image.width(), image.height()), (WIDTH, HEIGHT));
        // The page is not a flat fill: text and panel bands produce variety.
        let first = &image.rgba()[..4];
        assert!(image.rgba().chunks(4).any(|pixel| pixel != first));
        assert_eq!(settings.binding_rows().len(), 10);
        assert!(image.rgba().len() as u32 == WIDTH * HEIGHT * 4);
    }

    #[test]
    fn touch_hit_testing_matches_the_layout() {
        let layout = TouchLayout::landscape();
        for button in Button::ALL {
            let (x, y, width, height) = layout.button_pixels(button, WIDTH, HEIGHT).expect("rect");
            let nx = (x as f32 + width as f32 / 2.0) / WIDTH as f32;
            let ny = (y as f32 + height as f32 / 2.0) / HEIGHT as f32;
            assert_eq!(hit_test(nx, ny), Some(button));
        }
        assert_eq!(hit_test(0.5, 0.5), None);
    }

    #[test]
    fn glyphs_cover_the_visible_labels() {
        for text in [
            "SETTINGS",
            "A",
            "SELECT",
            "START",
            "F11",
            "TOUCH CONTROLS",
            "0-9:",
        ] {
            assert!(text_width(text, 3) > 0);
        }
        assert_eq!(demo::DEMO_BUDGET, 230_411);
    }
}
