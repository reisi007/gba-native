//! Snapshot renderer for the implemented bitmap display subset.
//!
//! This is not a scanline PPU: it reads the current DISPCNT/VRAM/palette state
//! once. Tiles, sprites, windows, blending, mosaic and affine transforms are
//! rejected or absent, never silently approximated.

use crate::bus::{Bus, PALETTE_BASE, VRAM_BASE};
use crate::error::{Error, Result};
use crate::sha256;

pub const WIDTH: u32 = 240;
pub const HEIGHT: u32 = 160;

/// Bits that would request unimplemented visual features: CGB mode, BG0/BG1/BG3,
/// OBJ layer and windows. Bits 5 and 6 have no effect on the bitmap subset.
const UNSUPPORTED_DISPCNT_MASK: u16 =
    0x0008 | 0x0100 | 0x0200 | 0x0800 | 0x1000 | 0x2000 | 0x4000 | 0x8000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Packed 0x00RRGGBB.
    pub pixels: Vec<u32>,
}

impl Image {
    pub fn new(width: u32, height: u32, pixels: Vec<u32>) -> Result<Self> {
        if width == 0 || height == 0 || pixels.len() != (width as usize) * (height as usize) {
            return Err(Error::InvalidOutputSize { width, height });
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn pixel(&self, x: u32, y: u32) -> u32 {
        self.pixels[(y * self.width + x) as usize]
    }

    pub fn sha256_hex(&self) -> String {
        let mut bytes = Vec::with_capacity(self.pixels.len() * 3);
        for pixel in &self.pixels {
            bytes.push((pixel >> 16) as u8);
            bytes.push((pixel >> 8) as u8);
            bytes.push(*pixel as u8);
        }
        sha256::hex(&sha256::digest(&bytes))
    }
}

pub fn snapshot(bus: &Bus) -> Result<Image> {
    let dispcnt = bus.read16(crate::bus::DISPCNT_ADDR)?;
    render_dispcnt(bus, dispcnt)
}

pub fn render_dispcnt(bus: &Bus, dispcnt: u16) -> Result<Image> {
    if dispcnt & UNSUPPORTED_DISPCNT_MASK != 0 {
        return Err(Error::UnsupportedPpuFeature { dispcnt });
    }
    if dispcnt & 0x0080 != 0 {
        return Ok(Image {
            width: WIDTH,
            height: HEIGHT,
            pixels: vec![0x00FF_FFFF; (WIDTH * HEIGHT) as usize],
        });
    }

    let backdrop = palette_color(bus, 0)?;
    if dispcnt & 0x0400 == 0 {
        return Ok(solid(backdrop));
    }

    match dispcnt & 0x7 {
        3 => render_mode3(bus),
        4 => {
            let page = if dispcnt & 0x0010 != 0 { 0xA000 } else { 0 };
            render_mode4(bus, page)
        }
        5 => {
            let page = if dispcnt & 0x0010 != 0 { 0xA000 } else { 0 };
            render_mode5(bus, page, backdrop)
        }
        mode => Err(Error::UnsupportedDisplayMode { mode }),
    }
}

fn render_mode3(bus: &Bus) -> Result<Image> {
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let addr = VRAM_BASE + 2 * (y * WIDTH + x);
            pixels.push(rgb555_to_rgb888(bus.read16(addr)?));
        }
    }
    Ok(Image {
        width: WIDTH,
        height: HEIGHT,
        pixels,
    })
}

fn render_mode4(bus: &Bus, page: u32) -> Result<Image> {
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let addr = VRAM_BASE + page + y * WIDTH + x;
            let index = bus.read8(addr)?;
            pixels.push(palette_color(bus, index as usize)?);
        }
    }
    Ok(Image {
        width: WIDTH,
        height: HEIGHT,
        pixels,
    })
}

fn render_mode5(bus: &Bus, page: u32, backdrop: u32) -> Result<Image> {
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if x < 160 && y < 128 {
                let addr = VRAM_BASE + page + 2 * (y * 160 + x);
                pixels.push(rgb555_to_rgb888(bus.read16(addr)?));
            } else {
                pixels.push(backdrop);
            }
        }
    }
    Ok(Image {
        width: WIDTH,
        height: HEIGHT,
        pixels,
    })
}

fn solid(color: u32) -> Image {
    Image {
        width: WIDTH,
        height: HEIGHT,
        pixels: vec![color; (WIDTH * HEIGHT) as usize],
    }
}

fn palette_color(bus: &Bus, index: usize) -> Result<u32> {
    let value = bus.read16(PALETTE_BASE + 2 * index as u32)?;
    Ok(rgb555_to_rgb888(value))
}

pub fn rgb555_to_rgb888(value: u16) -> u32 {
    let r = expand5(value & 0x1F);
    let g = expand5((value >> 5) & 0x1F);
    let b = expand5((value >> 10) & 0x1F);
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

fn expand5(value: u16) -> u8 {
    ((value << 3) | (value >> 2)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb555_expansion_replicates_high_bits() {
        assert_eq!(rgb555_to_rgb888(0x0000), 0x000000);
        assert_eq!(rgb555_to_rgb888(0x7FFF), 0xFFFFFF);
        assert_eq!(rgb555_to_rgb888(0x001F), 0xFF0000);
        assert_eq!(rgb555_to_rgb888(0x03E0), 0x00FF00);
        assert_eq!(rgb555_to_rgb888(0x7C00), 0x0000FF);
    }
}
