//! Static Mode 0 OBJ (sprite) layer for the supported OAM forms.
//!
//! Supports normal and affine sprites, 1D/2D tile mapping, 4/8bpp tiles, flips,
//! per-pixel priority and per-OBJ mosaic. Semi-transparent OBJ affects only the
//! selected top OBJ pixel during static composition; double-size canvas placement
//! is a static Design policy.

use super::layer::{LayerPixel, choose};
use super::{HEIGHT, WIDTH};
use crate::bus::{Bus, OAM_BASE, PALETTE_BASE, VRAM_BASE};
use crate::error::{Error, Result};

const OAM_LEN: u32 = 1024;
const VRAM_LEN: u32 = 96 * 1024;
const OBJ_PALETTE_BASE: u32 = PALETTE_BASE + 0x200;
const OBJ_TILE_BASE: u32 = 0x1_0000;
const BITMAP_OBJ_TILE_BASE: u32 = 0x1_4000;
const BITMAP_OBJ_TILE_END: u32 = BITMAP_OBJ_TILE_BASE + 16 * 1024;

#[derive(Clone, Copy)]
struct ObjDimensions {
    source_width: usize,
    source_height: usize,
    canvas_width: usize,
    canvas_height: usize,
}

pub(super) struct ObjLayer {
    pub pixels: Vec<Option<LayerPixel>>,
    pub window: Vec<bool>,
}

struct AffineMosaicSampler {
    dimensions: ObjDimensions,
    origin_x: i32,
    origin_y: i32,
    mosaic_width: i32,
    mosaic_height: i32,
    matrix: [i16; 4],
}

impl AffineMosaicSampler {
    fn source_sample(&self, group_x: i32, group_y: i32) -> Option<(i32, i32)> {
        let left = group_x.max(self.origin_x).max(0);
        let top = group_y.max(self.origin_y).max(0);
        let right = (group_x + self.mosaic_width)
            .min(self.origin_x + self.dimensions.canvas_width as i32)
            .min(WIDTH as i32);
        let bottom = (group_y + self.mosaic_height)
            .min(self.origin_y + self.dimensions.canvas_height as i32)
            .min(HEIGHT as i32);
        for screen_y in top..bottom {
            for screen_x in left..right {
                if let Some(source) = source_pixel(
                    self.dimensions,
                    screen_x - self.origin_x,
                    screen_y - self.origin_y,
                    false,
                    false,
                    Some(self.matrix),
                ) {
                    return Some(source);
                }
            }
        }
        None
    }
}

pub(super) fn render(bus: &Bus, dispcnt: u16) -> Result<ObjLayer> {
    let mosaic = bus.mosaic();
    let mosaic_width = i32::from((mosaic >> 8) & 0xF) + 1;
    let mosaic_height = i32::from((mosaic >> 12) & 0xF) + 1;
    let area = (WIDTH * HEIGHT) as usize;
    let mut layer = ObjLayer {
        pixels: vec![None; area],
        window: vec![false; area],
    };
    for entry in 0..128u32 {
        render_entry(bus, dispcnt, entry, mosaic_width, mosaic_height, &mut layer)?;
    }
    Ok(layer)
}

