//! Static Mode 0 OBJ (sprite) layer for the supported OAM forms.
//!
//! Supports normal and affine sprites, 1D tile mapping, 4/8bpp tiles, flips and
//! per-pixel priority. 2D tile mapping, semi-transparent OBJ mode, mosaic and
//! double-size affine sprites are explicit errors rather than approximated
//! layers.

use super::layer::{LayerPixel, choose};
use super::{HEIGHT, WIDTH};
use crate::bus::{Bus, OAM_BASE, PALETTE_BASE, VRAM_BASE};
use crate::error::{Error, Result};

const OAM_LEN: u32 = 1024;
const VRAM_LEN: u32 = 96 * 1024;
const OBJ_PALETTE_BASE: u32 = PALETTE_BASE + 0x200;
const OBJ_TILE_BASE: u32 = 0x1_0000;

pub(super) struct ObjLayer {
    pub pixels: Vec<Option<LayerPixel>>,
    pub window: Vec<bool>,
}

pub(super) fn render(bus: &Bus, dispcnt: u16) -> Result<ObjLayer> {
    if dispcnt & 0x0040 == 0 {
        return Err(Error::UnsupportedPpuFeature { dispcnt });
    }
    let area = (WIDTH * HEIGHT) as usize;
    let mut layer = ObjLayer {
        pixels: vec![None; area],
        window: vec![false; area],
    };
    for entry in 0..128u32 {
        render_entry(bus, dispcnt, entry, &mut layer)?;
    }
    Ok(layer)
}

fn render_entry(bus: &Bus, dispcnt: u16, entry: u32, layer: &mut ObjLayer) -> Result<()> {
    let base = entry * 8;
    let attr0 = read_oam16(bus, base)?;
    let attr1 = read_oam16(bus, base + 2)?;
    let attr2 = read_oam16(bus, base + 4)?;
    let affine = attr0 & 0x0100 != 0;
    if affine {
        if attr0 & 0x0200 != 0 {
            return Err(Error::UnsupportedPpuFeature { dispcnt });
        }
    } else if attr0 & 0x0200 != 0 {
        return Ok(()); // disabled
    }
    let mode = (attr0 >> 10) & 0x3;
    let window_mode = mode == 2;
    if mode == 1 || mode == 3 {
        return Err(Error::UnsupportedPpuFeature { dispcnt });
    }
    if attr0 & 0x1000 != 0 {
        return Err(Error::UnsupportedPpuFeature { dispcnt });
    }
    let color_8bpp = attr0 & 0x2000 != 0;
    let shape = ((attr0 >> 14) & 0x3) as usize;
    if shape == 3 {
        return Ok(());
    }
    let size = ((attr1 >> 14) & 0x3) as usize;
    let Some((width, height)) = obj_size(shape, size) else {
        return Ok(());
    };
    let origin_y = (attr0 & 0x00FF) as i32;
    let mut origin_x = (attr1 & 0x01FF) as i32;
    if origin_x >= 256 {
        origin_x -= 512;
    } else if origin_x >= 240 {
        return Ok(());
    }
    if origin_y >= 160 {
        return Ok(());
    }
    let priority = ((attr2 >> 10) & 0x3) as u8;
    let tile_base = (attr2 & 0x03FF) as usize;
    let palette_bank = ((attr2 >> 12) & 0xF) as usize;
    let (matrix, flip_x, flip_y) = if affine {
        (Some(read_affine(bus, attr1)?), false, false)
    } else {
        (None, attr1 & 0x1000 != 0, attr1 & 0x2000 != 0)
    };
    for dy in 0..height as i32 {
        let screen_y = origin_y + dy;
        if !(0..HEIGHT as i32).contains(&screen_y) {
            continue;
        }
        for dx in 0..width as i32 {
            let screen_x = origin_x + dx;
            if !(0..WIDTH as i32).contains(&screen_x) {
                continue;
            }
            let Some((source_x, source_y)) =
                source_pixel(width, height, dx, dy, flip_x, flip_y, matrix)
            else {
                continue;
            };
            let tile_step = if color_8bpp { 2 } else { 1 };
            let tile = tile_base
                + tile_step * ((source_y as usize / 8) * (width / 8) + source_x as usize / 8);
            let pixel_x = source_x as usize % 8;
            let pixel_y = source_y as usize % 8;
            let Some(color_index) = read_tile_pixel(bus, tile, pixel_x, pixel_y, color_8bpp)?
            else {
                continue;
            };
            let pixel_index = (screen_y as usize) * WIDTH as usize + screen_x as usize;
            if window_mode {
                layer.window[pixel_index] = true;
                continue;
            }
            let palette_index = if color_8bpp {
                color_index
            } else {
                palette_bank * 16 + color_index
            };
            let color = read_obj_palette(bus, palette_index)?;
            let candidate = LayerPixel {
                color,
                priority,
                layer: 4,
                order: 0,
            };
            choose(&mut layer.pixels[pixel_index], Some(candidate));
        }
    }
    Ok(())
}

