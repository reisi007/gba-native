//! Static Mode 0 text backgrounds: BG0-BG3 tile maps and 4/8bpp tiles.
//!
//! The renderer reads the current VRAM/register state once. It never
//! approximates a layer: unsupported BGxCNT bits are explicit errors.

use super::layer::LayerPixel;
use super::{palette_color, read_bg8, read_bg16};
use crate::bus::Bus;
use crate::error::{Error, Result};

pub(super) struct TextBg<'a> {
    bus: &'a Bus,
    width: usize,
    height: usize,
    char_base: usize,
    screen_base: usize,
    color_8bpp: bool,
    priority: u8,
    scroll_x: u16,
    scroll_y: u16,
    order: u8,
}

pub(super) fn validate_control(bgcnt: u16) -> Result<()> {
    if bgcnt & 0x0040 != 0 {
        return Err(Error::UnsupportedBgControl {
            bgcnt,
            reason: "mosaic is not implemented",
        });
    }
    if bgcnt & 0x2030 != 0 {
        return Err(Error::UnsupportedBgControl {
            bgcnt,
            reason: "reserved BGxCNT bits must be zero in GBA mode",
        });
    }
    Ok(())
}

impl<'a> TextBg<'a> {
    pub(super) fn new(bus: &'a Bus, index: u8) -> Result<Self> {
        let bgcnt = bus.bgcnt(index);
        validate_control(bgcnt)?;
        let size = ((bgcnt >> 14) & 0x3) as usize;
        let width = if size & 1 == 0 { 256 } else { 512 };
        let height = if size & 2 == 0 { 256 } else { 512 };
        let char_base = (((bgcnt >> 2) & 0x3) as usize) * 0x4000;
        let screen_base = (((bgcnt >> 8) & 0x1F) as usize) * 0x800;
        let (scroll_x, scroll_y) = bus.bg_scroll(index);
        Ok(Self {
            bus,
            width,
            height,
            char_base,
            screen_base,
            color_8bpp: bgcnt & 0x0080 != 0,
            priority: (bgcnt & 0x3) as u8,
            scroll_x: scroll_x & 0x01FF,
            scroll_y: scroll_y & 0x01FF,
            order: 1 + index,
        })
    }

    pub(super) fn pixel(&self, x: usize, y: usize) -> Result<Option<LayerPixel>> {
        let source_x = (x + self.scroll_x as usize) % self.width;
        let source_y = (y + self.scroll_y as usize) % self.height;
        let tile_x = source_x / 8;
        let tile_y = source_y / 8;
        let screen_quadrants_wide = self.width / 256;
        let quadrant = (tile_y / 32) * screen_quadrants_wide + tile_x / 32;
        let entry_offset =
            self.screen_base + quadrant * 0x800 + ((tile_y % 32) * 32 + tile_x % 32) * 2;
        let entry = read_bg16(self.bus, entry_offset)?;
        let mut pixel_x = source_x % 8;
        let mut pixel_y = source_y % 8;
        if entry & 0x0400 != 0 {
            pixel_x = 7 - pixel_x;
        }
        if entry & 0x0800 != 0 {
            pixel_y = 7 - pixel_y;
        }
        let tile_number = (entry & 0x03FF) as usize;
        let color_index = if self.color_8bpp {
            let tile_offset = self.char_base + tile_number * 64;
            read_bg8(self.bus, tile_offset + pixel_y * 8 + pixel_x)? as usize
        } else {
            let tile_offset = self.char_base + tile_number * 32;
            let packed = read_bg8(self.bus, tile_offset + pixel_y * 4 + pixel_x / 2)?;
            let index = if pixel_x & 1 == 0 {
                packed & 0x0F
            } else {
                packed >> 4
            } as usize;
            if index == 0 {
                return Ok(None);
            }
            (((entry >> 12) & 0xF) as usize) * 16 + index
        };
        if color_index == 0 {
            return Ok(None);
        }
        Ok(Some(LayerPixel {
            color: palette_color(self.bus, color_index)?,
            priority: self.priority,
            layer: self.order - 1,
            order: self.order,
        }))
    }
}
