//! Clean image and viewport boundary between the std-only core and the GPU
//! frontend.
//!
//! The core produces packed opaque `0x00RRGGBB` pixels. This module validates
//! dimensions and buffer length, converts to the RGBA byte layout consumed by
//! Pixels/WGPU, and mirrors the integer nearest-neighbour letterbox geometry.
//! It contains no GPU calls, so every rule below is unit-testable without a
//! window or adapter.

use std::fmt;

use gba_native::demo;
use gba_native::display;

/// Refuse accidental giant conversions even before the GPU is involved.
pub const MAX_IMAGE_PIXELS: u64 = 4096 * 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageError {
    ZeroDimension { width: u32, height: u32 },
    TooLarge { width: u32, height: u32 },
    PixelCount { width: u32, height: u32, len: usize },
    NonOpaquePixel { index: usize, value: u32 },
}

impl fmt::Display for ImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDimension { width, height } => write!(
                formatter,
                "image dimensions must be non-zero, got {width}x{height}"
            ),
            Self::TooLarge { width, height } => write!(
                formatter,
                "image {width}x{height} exceeds the {} pixel conversion limit",
                MAX_IMAGE_PIXELS
            ),
            Self::PixelCount { width, height, len } => write!(
                formatter,
                "image buffer length {len} does not match {width}x{height}"
            ),
            Self::NonOpaquePixel { index, value } => write!(
                formatter,
                "pixel {index} is 0x{value:08X}; RGB888 input must have a zero high byte"
            ),
        }
    }
}

impl std::error::Error for ImageError {}

/// A validated RGBA8888 image plus the identity of the RGB888 input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    rgb_sha256: String,
    guest_budget: u64,
}