fn render_entry(
    bus: &Bus,
    dispcnt: u16,
    entry: u32,
    mosaic_width: i32,
    mosaic_height: i32,
    layer: &mut ObjLayer,
) -> Result<()> {
    let base = entry * 8;
    let attr0 = read_oam16(bus, base)?;
    let attr1 = read_oam16(bus, base + 2)?;
    let attr2 = read_oam16(bus, base + 4)?;
    let affine = attr0 & 0x0100 != 0;
    let double_size = affine && attr0 & 0x0200 != 0;
    if !affine && attr0 & 0x0200 != 0 {
        return Ok(()); // disabled
    }
    let mode = (attr0 >> 10) & 0x3;
    let window_mode = mode == 2;
    if mode == 3 {
        return Err(Error::UnsupportedPpuFeature { dispcnt });
    }
    let mosaic_enabled = attr0 & 0x1000 != 0;
    let color_8bpp = attr0 & 0x2000 != 0;
    let shape = ((attr0 >> 14) & 0x3) as usize;
    if shape == 3 {
        return Ok(());
    }
    let size = ((attr1 >> 14) & 0x3) as usize;
    let Some((width, height)) = obj_size(shape, size) else {
        return Ok(());
    };
    let source_origin_y = (attr0 & 0x00FF) as i32;
    let mut origin_y = source_origin_y;
    let mut origin_x = (attr1 & 0x01FF) as i32;
    if origin_x >= 256 {
        origin_x -= 512;
    } else if origin_x >= 240 && !double_size {
        return Ok(());
    }
    let dimensions = ObjDimensions {
        source_width: width,
        source_height: height,
        canvas_width: width * if double_size { 2 } else { 1 },
        canvas_height: height * if double_size { 2 } else { 1 },
    };
    if double_size {
        // Design policy: preserve the source upper-left reference and center
        // the expanded clipping canvas on the original source center.
        origin_x -= (dimensions.canvas_width - width) as i32 / 2;
        origin_y -= (dimensions.canvas_height - height) as i32 / 2;
    }
    if source_origin_y >= 160 {
        return Ok(());
    }
    let priority = ((attr2 >> 10) & 0x3) as u8;
    let tile_base = (attr2 & 0x03FF) as usize;
    let palette_bank = ((attr2 >> 12) & 0xF) as usize;
    let (obj_tile_base, obj_tile_end) = if dispcnt & 0x7 >= 3 {
        (BITMAP_OBJ_TILE_BASE, BITMAP_OBJ_TILE_END)
    } else {
        (OBJ_TILE_BASE, VRAM_LEN)
    };
    let (matrix, flip_x, flip_y) = if affine {
        (Some(read_affine(bus, attr1)?), false, false)
    } else {
        (None, attr1 & 0x1000 != 0, attr1 & 0x2000 != 0)
    };
    let mut affine_mosaic_samples = if mosaic_enabled {
        matrix.map(|matrix| {
            let first_group_x = origin_x.div_euclid(mosaic_width) * mosaic_width;
            let first_group_y = origin_y.div_euclid(mosaic_height) * mosaic_height;
            let last_group_x = (origin_x + dimensions.canvas_width as i32 - 1)
                .div_euclid(mosaic_width)
                * mosaic_width;
            let last_group_y = (origin_y + dimensions.canvas_height as i32 - 1)
                .div_euclid(mosaic_height)
                * mosaic_height;
            let columns = ((last_group_x - first_group_x) / mosaic_width + 1) as usize;
            let rows = ((last_group_y - first_group_y) / mosaic_height + 1) as usize;
            let sampler = AffineMosaicSampler {
                dimensions,
                origin_x,
                origin_y,
                mosaic_width,
                mosaic_height,
                matrix,
            };
            (
                first_group_x,
                first_group_y,
                columns,
                sampler,
                vec![None; columns * rows],
            )
        })
    } else {
        None
    };
    for dy in 0..dimensions.canvas_height as i32 {
        let screen_y = origin_y + dy;
        if !(0..HEIGHT as i32).contains(&screen_y) {
            continue;
        }
        for dx in 0..dimensions.canvas_width as i32 {
            let screen_x = origin_x + dx;
            if !(0..WIDTH as i32).contains(&screen_x) {
                continue;
            }
            let source = if !mosaic_enabled {
                source_pixel(dimensions, dx, dy, flip_x, flip_y, matrix)
            } else if let Some((first_group_x, first_group_y, columns, sampler, samples)) =
                &mut affine_mosaic_samples
            {
                let group_x = screen_x - screen_x % mosaic_width;
                let group_y = screen_y - screen_y % mosaic_height;
                let column = ((group_x - *first_group_x) / mosaic_width) as usize;
                let row = ((group_y - *first_group_y) / mosaic_height) as usize;
                let sample_index = row * *columns + column;
                if samples[sample_index].is_none() {
                    samples[sample_index] = Some(sampler.source_sample(group_x, group_y));
                }
                samples[sample_index].unwrap_or(None)
            } else {
                // Groups align to the screen; partial top/left groups begin at
                // the first coordinate covered by this normal OBJ.
                let sample_x = (screen_x - screen_x % mosaic_width).max(origin_x);
                let sample_y = (screen_y - screen_y % mosaic_height).max(origin_y);
                source_pixel(
                    dimensions,
                    sample_x - origin_x,
                    sample_y - origin_y,
                    flip_x,
                    flip_y,
                    matrix,
                )
            };
            let Some((source_x, source_y)) = source else {
                continue;
            };
            let tile_step = if color_8bpp { 2 } else { 1 };
            let source_tile_x = source_x as usize / 8;
            let source_tile_y = source_y as usize / 8;
            let tile = if dispcnt & 0x0040 != 0 {
                tile_base + tile_step * (source_tile_y * (width / 8) + source_tile_x)
            } else {
                tile_base + source_tile_y * 32 + tile_step * source_tile_x
            };
            let pixel_x = source_x as usize % 8;
            let pixel_y = source_y as usize % 8;
            let Some(color_index) = read_tile_pixel(
                bus,
                tile,
                pixel_x,
                pixel_y,
                color_8bpp,
                obj_tile_base,
                obj_tile_end,
            )?
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
                semi_transparent: mode == 1,
                order: 0,
            };
            choose(&mut layer.pixels[pixel_index], Some(candidate));
        }
    }
    Ok(())
}

fn source_pixel(
    dimensions: ObjDimensions,
    dx: i32,
    dy: i32,
    flip_x: bool,
    flip_y: bool,
    matrix: Option<[i16; 4]>,
) -> Option<(i32, i32)> {
    if let Some([pa, pb, pc, pd]) = matrix {
        let center_x = dimensions.source_width as i32 / 2;
        let center_y = dimensions.source_height as i32 / 2;
        let rel_x = dx - dimensions.canvas_width as i32 / 2;
        let rel_y = dy - dimensions.canvas_height as i32 / 2;
        let source_x = center_x + ((i32::from(pa) * rel_x + i32::from(pb) * rel_y) >> 8);
        let source_y = center_y + ((i32::from(pc) * rel_x + i32::from(pd) * rel_y) >> 8);
        if (0..dimensions.source_width as i32).contains(&source_x)
            && (0..dimensions.source_height as i32).contains(&source_y)
        {
            Some((source_x, source_y))
        } else {
            None
        }
    } else {
        let source_x = if flip_x {
            dimensions.source_width as i32 - 1 - dx
        } else {
            dx
        };
        let source_y = if flip_y {
            dimensions.source_height as i32 - 1 - dy
        } else {
            dy
        };
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
    obj_tile_base: u32,
    obj_tile_end: u32,
) -> Result<Option<usize>> {
    // The OAM tile number is a 32-byte unit for both depths;
    // 8bpp tiles occupy two consecutive units (GBATEK's 04h/06h example).
    let offset = if color_8bpp {
        obj_tile_base as usize + tile * 32 + y * 8 + x
    } else {
        obj_tile_base as usize + tile * 32 + y * 4 + x / 2
    };
    if offset >= obj_tile_end as usize {
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
