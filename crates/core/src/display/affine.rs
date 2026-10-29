//! Static Mode 1 affine BG2/BG3 layer.

use super::layer::LayerPixel;
use super::{palette_color, read_bg8};
use crate::bus::Bus;
use crate::error::{Error, Result};

pub(super) struct AffineBg<'a> {
    bus: &'a Bus,
    size: usize,
    char_base: usize,
    screen_base: usize,
    wrap: bool,
    priority: u8,
    order: u8,
    matrix: [i16; 4],
    reference: [u32; 2],
}

impl<'a> AffineBg<'a> {
    pub(super) fn new(bus: &'a Bus, index: u8) -> Result<Self> {
        let bgcnt = bus.bgcnt(index);
        if bgcnt & 0x0040 != 0 {
            return Err(Error::UnsupportedBgControl {
                bgcnt,
                reason: "mosaic is not implemented",
            });
        }
        if bgcnt & 0x0030 != 0 || bgcnt & 0x0080 == 0 {
            return Err(Error::UnsupportedBgControl {
                bgcnt,
                reason: "affine BG requires 8bpp and clear reserved bits",
            });
        }
        let size = match (bgcnt >> 14) & 0x3 {
            0 => 128,
            1 => 256,
            2 => 512,
            _ => 1024,
        };
        Ok(Self {
            bus,
            size,
            char_base: (((bgcnt >> 2) & 0x3) as usize) * 0x4000,
            screen_base: (((bgcnt >> 8) & 0x1F) as usize) * 0x800,
            wrap: bgcnt & 0x2000 != 0,
            priority: (bgcnt & 0x3) as u8,
            order: 1 + index,
            matrix: bus.bg_affine(index - 2),
            reference: bus.bg_reference(index - 2),
        })
    }

    pub(super) fn pixel(&self, x: usize, y: usize) -> Result<Option<LayerPixel>> {
        let source_x = (i64::from(self.reference[0] as i32)
            + i64::from(self.matrix[0]) * x as i64
            + i64::from(self.matrix[1]) * y as i64)
            >> 8;
        let source_y = (i64::from(self.reference[1] as i32)
            + i64::from(self.matrix[2]) * x as i64
            + i64::from(self.matrix[3]) * y as i64)
            >> 8;
        let (source_x, source_y) = if self.wrap {
            (
                source_x.rem_euclid(self.size as i64) as usize,
                source_y.rem_euclid(self.size as i64) as usize,
            )
        } else {
            if source_x < 0
                || source_y < 0
                || source_x >= self.size as i64
                || source_y >= self.size as i64
            {
                return Ok(None);
            }
            (source_x as usize, source_y as usize)
        };
        let tiles = self.size / 8;
        let map_offset = self.screen_base + (source_y / 8) * tiles + source_x / 8;
        let tile = read_bg8(self.bus, map_offset)? as usize;
        let tile_offset = self.char_base + tile * 64 + (source_y % 8) * 8 + source_x % 8;
        let index = read_bg8(self.bus, tile_offset)? as usize;
        if index == 0 {
            return Ok(None);
        }
        Ok(Some(LayerPixel {
            color: palette_color(self.bus, index)?,
            priority: self.priority,
            layer: self.order - 1,
            order: self.order,
        }))
    }
}
