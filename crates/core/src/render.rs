//! CPU presentation filters with centered letterboxing. Vendor temporal/AI
//! backends are explicit unsupported status, never hidden fallbacks.

use crate::display::{HEIGHT, Image, WIDTH};
use crate::error::{Error, Result};

pub const MAX_OUTPUT_DIMENSION: u32 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Nearest,
    SharpBilinear,
    Fsr1,
    TemporalFsr,
    MlFsr,
    Dlss,
}

impl Backend {
    pub fn parse(name: &str) -> Result<Self> {
        match name.to_ascii_lowercase().as_str() {
            "nearest" => Ok(Backend::Nearest),
            "sharp-bilinear" => Ok(Backend::SharpBilinear),
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
            Backend::SharpBilinear => "sharp-bilinear",
            Backend::Fsr1 => "fsr1",
            Backend::TemporalFsr => "temporal-fsr",
            Backend::MlFsr => "ml-fsr",
            Backend::Dlss => "dlss",
        }
    }
}

/// Scale the native image into an output with centered black bars.
/// Sharp-bilinear is a CPU reference filter; vendor backends remain unsupported.
pub fn scale(image: &Image, width: u32, height: u32, backend: Backend) -> Result<Image> {
    if !matches!(backend, Backend::Nearest | Backend::SharpBilinear) {
        return Err(Error::UnsupportedBackend {
            name: backend.name().to_string(),
        });
    }
    if width == 0 || height == 0 || width > MAX_OUTPUT_DIMENSION || height > MAX_OUTPUT_DIMENSION {
        return Err(Error::InvalidOutputSize { width, height });
    }
    if backend == Backend::SharpBilinear {
        if width < WIDTH || height < HEIGHT {
            return Err(Error::OutputTooSmall { width, height });
        }
        return Ok(scale_sharp_bilinear(image, width, height));
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

fn sharp_axis(position: u32, source_size: u32, scale: f64) -> (u32, u32, f64) {
    let source = ((f64::from(position) + 0.5) / scale - 0.5).clamp(0.0, f64::from(source_size - 1));
    let low = source.floor() as u32;
    let fraction = source - f64::from(low);
    let weight = ((fraction - 0.5) * scale + 0.5).clamp(0.0, 1.0);
    (low, (low + 1).min(source_size - 1), weight)
}

fn sharp_bilinear_pixel(image: &Image, x: u32, y: u32, scale: f64) -> u32 {
    let (x0, x1, tx) = sharp_axis(x, WIDTH, scale);
    let (y0, y1, ty) = sharp_axis(y, HEIGHT, scale);
    let [top_left, top_right, bottom_left, bottom_right] = [
        image.pixel(x0, y0),
        image.pixel(x1, y0),
        image.pixel(x0, y1),
        image.pixel(x1, y1),
    ];
    let channel = |shift: u32| {
        let top = f64::from((top_left >> shift) & 0xFFu32) * (1.0 - tx)
            + f64::from((top_right >> shift) & 0xFFu32) * tx;
        let bottom = f64::from((bottom_left >> shift) & 0xFFu32) * (1.0 - tx)
            + f64::from((bottom_right >> shift) & 0xFFu32) * tx;
        ((top * (1.0 - ty) + bottom * ty).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

fn scale_sharp_bilinear(image: &Image, width: u32, height: u32) -> Image {
    let scale = (f64::from(width) / f64::from(WIDTH)).min(f64::from(height) / f64::from(HEIGHT));
    // Clamp rounding at the output boundary before subtracting/offsetting.
    let content_width = ((f64::from(WIDTH) * scale).round() as u32).min(width);
    let content_height = ((f64::from(HEIGHT) * scale).round() as u32).min(height);
    let offset_x = (width - content_width) / 2;
    let offset_y = (height - content_height) / 2;
    let mut output = vec![0; (width * height) as usize];
    for y in 0..content_height {
        for x in 0..content_width {
            let index = ((offset_y + y) * width + offset_x + x) as usize;
            output[index] = sharp_bilinear_pixel(image, x, y, scale);
        }
    }
    Image {
        width,
        height,
        pixels: output,
    }
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

        let sharp_integer = scale(&image, 480, 320, Backend::SharpBilinear).expect("sharp scale");
        assert_eq!(sharp_integer.pixels, scaled.pixels);
        let sharp_odd_integer =
            scale(&image, 720, 480, Backend::SharpBilinear).expect("odd integer scale");
        let nearest_odd_integer = scale(&image, 720, 480, Backend::Nearest).expect("nearest scale");
        assert_eq!(sharp_odd_integer.pixels, nearest_odd_integer.pixels);

        let mut edge = Image {
            width: WIDTH,
            height: HEIGHT,
            pixels: vec![0x00FF_FFFF; (WIDTH * HEIGHT) as usize],
        };
        for y in 0..HEIGHT {
            edge.pixels[(y * WIDTH) as usize] = 0;
        }
        let fractional = scale(&edge, 360, 240, Backend::SharpBilinear).expect("fractional scale");
        assert_eq!(fractional.pixel(0, 0), 0);
        assert_eq!(fractional.pixel(1, 0), 0x0080_8080);
        assert_eq!(fractional.pixel(2, 0), 0x00FF_FFFF);
        assert_eq!(Backend::parse("sharp-bilinear"), Ok(Backend::SharpBilinear));
    }

    fn assert_solid_content_rect(image: &Image, rect: (u32, u32, u32, u32), color: u32) {
        let (left, top, content_width, content_height) = rect;
        let right = left + content_width;
        let bottom = top + content_height;

        for y in 0..image.height {
            for x in 0..image.width {
                let inside = x >= left && x < right && y >= top && y < bottom;
                let expected = if inside { color } else { 0 };
                assert_eq!(
                    image.pixel(x, y),
                    expected,
                    "unexpected output pixel at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn four_k_layout_bounds_and_bars_are_cpu_verified() {
        let color = 0x0012_3456;
        let image = Image {
            width: WIDTH,
            height: HEIGHT,
            pixels: vec![color; (WIDTH * HEIGHT) as usize],
        };

        for width in [3840, 4096] {
            for backend in [Backend::Nearest, Backend::SharpBilinear] {
                let (content_width, content_height) = match backend {
                    Backend::Nearest => (3120, 2080),
                    Backend::SharpBilinear => (3240, 2160),
                    _ => unreachable!("only CPU presentation modes are exercised"),
                };
                let output = scale(&image, width, 2160, backend).expect("4K CPU scale");
                assert_eq!((output.width, output.height), (width, 2160));
                let rect = (
                    (width - content_width) / 2,
                    (2160 - content_height) / 2,
                    content_width,
                    content_height,
                );

                assert_solid_content_rect(&output, rect, color);
            }
        }
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
