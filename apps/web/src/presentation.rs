//! Browser-side validation of the synthetic core bitmap.
//!
//! The std-only core produces packed opaque `0x00RRGGBB` pixels. This module
//! validates dimensions and buffer length, converts to the RGBA byte layout
//! consumed by Pixels, and records the SHA-256 of the three-byte RGB stream.
//! No cartridge, BIOS, save or file API is involved.

use std::fmt;

use gba_native::demo;
use gba_native::display;

use crate::model::{IMAGE_HEIGHT, IMAGE_WIDTH, MAX_SURFACE_PIXELS};

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
                "image {width}x{height} exceeds the {MAX_SURFACE_PIXELS} pixel conversion limit"
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    rgb_sha256: String,
    guest_budget: u64,
}

impl RgbaImage {
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

    /// Known core sample used by the browser checks: guest pixel (x, y).
    pub fn rgb_at(&self, x: u32, y: u32) -> Option<(u8, u8, u8)> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = ((y * self.width + x) * 4) as usize;
        Some((self.rgba[index], self.rgba[index + 1], self.rgba[index + 2]))
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
                "synthetic demo produced {width}x{height}, expected {IMAGE_WIDTH}x{IMAGE_HEIGHT}"
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// Convert validated RGB888 words into opaque RGBA8888 bytes.
pub fn convert_rgb888_to_rgba(width: u32, height: u32, rgb: &[u32]) -> Result<Vec<u8>, ImageError> {
    if width == 0 || height == 0 {
        return Err(ImageError::ZeroDimension { width, height });
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_SURFACE_PIXELS {
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

/// Execute the synthetic demo through the core's interpreter exactly once.
pub fn synthetic_demo_image() -> Result<RgbaImage, LoadError> {
    let (_cpu, bus) =
        demo::run_demo_reference().map_err(|error| LoadError::Core(error.to_string()))?;
    let image: display::Image =
        display::snapshot(&bus).map_err(|error| LoadError::Core(error.to_string()))?;
    if image.width != IMAGE_WIDTH || image.height != IMAGE_HEIGHT {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EXPECTED_GUEST_BUDGET, EXPECTED_SHA256};

    #[test]
    fn browser_source_matches_the_recorded_native_evidence() {
        let image = synthetic_demo_image().expect("synthetic demo must execute");
        assert_eq!(image.width(), 240);
        assert_eq!(image.height(), 160);
        assert_eq!(image.rgb_sha256(), EXPECTED_SHA256);
        assert_eq!(image.guest_budget(), EXPECTED_GUEST_BUDGET);
        assert_eq!(image.rgba().len(), 240 * 160 * 4);
        assert_eq!(image.rgb_at(0, 0), Some((0x00, 0x00, 0x00)));
        assert_eq!(image.rgb_at(120, 80), Some((0xC6, 0xDE, 0x94)));
        assert_eq!(image.rgb_at(60, 40), Some((0xE7, 0x6B, 0x4A)));
        assert_eq!(image.rgb_at(180, 120), Some((0xA5, 0x4A, 0xE7)));
        assert_eq!(image.rgb_at(239, 159), Some((0xFF, 0x7B, 0x29)));
        assert_eq!(image.rgb_at(240, 0), None);
    }

    #[test]
    fn every_converted_pixel_is_opaque() {
        let image = synthetic_demo_image().expect("synthetic demo");
        assert!(
            image
                .rgba()
                .iter()
                .skip(3)
                .step_by(4)
                .all(|alpha| *alpha == 0xFF)
        );
    }

    #[test]
    fn conversion_preserves_known_colours_and_validates_input() {
        let rgba = convert_rgb888_to_rgba(3, 1, &[0x0011_2233, 0x00FF_FFFF, 0x0000_0000])
            .expect("valid conversion");
        assert_eq!(
            rgba,
            vec![
                0x11, 0x22, 0x33, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0xFF,
            ]
        );
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
    }
}
