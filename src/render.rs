//! Presentation baseline: deterministic nearest-neighbor integer scaling with
//! 3:2 letterboxing. Vendor temporal/AI backends are explicit unsupported
//! status, never a hidden nearest-neighbor fallback.

use crate::display::{HEIGHT, Image, WIDTH};
use crate::error::{Error, Result};

pub const MAX_OUTPUT_DIMENSION: u32 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Nearest,
    Fsr1,
    TemporalFsr,
    MlFsr,
    Dlss,
}

impl Backend {
    pub fn parse(name: &str) -> Result<Self> {
        match name.to_ascii_lowercase().as_str() {
            "nearest" => Ok(Backend::Nearest),
            "fsr1" | "fsr" => Ok(Backend::Fsr1),
            "temporal-fsr" | "temporalfsr" => Ok(Backend::TemporalFsr),
            "ml-fsr" | "mlfsr" => Ok(Backend::MlFsr),
            "dlss" => Ok(Backend::Dlss),
            _ => Err(Error::UnsupportedBackend {
                name: name.to_string(),
            }),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Backend::Nearest => "nearest",
            Backend::Fsr1 => "fsr1",
            Backend::TemporalFsr => "temporal-fsr",
            Backend::MlFsr => "ml-fsr",
            Backend::Dlss => "dlss",
        }
    }
}

/// Integer nearest-neighbor scale. The content is always exactly 3:2 and is
/// centered with black bars. Fractional scaling and all vendor backends are
/// deliberately absent.
pub fn scale(image: &Image, width: u32, height: u32, backend: Backend) -> Result<Image> {
    if backend != Backend::Nearest {
        return Err(Error::UnsupportedBackend {
            name: backend.name().to_string(),
        });
    }
    if width == 0 || height == 0 || width > MAX_OUTPUT_DIMENSION || height > MAX_OUTPUT_DIMENSION {
        return Err(Error::InvalidOutputSize { width, height });
    }

    let scale = (width / WIDTH).min(height / HEIGHT);
    if scale == 0 {
        return Err(Error::OutputTooSmall { width, height });
    }
    let content_width = WIDTH * scale;
    let content_height = HEIGHT * scale;
    let offset_x = (width - content_width) / 2;
    let offset_y = (height - content_height) / 2;

    let mut pixels = vec![0u32; (width * height) as usize];
    for source_y in 0..HEIGHT {
        for source_x in 0..WIDTH {
            let color = image.pixel(source_x, source_y);
            let dest_x0 = offset_x + source_x * scale;
            let dest_y0 = offset_y + source_y * scale;
            for dy in 0..scale {
                let row = ((dest_y0 + dy) * width) as usize;
                for dx in 0..scale {
                    pixels[row + (dest_x0 + dx) as usize] = color;
                }
            }
        }
    }

    Ok(Image {
        width,
        height,
        pixels,
    })
}

pub fn to_ppm(image: &Image) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32 + image.pixels.len() * 3);
    bytes.extend_from_slice(format!("P6\n{} {}\n255\n", image.width, image.height).as_bytes());
    for pixel in &image.pixels {
        bytes.push((pixel >> 16) as u8);
        bytes.push((pixel >> 8) as u8);
        bytes.push(*pixel as u8);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker() -> Image {
        let mut pixels = vec![0u32; (WIDTH * HEIGHT) as usize];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                pixels[(y * WIDTH + x) as usize] = if (x + y) % 2 == 0 {
                    0x00FF_FFFF
                } else {
                    0x0000_00FF
                };
            }
        }
        Image {
            width: WIDTH,
            height: HEIGHT,
            pixels,
        }
    }

    #[test]
    fn integer_scaling_and_letterbox() {
        let image = checker();
        let scaled = scale(&image, 480, 320, Backend::Nearest).expect("scale");
        assert_eq!((scaled.width, scaled.height), (480, 320));
        assert_eq!(scaled.pixel(1, 0), scaled.pixel(0, 0));
        assert_eq!(scaled.pixel(2, 0), scaled.pixel(3, 0));
        assert_ne!(scaled.pixel(2, 0), scaled.pixel(0, 0));

        let letterboxed = scale(&image, 300, 200, Backend::Nearest).expect("scale");
        assert_eq!(letterboxed.pixel(0, 0), 0);
        assert_eq!(letterboxed.pixel(30, 20), image.pixel(0, 0));
        assert_eq!(letterboxed.pixel(269, 179), image.pixel(239, 159));
        assert_eq!(letterboxed.pixel(270, 20), 0);
    }

    #[test]
    fn unknown_backend_name_is_preserved_in_the_diagnostic() {
        let error = Backend::parse("foo").expect_err("unknown backend");
        assert!(matches!(
            &error,
            Error::UnsupportedBackend { name } if name == "foo"
        ));
        assert!(format!("{error}").contains("\"foo\""));
    }

    #[test]
    fn rejects_invalid_sizes_and_vendor_backends() {
        let image = checker();
        assert!(matches!(
            scale(&image, 300, 100, Backend::Nearest),
            Err(Error::OutputTooSmall { .. })
        ));
        assert!(matches!(
            scale(&image, 0, 0, Backend::Nearest),
            Err(Error::InvalidOutputSize { .. })
        ));
        assert!(matches!(
            scale(&image, 5000, 5000, Backend::Nearest),
            Err(Error::InvalidOutputSize { .. })
        ));
        for backend in [
            Backend::Fsr1,
            Backend::TemporalFsr,
            Backend::MlFsr,
            Backend::Dlss,
        ] {
            assert!(matches!(
                scale(&image, 480, 320, backend),
                Err(Error::UnsupportedBackend { .. })
            ));
        }
    }
}
