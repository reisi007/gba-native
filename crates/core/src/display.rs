//! Snapshot renderer for the implemented Mode 0 BG0 and bitmap display subsets.
//!
//! This is not a scanline PPU: it reads the current DISPCNT/VRAM/palette state
//! once. Only Mode 0 BG0 text tiles and bitmap modes 3-5 are rendered; other
//! layers, sprites, windows, blending, mosaic and affine transforms are rejected
//! or absent, never silently approximated.

use crate::bus::{Bus, PALETTE_BASE, VRAM_BASE};
use crate::error::{Error, Result};
use crate::sha256;

pub const WIDTH: u32 = 240;
pub const HEIGHT: u32 = 160;

/// Unsupported layers/features in bitmap modes. Bits 5 and 6 do not affect this subset.
const BITMAP_UNSUPPORTED_DISPCNT_MASK: u16 =
    0x0008 | 0x0100 | 0x0200 | 0x0800 | 0x1000 | 0x2000 | 0x4000 | 0x8000;
/// Mode 0 uses BG0, but not the other BGs, OBJ, windows or CGB mode.
const MODE0_UNSUPPORTED_DISPCNT_MASK: u16 =
    0x0008 | 0x0200 | 0x0400 | 0x0800 | 0x1000 | 0x2000 | 0x4000 | 0x8000;
const BG_VRAM_LEN: usize = 64 * 1024;

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
    let mode = dispcnt & 0x7;
    let unsupported_mask = if mode == 0 {
        MODE0_UNSUPPORTED_DISPCNT_MASK
    } else {
        BITMAP_UNSUPPORTED_DISPCNT_MASK
    };
    if dispcnt & unsupported_mask != 0 {
        return Err(Error::UnsupportedPpuFeature { dispcnt });
    }
    if mode == 0 && dispcnt & 0x0100 != 0 {
        validate_mode0_bg0_control(bus.bg0cnt())?;
    }
    if dispcnt & 0x0080 != 0 {
        return Ok(Image {
            width: WIDTH,
            height: HEIGHT,
            pixels: vec![0x00FF_FFFF; (WIDTH * HEIGHT) as usize],
        });
    }

    let backdrop = palette_color(bus, 0)?;

    match mode {
        0 => {
            if dispcnt & 0x0100 == 0 {
                Ok(solid(backdrop))
            } else {
                render_mode0_bg0(bus, backdrop)
            }
        }
        3 => {
            if dispcnt & 0x0400 == 0 {
                Ok(solid(backdrop))
            } else {
                render_mode3(bus)
            }
        }
        4 => {
            if dispcnt & 0x0400 == 0 {
                return Ok(solid(backdrop));
            }
            let page = if dispcnt & 0x0010 != 0 { 0xA000 } else { 0 };
            render_mode4(bus, page)
        }
        5 => {
            if dispcnt & 0x0400 == 0 {
                return Ok(solid(backdrop));
            }
            let page = if dispcnt & 0x0010 != 0 { 0xA000 } else { 0 };
            render_mode5(bus, page, backdrop)
        }
        mode => Err(Error::UnsupportedDisplayMode { mode }),
    }
}

fn render_mode0_bg0(bus: &Bus, backdrop: u32) -> Result<Image> {
    let bgcnt = bus.bg0cnt();
    let size = ((bgcnt >> 14) & 0x3) as usize;
    let bg_width = if size & 1 == 0 { 256 } else { 512 };
    let bg_height = if size & 2 == 0 { 256 } else { 512 };
    let char_base = (((bgcnt >> 2) & 0x3) as usize) * 0x4000;
    let screen_base = (((bgcnt >> 8) & 0x1F) as usize) * 0x800;
    let color_8bpp = bgcnt & 0x0080 != 0;
    let (scroll_x, scroll_y) = bus.bg0_scroll();

    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT as usize {
        for x in 0..WIDTH as usize {
            let source_x = (x + scroll_x as usize) % bg_width;
            let source_y = (y + scroll_y as usize) % bg_height;
            let tile_x = source_x / 8;
            let tile_y = source_y / 8;
            let screen_quadrants_wide = bg_width / 256;
            let quadrant = (tile_y / 32) * screen_quadrants_wide + tile_x / 32;
            let entry_offset =
                screen_base + quadrant * 0x800 + ((tile_y % 32) * 32 + tile_x % 32) * 2;
            let entry = read_bg16(bus, entry_offset)?;
            let mut pixel_x = source_x % 8;
            let mut pixel_y = source_y % 8;
            if entry & 0x0400 != 0 {
                pixel_x = 7 - pixel_x;
            }
            if entry & 0x0800 != 0 {
                pixel_y = 7 - pixel_y;
            }
            let tile_number = (entry & 0x03FF) as usize;
            let color_index = if color_8bpp {
                let tile_offset = char_base + tile_number * 64;
                read_bg8(bus, tile_offset + pixel_y * 8 + pixel_x)? as usize
            } else {
                let tile_offset = char_base + tile_number * 32;
                let packed = read_bg8(bus, tile_offset + pixel_y * 4 + pixel_x / 2)?;
                let index = (if pixel_x & 1 == 0 {
                    packed & 0x0F
                } else {
                    packed >> 4
                }) as usize;
                if index == 0 {
                    pixels.push(backdrop);
                    continue;
                }
                (((entry >> 12) & 0xF) as usize) * 16 + index
            };
            if color_index == 0 {
                pixels.push(backdrop);
            } else {
                pixels.push(palette_color(bus, color_index)?);
            }
        }
    }
    Ok(Image {
        width: WIDTH,
        height: HEIGHT,
        pixels,
    })
}

fn validate_mode0_bg0_control(bgcnt: u16) -> Result<()> {
    if bgcnt & 0x0040 != 0 {
        return Err(Error::UnsupportedBgControl {
            bgcnt,
            reason: "mosaic is not implemented",
        });
    }
    if bgcnt & 0x2030 != 0 {
        return Err(Error::UnsupportedBgControl {
            bgcnt,
            reason: "reserved BG0CNT bits must be zero in GBA mode",
        });
    }
    Ok(())
}

fn read_bg16(bus: &Bus, offset: usize) -> Result<u16> {
    check_bg_vram(offset, 2)?;
    bus.read16(VRAM_BASE + offset as u32)
}

fn read_bg8(bus: &Bus, offset: usize) -> Result<u8> {
    check_bg_vram(offset, 1)?;
    bus.read8(VRAM_BASE + offset as u32)
}

fn check_bg_vram(offset: usize, width: usize) -> Result<()> {
    if offset
        .checked_add(width)
        .is_some_and(|end| end <= BG_VRAM_LEN)
    {
        return Ok(());
    }
    Err(Error::BgVramOutOfRange {
        addr: VRAM_BASE + offset as u32,
        width: width as u8,
    })
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
