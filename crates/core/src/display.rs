//! Snapshot renderer for the implemented Mode 0 BG0-BG3/OBJ and bitmap subsets.
//!
//! This is not a scanline PPU: it reads the current DISPCNT/VRAM/palette state
//! once. Mode 0 BG0-BG3 text + OBJ and Mode 1 BG0/BG1 text + BG2 affine + OBJ
//! are composed with window 0, the OBJ window and BLDCNT alpha/brightness
//! blending. 2D OBJ mapping, semi-transparent OBJ mode, double-size affine OBJ,
//! mosaic, window 1 and Mode 2 are explicit errors, never silent approximations.

use crate::bus::{Bus, PALETTE_BASE, VRAM_BASE};
use crate::error::{Error, Result};
use crate::sha256;

mod affine;
mod blend;
mod layer;
mod obj;
mod text;
mod window;

pub const WIDTH: u32 = 240;
pub const HEIGHT: u32 = 160;

/// Unsupported layers/features in bitmap modes. Bits 5 and 6 do not affect this subset.
const BITMAP_UNSUPPORTED_DISPCNT_MASK: u16 =
    0x0008 | 0x0100 | 0x0200 | 0x0800 | 0x1000 | 0x2000 | 0x4000 | 0x8000;
/// Mode 0 rejects the reserved bit and the window-1 enable. Window 0 and the
/// OBJ window are implemented.
const MODE0_UNSUPPORTED_DISPCNT_MASK: u16 = 0x0008 | 0x4000;
/// Mode 1 uses BG0/BG1 text, BG2 affine and OBJ; BG3 and the two later windows
/// are rejected explicitly.
const MODE1_UNSUPPORTED_DISPCNT_MASK: u16 = 0x0008 | 0x0800 | 0x4000;
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
    let unsupported_mask = match mode {
        0 => MODE0_UNSUPPORTED_DISPCNT_MASK,
        1 => MODE1_UNSUPPORTED_DISPCNT_MASK,
        _ => BITMAP_UNSUPPORTED_DISPCNT_MASK,
    };
    if dispcnt & unsupported_mask != 0 {
        return Err(Error::UnsupportedPpuFeature { dispcnt });
    }
    if mode == 0 {
        // Validate enabled text backgrounds before forced blank can hide them.
        for index in 0..4u8 {
            if dispcnt & (0x0100 << index) != 0 {
                text::validate_control(bus.bgcnt(index))?;
            }
        }
    }
    if dispcnt & 0x0080 != 0 {
        return Ok(Image {
            width: WIDTH,
            height: HEIGHT,
            pixels: vec![0x00FF_FFFF; (WIDTH * HEIGHT) as usize],
        });
    }

    let blend = blend::Blend::new(bus)?;
    let window = window::Window::new(bus, dispcnt)?;
    let backdrop = palette_color(bus, 0)?;

    match mode {
        0 => {
            if dispcnt & 0x1F00 == 0 {
                Ok(solid(backdrop))
            } else {
                render_mode0(bus, dispcnt, backdrop, &window, &blend)
            }
        }
        1 => {
            if dispcnt & 0x1700 == 0 {
                Ok(solid(backdrop))
            } else {
                render_mode1(bus, dispcnt, backdrop, &window, &blend)
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

fn render_mode0(
    bus: &Bus,
    dispcnt: u16,
    backdrop: u32,
    window: &window::Window,
    blend: &blend::Blend,
) -> Result<Image> {
    let mut backgrounds: [Option<text::TextBg>; 4] = [None, None, None, None];
    for index in 0..4u8 {
        if dispcnt & (0x0100 << index) != 0 {
            backgrounds[index as usize] = Some(text::TextBg::new(bus, index)?);
        }
    }
    let objects = if dispcnt & 0x1000 != 0 {
        Some(obj::render(bus, dispcnt)?)
    } else {
        None
    };
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT as usize {
        for x in 0..WIDTH as usize {
            let obj_pixel = objects
                .as_ref()
                .is_some_and(|objects| objects.window[y * WIDTH as usize + x]);
            let mut candidates: [Option<layer::LayerPixel>; 5] = [None; 5];
            for (index, background) in backgrounds.iter().enumerate() {
                if let Some(background) = background
                    && window.allows(x as u32, y as u32, index as u8, obj_pixel)
                {
                    candidates[index] = background.pixel(x, y)?;
                }
            }
            if let Some(objects) = &objects
                && window.allows(x as u32, y as u32, 4, obj_pixel)
            {
                candidates[4] = objects.pixels[y * WIDTH as usize + x];
            }
            pixels.push(composite(&candidates, backdrop, blend));
        }
    }
    Ok(Image {
        width: WIDTH,
        height: HEIGHT,
        pixels,
    })
}

fn render_mode1(
    bus: &Bus,
    dispcnt: u16,
    backdrop: u32,
    window: &window::Window,
    blend: &blend::Blend,
) -> Result<Image> {
    let mut backgrounds: [Option<text::TextBg>; 2] = [None, None];
    for index in 0..2u8 {
        if dispcnt & (0x0100 << index) != 0 {
            backgrounds[index as usize] = Some(text::TextBg::new(bus, index)?);
        }
    }
    let affine = if dispcnt & 0x0400 != 0 {
        Some(affine::AffineBg::new(bus, 2)?)
    } else {
        None
    };
    let objects = if dispcnt & 0x1000 != 0 {
        Some(obj::render(bus, dispcnt)?)
    } else {
        None
    };
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT as usize {
        for x in 0..WIDTH as usize {
            let obj_pixel = objects
                .as_ref()
                .is_some_and(|objects| objects.window[y * WIDTH as usize + x]);
            let mut candidates: [Option<layer::LayerPixel>; 5] = [None; 5];
            for (index, background) in backgrounds.iter().enumerate() {
                if let Some(background) = background
                    && window.allows(x as u32, y as u32, index as u8, obj_pixel)
                {
                    candidates[index] = background.pixel(x, y)?;
                }
            }
            if let Some(affine) = &affine
                && window.allows(x as u32, y as u32, 2, obj_pixel)
            {
                candidates[2] = affine.pixel(x, y)?;
            }
            if let Some(objects) = &objects
                && window.allows(x as u32, y as u32, 4, obj_pixel)
            {
                candidates[4] = objects.pixels[y * WIDTH as usize + x];
            }
            pixels.push(composite(&candidates, backdrop, blend));
        }
    }
    Ok(Image {
        width: WIDTH,
        height: HEIGHT,
        pixels,
    })
}

pub(super) fn read_bg16(bus: &Bus, offset: usize) -> Result<u16> {
    check_bg_vram(offset, 2)?;
    bus.read16(VRAM_BASE + offset as u32)
}

pub(super) fn read_bg8(bus: &Bus, offset: usize) -> Result<u8> {
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

fn composite(
    candidates: &[Option<layer::LayerPixel>; 5],
    backdrop: u32,
    blend: &blend::Blend,
) -> u32 {
    let Some(top) = layer::best(candidates) else {
        return blend.brightness(5, backdrop);
    };
    if blend.mode() == 1 && blend.is_first(top.layer) {
        if let Some(next) = layer::next(candidates, top) {
            // GBATEK: blend only when the immediately next lower
            // non-transparent layer is a second target.
            return if blend.is_second(next.layer) {
                blend.alpha(top.color, next.color)
            } else {
                top.color
            };
        }
        return if blend.is_second(5) {
            blend.alpha(top.color, backdrop)
        } else {
            top.color
        };
    }
    blend.brightness(top.layer, top.color)
}

fn solid(color: u32) -> Image {
    Image {
        width: WIDTH,
        height: HEIGHT,
        pixels: vec![color; (WIDTH * HEIGHT) as usize],
    }
}

pub(super) fn palette_color(bus: &Bus, index: usize) -> Result<u32> {
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