fn source_pixel(
    width: usize,
    height: usize,
    dx: i32,
    dy: i32,
    flip_x: bool,
    flip_y: bool,
    matrix: Option<[i16; 4]>,
) -> Option<(i32, i32)> {
    if let Some([pa, pb, pc, pd]) = matrix {
        let center_x = width as i32 / 2;
        let center_y = height as i32 / 2;
        let rel_x = dx - center_x;
        let rel_y = dy - center_y;
        let source_x = center_x + ((i32::from(pa) * rel_x + i32::from(pb) * rel_y) >> 8);
        let source_y = center_y + ((i32::from(pc) * rel_x + i32::from(pd) * rel_y) >> 8);
        if (0..width as i32).contains(&source_x) && (0..height as i32).contains(&source_y) {
            Some((source_x, source_y))
        } else {
            None
        }
    } else {
        let source_x = if flip_x { width as i32 - 1 - dx } else { dx };
        let source_y = if flip_y { height as i32 - 1 - dy } else { dy };
        Some((source_x, source_y))
    }
}

fn read_affine(bus: &Bus, attr1: u16) -> Result<[i16; 4]> {
    let group = ((attr1 >> 9) & 0x1F) as u32;
    // One group uses the two-byte affine slots of four consecutive OAM entries.
    let base = group * 32 + 6;
    Ok([
        read_oam16(bus, base)? as i16,
        read_oam16(bus, base + 8)? as i16,
        read_oam16(bus, base + 16)? as i16,
        read_oam16(bus, base + 24)? as i16,
    ])
}

fn read_tile_pixel(
    bus: &Bus,
    tile: usize,
    x: usize,
    y: usize,
    color_8bpp: bool,
) -> Result<Option<usize>> {
    // In 1D mapping the OAM tile number is a 32-byte unit for both depths;
    // 8bpp tiles occupy two consecutive units (GBATEK's 04h/06h example).
    let offset = if color_8bpp {
        OBJ_TILE_BASE as usize + tile * 32 + y * 8 + x
    } else {
        OBJ_TILE_BASE as usize + tile * 32 + y * 4 + x / 2
    };
    if offset >= VRAM_LEN as usize {
        return Err(Error::BgVramOutOfRange {
            addr: VRAM_BASE + offset as u32,
            width: 1,
        });
    }
    let value = bus.read8(VRAM_BASE + offset as u32)?;
    if color_8bpp {
        Ok((value != 0).then_some(value as usize))
    } else {
        let index = if x & 1 == 0 { value & 0x0F } else { value >> 4 } as usize;
        Ok((index != 0).then_some(index))
    }
}

fn read_obj_palette(bus: &Bus, index: usize) -> Result<u32> {
    let value = bus.read16(OBJ_PALETTE_BASE + 2 * index as u32)?;
    Ok(super::rgb555_to_rgb888(value))
}

fn read_oam16(bus: &Bus, offset: u32) -> Result<u16> {
    if offset + 2 > OAM_LEN {
        return Err(Error::Unmapped {
            addr: OAM_BASE + offset,
        });
    }
    bus.read16(OAM_BASE + offset)
}

fn obj_size(shape: usize, size: usize) -> Option<(usize, usize)> {
    Some(match (shape, size) {
        (0, 0) => (8, 8),
        (0, 1) => (16, 16),
        (0, 2) => (32, 32),
        (0, 3) => (64, 64),
        (1, 0) => (16, 8),
        (1, 1) => (32, 8),
        (1, 2) => (32, 16),
        (1, 3) => (64, 32),
        (2, 0) => (8, 16),
        (2, 1) => (8, 32),
        (2, 2) => (16, 32),
        (2, 3) => (32, 64),
        _ => return None,
    })
}