impl RgbaImage {
    /// Build a validated RGBA image from already-expanded channels.
    ///
    /// Used by the windowed shell/settings compositor. The RGB digest is
    /// recomputed from the buffer, never caller-supplied.
    pub(crate) fn from_rgba(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        guest_budget: u64,
    ) -> Result<Self, ImageError> {
        if width == 0 || height == 0 {
            return Err(ImageError::ZeroDimension { width, height });
        }
        let pixels = u64::from(width) * u64::from(height);
        if pixels > MAX_IMAGE_PIXELS {
            return Err(ImageError::TooLarge { width, height });
        }
        if rgba.len() != pixels as usize * 4 {
            return Err(ImageError::PixelCount {
                width,
                height,
                len: rgba.len(),
            });
        }
        let mut rgb = Vec::with_capacity(pixels as usize);
        for (index, pixel) in rgba.chunks(4).enumerate() {
            if pixel[3] != 0xFF {
                return Err(ImageError::NonOpaquePixel {
                    index,
                    value: u32::from_be_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]),
                });
            }
            rgb.push(u32::from(pixel[0]) << 16 | u32::from(pixel[1]) << 8 | u32::from(pixel[2]));
        }
        let digest = gba_native::sha256::hex(&gba_native::sha256::digest(
            &rgb.iter()
                .flat_map(|pixel| [(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8])
                .collect::<Vec<u8>>(),
        ));
        Ok(Self {
            width,
            height,
            rgba,
            rgb_sha256: digest,
            guest_budget,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// SHA-256 of the core's three-byte RGB stream, not of the RGBA expansion.
    pub fn rgb_sha256(&self) -> &str {
        &self.rgb_sha256
    }

    /// The existing synthetic demo instruction budget. This is not frame time.
    pub fn guest_budget(&self) -> u64 {
        self.guest_budget
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    Core(String),
    Image(ImageError),
    UnexpectedDimensions { width: u32, height: u32 },
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(message) => {
                write!(formatter, "synthetic demo execution failed: {message}")
            }
            Self::Image(error) => write!(formatter, "synthetic image conversion failed: {error}"),
            Self::UnexpectedDimensions { width, height } => write!(
                formatter,
                "synthetic demo produced {width}x{height}, expected 240x160"
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// Convert validated RGB888 words into opaque RGBA8888 bytes.
///
/// The alpha byte is always `0xFF` because the core bitmap has no alpha
/// channel. Values with a non-zero high byte are rejected instead of silently
/// discarding information.
pub fn convert_rgb888_to_rgba(width: u32, height: u32, rgb: &[u32]) -> Result<Vec<u8>, ImageError> {
    if width == 0 || height == 0 {
        return Err(ImageError::ZeroDimension { width, height });
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_IMAGE_PIXELS {
        return Err(ImageError::TooLarge { width, height });
    }
    if rgb.len() != pixels as usize {
        return Err(ImageError::PixelCount {
            width,
            height,
            len: rgb.len(),
        });
    }

    let mut rgba = Vec::with_capacity(rgb.len() * 4);
    for (index, pixel) in rgb.iter().copied().enumerate() {
        if pixel & 0xFF00_0000 != 0 {
            return Err(ImageError::NonOpaquePixel {
                index,
                value: pixel,
            });
        }
        rgba.push((pixel >> 16) as u8);
        rgba.push((pixel >> 8) as u8);
        rgba.push(pixel as u8);
        rgba.push(0xFF);
    }
    Ok(rgba)
}

/// Execute the existing synthetic demo through the core's interpreter and
/// snapshot its bitmap. Nothing here reads a ROM, BIOS or save.
pub fn synthetic_demo_image() -> Result<RgbaImage, LoadError> {
    let (_cpu, bus) =
        demo::run_demo_reference().map_err(|error| LoadError::Core(error.to_string()))?;
    let image: display::Image =
        display::snapshot(&bus).map_err(|error| LoadError::Core(error.to_string()))?;
    if image.width != display::WIDTH || image.height != display::HEIGHT {
        return Err(LoadError::UnexpectedDimensions {
            width: image.width,
            height: image.height,
        });
    }
    let rgba = convert_rgb888_to_rgba(image.width, image.height, &image.pixels)
        .map_err(LoadError::Image)?;
    Ok(RgbaImage {
        width: image.width,
        height: image.height,
        rgba,
        rgb_sha256: image.sha256_hex(),
        guest_budget: demo::DEMO_BUDGET,
    })
}

/// Integer nearest-neighbour viewport inside a physical surface.
///
/// This mirrors Pixels' `ScalingMode::PixelPerfect`: scale is the largest
/// integer fitting both axes, but at least 1. A surface smaller than the guest
/// image therefore clips instead of inventing a fractional filter. Zero surface
/// dimensions (minimised) yield an empty viewport. Remaining surface area is
/// black letterbox, because the caller sets Pixels' clear colour to black.
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
    fn conversion_preserves_known_colours_and_sets_opaque_alpha() {
        let rgba = convert_rgb888_to_rgba(3, 1, &[0x0011_2233, 0x00FF_FFFF, 0x0000_0000])
            .expect("valid conversion");
        assert_eq!(
            rgba,
            vec![
                0x11, 0x22, 0x33, 0xFF, // known colour
                0xFF, 0xFF, 0xFF, 0xFF, // white
                0x00, 0x00, 0x00, 0xFF, // black stays opaque
            ]
        );
    }

    #[test]
    fn conversion_rejects_wrong_lengths_dimensions_and_alpha_bits() {
        assert_eq!(
            convert_rgb888_to_rgba(2, 2, &[0, 0, 0]),
            Err(ImageError::PixelCount {
                width: 2,
                height: 2,
                len: 3
            })
        );
        assert_eq!(
            convert_rgb888_to_rgba(0, 1, &[]),
            Err(ImageError::ZeroDimension {
                width: 0,
                height: 1
            })
        );
        assert_eq!(
            convert_rgb888_to_rgba(1, 1, &[0x0100_0000]),
            Err(ImageError::NonOpaquePixel {
                index: 0,
                value: 0x0100_0000
            })
        );
        assert_eq!(
            convert_rgb888_to_rgba(5000, 5000, &[]),
            Err(ImageError::TooLarge {
                width: 5000,
                height: 5000
            })
        );
    }

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
