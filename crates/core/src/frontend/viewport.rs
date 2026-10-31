//! Integer nearest-neighbour viewport shared by every GUI target.
//!
//! The scale is the largest integer that fits both axes, but at least 1, so a
//! surface smaller than the guest image clips instead of inventing a
//! fractional filter. Zero surface dimensions (minimised or not yet sized)
//! yield an empty viewport. The remaining surface is black letterbox.

/// Integer nearest-neighbour placement of the guest image inside a surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub scale: u32,
    pub clipped: bool,
}

pub fn compute_viewport(
    content_width: u32,
    content_height: u32,
    surface_width: u32,
    surface_height: u32,
) -> Viewport {
    if content_width == 0 || content_height == 0 || surface_width == 0 || surface_height == 0 {
        return Viewport {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            scale: 0,
            clipped: false,
        };
    }

    let scale = (surface_width / content_width)
        .min(surface_height / content_height)
        .max(1);
    let scaled_width = content_width.saturating_mul(scale);
    let scaled_height = content_height.saturating_mul(scale);
    let width = scaled_width.min(surface_width);
    let height = scaled_height.min(surface_height);
    Viewport {
        x: (surface_width - width) / 2,
        y: (surface_height - height) / 2,
        width,
        height,
        scale,
        clipped: scaled_width > surface_width || scaled_height > surface_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_is_three_to_two_at_three_x() {
        assert_eq!(
            compute_viewport(240, 160, 720, 480),
            Viewport {
                x: 0,
                y: 0,
                width: 720,
                height: 480,
                scale: 3,
                clipped: false
            }
        );
    }

    #[test]
    fn viewport_centres_letterbox_on_both_axes() {
        assert_eq!(
            compute_viewport(240, 160, 800, 600),
            Viewport {
                x: 40,
                y: 60,
                width: 720,
                height: 480,
                scale: 3,
                clipped: false
            }
        );
        assert_eq!(
            compute_viewport(240, 160, 480, 720),
            Viewport {
                x: 0,
                y: 200,
                width: 480,
                height: 320,
                scale: 2,
                clipped: false
            }
        );
    }

    #[test]
    fn viewport_uses_maximum_integer_scale() {
        assert_eq!(
            compute_viewport(240, 160, 1920, 1080),
            Viewport {
                x: 240,
                y: 60,
                width: 1440,
                height: 960,
                scale: 6,
                clipped: false
            }
        );
        assert_eq!(
            compute_viewport(240, 160, 3840, 2160),
            Viewport {
                x: 360,
                y: 40,
                width: 3120,
                height: 2080,
                scale: 13,
                clipped: false
            }
        );
    }

    #[test]
    fn viewport_minimum_and_minimised_cases_are_explicit() {
        assert_eq!(
            compute_viewport(240, 160, 240, 160),
            Viewport {
                x: 0,
                y: 0,
                width: 240,
                height: 160,
                scale: 1,
                clipped: false
            }
        );
        let clipped = compute_viewport(240, 160, 100, 100);
        assert_eq!(
            clipped,
            Viewport {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
                scale: 1,
                clipped: true
            }
        );
        assert_eq!(
            compute_viewport(240, 160, 0, 480),
            Viewport {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                scale: 0,
                clipped: false
            }
        );
    }
}
