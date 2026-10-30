//! Normalised touch layout for windowed GBA-shell rendering.
//!
//! The layout is expressed in fractions of the shell surface, so both native
//! and browser frontends can scale it to any window size. Fullscreen mode hides
//! the shell and shows only the screen; windowed mode uses these rects. This is
//! a presentation model, not a claim about hardware button geometry.

use super::Button;

/// Axis-aligned rectangle in normalised surface coordinates (`0.0..=1.0`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.width && y <= self.y + self.height
    }

    #[must_use]
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// Convert to physical pixels for a surface size, rounding outward so a
    /// scaled control never loses its last row/column.
    #[must_use]
    pub fn pixels(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        let x = (self.x * width as f32).floor().max(0.0) as u32;
        let y = (self.y * height as f32).floor().max(0.0) as u32;
        let right = ((self.x + self.width) * width as f32).ceil() as u32;
        let bottom = ((self.y + self.height) * height as f32).ceil() as u32;
        (
            x.min(width),
            y.min(height),
            right.saturating_sub(x).min(width.saturating_sub(x)),
            bottom.saturating_sub(y).min(height.saturating_sub(y)),
        )
    }
}

/// Screen and touch/button rects for one shell orientation.
#[derive(Clone, Debug, PartialEq)]
pub struct GbaLayout {
    pub screen: Rect,
    pub controls: Vec<(Button, Rect)>,
}

/// Shell profile selected from the surface aspect ratio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchLayout {
    Landscape,
    Portrait,
}

impl TouchLayout {
    /// Choose the profile: wide surfaces use the landscape shell.
    #[must_use]
    pub fn for_aspect(width: u32, height: u32) -> Self {
        if width >= height {
            Self::Landscape
        } else {
            Self::Portrait
        }
    }

    /// Build the normalised layout for this profile.
    #[must_use]
    pub fn layout(self) -> GbaLayout {
        match self {
            Self::Landscape => Self::landscape(),
            Self::Portrait => Self::portrait(),
        }
    }

    #[must_use]
    pub fn landscape() -> GbaLayout {
        GbaLayout {
            screen: Rect::new(0.20, 0.0667, 0.60, 0.5333),
            controls: vec![
                (Button::L, Rect::new(0.02, 0.04, 0.14, 0.07)),
                (Button::R, Rect::new(0.84, 0.04, 0.14, 0.07)),
                (Button::Up, Rect::new(0.10, 0.64, 0.08, 0.12)),
                (Button::Down, Rect::new(0.10, 0.80, 0.08, 0.12)),
                (Button::Left, Rect::new(0.02, 0.72, 0.08, 0.12)),
                (Button::Right, Rect::new(0.18, 0.72, 0.08, 0.12)),
                (Button::B, Rect::new(0.76, 0.72, 0.10, 0.14)),
                (Button::A, Rect::new(0.88, 0.66, 0.10, 0.14)),
                (Button::Select, Rect::new(0.36, 0.86, 0.09, 0.10)),
                (Button::Start, Rect::new(0.52, 0.86, 0.09, 0.10)),
            ],
        }
    }

    #[must_use]
    pub fn portrait() -> GbaLayout {
        GbaLayout {
            screen: Rect::new(0.06, 0.08, 0.88, 0.44),
            controls: vec![
                (Button::L, Rect::new(0.02, 0.03, 0.12, 0.05)),
                (Button::R, Rect::new(0.86, 0.03, 0.12, 0.05)),
                (Button::Up, Rect::new(0.18, 0.60, 0.10, 0.08)),
                (Button::Down, Rect::new(0.18, 0.74, 0.10, 0.08)),
                (Button::Left, Rect::new(0.08, 0.67, 0.10, 0.08)),
                (Button::Right, Rect::new(0.28, 0.67, 0.10, 0.08)),
                (Button::B, Rect::new(0.62, 0.66, 0.11, 0.09)),
                (Button::A, Rect::new(0.77, 0.59, 0.11, 0.09)),
                (Button::Select, Rect::new(0.36, 0.88, 0.10, 0.06)),
                (Button::Start, Rect::new(0.54, 0.88, 0.10, 0.06)),
            ],
        }
    }
}

impl GbaLayout {
    /// Resolve a normalised pointer position to a guest button.
    ///
    /// The screen area is deliberately not a button: a tap on the game image
    /// never produces guest input. Overlapping controls resolve in declaration
    /// order, which is why the default rects do not overlap.
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32) -> Option<Button> {
        if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
            return None;
        }
        self.controls
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(button, _)| *button)
    }

    #[must_use]
    pub fn is_on_screen(&self, x: f32, y: f32) -> bool {
        self.screen.contains(x, y)
    }

    /// Physical rect for a button, if the layout defines it.
    #[must_use]
    pub fn button_pixels(
        &self,
        button: Button,
        width: u32,
        height: u32,
    ) -> Option<(u32, u32, u32, u32)> {
        self.controls
            .iter()
            .find(|(candidate, _)| *candidate == button)
            .map(|(_, rect)| rect.pixels(width, height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_hits(layout: &GbaLayout) {
        for button in Button::ALL {
            let rect = layout
                .controls
                .iter()
                .find(|(candidate, _)| *candidate == button)
                .map(|(_, rect)| *rect)
                .expect("every button has a rect");
            let (x, y) = rect.center();
            assert_eq!(layout.hit_test(x, y), Some(button), "{}", button.name());
        }
    }

    #[test]
    fn both_profiles_hit_every_button_at_its_centre() {
        assert_hits(&TouchLayout::landscape());
        assert_hits(&TouchLayout::portrait());
    }

    #[test]
    fn screen_and_outside_taps_are_not_buttons() {
        for layout in [TouchLayout::landscape(), TouchLayout::portrait()] {
            let (x, y) = layout.screen.center();
            assert!(layout.is_on_screen(x, y));
            assert_eq!(layout.hit_test(x, y), None);
            assert_eq!(layout.hit_test(-0.01, 0.5), None);
            assert_eq!(layout.hit_test(0.5, 1.01), None);
        }
    }

    #[test]
    fn aspect_selection_and_pixel_conversion() {
        assert_eq!(TouchLayout::for_aspect(1920, 1080), TouchLayout::Landscape);
        assert_eq!(TouchLayout::for_aspect(390, 844), TouchLayout::Portrait);
        let layout = TouchLayout::portrait();
        let (x, y, w, h) = layout.screen.pixels(390, 844);
        assert!(x < 390 && y < 844 && w > 0 && h > 0);
        let (ax, ay, aw, ah) = layout.button_pixels(Button::A, 390, 844).unwrap();
        assert!(ax + aw <= 390 && ay + ah <= 844);
        assert!(layout.button_pixels(Button::Start, 390, 844).is_some());
    }

    #[test]
    fn controls_do_not_overlap_the_screen_or_each_other() {
        for layout in [TouchLayout::landscape(), TouchLayout::portrait()] {
            for (button, rect) in &layout.controls {
                assert!(
                    !rects_overlap(rect, &layout.screen),
                    "{} overlaps the screen",
                    button.name()
                );
                for (other, other_rect) in &layout.controls {
                    if button != other {
                        assert!(
                            !rects_overlap(rect, other_rect),
                            "{} overlaps {}",
                            button.name(),
                            other.name()
                        );
                    }
                }
            }
        }
    }

    fn rects_overlap(a: &Rect, b: &Rect) -> bool {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    }
}
