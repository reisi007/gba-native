//! Shared, window-free frontend image pipeline.
//!
//! Every GUI target (desktop, browser, Android) receives the same validated
//! RGBA image and the same demo snapshot from this module. Nothing here opens a
//! window, reads a ROM, BIOS or save, or claims GPU work: the platform layers
//! only copy these pixels into their surface.

use std::fmt;

use crate::{demo, display};

/// Refuse accidental giant conversions even before a surface is involved.
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
                "image {width}x{height} exceeds the {MAX_IMAGE_PIXELS} pixel conversion limit"
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

/// Errors from building the committed synthetic demo image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    Core(String),
    Image(ImageError),
    UnexpectedDimensions { width: u32, height: u32 },
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(message) => write!(formatter, "core error: {message}"),
            Self::Image(error) => write!(formatter, "{error}"),
            Self::UnexpectedDimensions { width, height } => write!(
                formatter,
                "synthetic demo produced {width}x{height}, expected {}x{}",
                display::WIDTH,
                display::HEIGHT
            ),
        }
    }
}

impl std::error::Error for LoadError {}

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
    /// The RGB digest is recomputed from the buffer, never caller-supplied.
    pub fn from_rgba(
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
        for chunk in rgba.as_chunks::<4>().0 {
            rgb.push(
                (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]),
            );
        }
        let rgb_sha256 = crate::sha256::hex(&crate::sha256::digest(&rgb_bytes(&rgb)));
        Ok(Self {
            width,
            height,
            rgba,
            rgb_sha256,
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

    pub fn rgb_sha256(&self) -> &str {
        &self.rgb_sha256
    }

    pub fn guest_budget(&self) -> u64 {
        self.guest_budget
    }
}

fn rgb_bytes(pixels: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(pixels.len() * 3);
    for pixel in pixels {
        bytes.push((pixel >> 16) as u8);
        bytes.push((pixel >> 8) as u8);
        bytes.push(*pixel as u8);
    }
    bytes
}

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

/// Execute the synthetic demo through the core's interpreter and snapshot it.
pub fn demo_image() -> Result<RgbaImage, LoadError> {
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
    fn from_rgba_round_trips_the_rgb_digest_and_rejects_bad_lengths() {
        let rgba = convert_rgb888_to_rgba(2, 1, &[0x0011_2233, 0x00AA_BBCC]).expect("convert");
        let image = RgbaImage::from_rgba(2, 1, rgba, 7).expect("image");
        assert_eq!(image.rgb_sha256(), {
            let rgb = [0x11u8, 0x22, 0x33, 0xAA, 0xBB, 0xCC];
            crate::sha256::hex(&crate::sha256::digest(&rgb))
        });
        assert_eq!(image.guest_budget(), 7);
        assert_eq!(
            RgbaImage::from_rgba(2, 1, vec![0; 7], 7),
            Err(ImageError::PixelCount {
                width: 2,
                height: 1,
                len: 7
            })
        );
    }

    #[test]
    fn demo_image_matches_the_synthetic_display() {
        let image = demo_image().expect("demo image");
        assert_eq!((image.width(), image.height()), (240, 160));
        let (_cpu, bus) = demo::run_demo_reference().expect("demo");
        let expected = display::snapshot(&bus).expect("snapshot");
        assert_eq!(image.rgb_sha256(), expected.sha256_hex());
        assert_eq!(image.guest_budget(), demo::DEMO_BUDGET);
        assert_eq!(image.rgba().len(), 240 * 160 * 4);
    }
}
