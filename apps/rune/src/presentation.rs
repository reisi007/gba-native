//! Run the synthetic core fixture once and adapt its snapshot to Rune pixels.

use gba_native::{demo, display};
use rune::Pixels;

pub const WIDTH: u32 = display::WIDTH;
pub const HEIGHT: u32 = display::HEIGHT;
pub const EXPECTED_RGB_SHA256: &str =
    "b02968d620ee66d7175c29bbde12bc9ff075290f3e8086b26e9d4829b9ccb154";
pub const EXPECTED_GUEST_BUDGET: u64 = 230_411;

pub struct SyntheticSnapshot {
    pub pixels: Pixels,
    pub width: u32,
    pub height: u32,
    pub rgb_sha256: String,
    pub guest_budget: u64,
}

/// Execute the built-in synthetic fixture and expose its static image to Rune.
pub fn synthetic_snapshot() -> Result<SyntheticSnapshot, String> {
    let (_cpu, bus) = demo::run_demo_reference().map_err(|error| error.to_string())?;
    let image = display::snapshot(&bus).map_err(|error| error.to_string())?;
    if (image.width, image.height) != (WIDTH, HEIGHT) {
        return Err(format!(
            "synthetic snapshot is {}x{}, expected {WIDTH}x{HEIGHT}",
            image.width, image.height
        ));
    }

    let mut rgba = Vec::with_capacity(image.pixels.len() * 4);
    for (index, pixel) in image.pixels.iter().copied().enumerate() {
        if pixel & 0xFF00_0000 != 0 {
            return Err(format!(
                "synthetic pixel {index} has a non-zero high byte: {pixel:#010X}"
            ));
        }
        rgba.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8, 0xFF]);
    }

    let pixels = Pixels::new();
    pixels.set(image.width, image.height, rgba);
    Ok(SyntheticSnapshot {
        pixels,
        width: image.width,
        height: image.height,
        rgb_sha256: image.sha256_hex(),
        guest_budget: demo::DEMO_BUDGET,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rune_source_matches_the_recorded_synthetic_fixture() {
        let snapshot = synthetic_snapshot().expect("synthetic demo should execute");
        assert_eq!((snapshot.width, snapshot.height), (240, 160));
        assert_eq!(snapshot.pixels.size(), (240, 160));
        assert_eq!(snapshot.rgb_sha256, EXPECTED_RGB_SHA256);
        assert_eq!(snapshot.guest_budget, EXPECTED_GUEST_BUDGET);
    }
}
