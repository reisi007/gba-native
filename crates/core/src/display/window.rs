//! Static window-0 clipping for the Mode 0/1 compositor.

use crate::bus::Bus;
use crate::error::{Error, Result};

pub(super) struct Window {
    window0: bool,
    obj_window: bool,
    x1: u32,
    x2: u32,
    y1: u32,
    y2: u32,
    inside: u16,
    outside: u16,
}

impl Window {
    pub(super) fn new(bus: &Bus, dispcnt: u16) -> Result<Self> {
        if dispcnt & 0x4000 != 0 {
            // Window 1 is not part of this static subset.
            return Err(Error::UnsupportedPpuFeature { dispcnt });
        }
        let window0 = dispcnt & 0x2000 != 0;
        let obj_window = dispcnt & 0x8000 != 0;
        let (horizontal, vertical, inside, outside) = bus.window0();
        let x1 = u32::from((horizontal >> 8) as u8);
        let mut x2 = u32::from((horizontal & 0xFF) as u8);
        if x2 > 240 || x1 > x2 {
            x2 = 240;
        }
        let y1 = u32::from((vertical >> 8) as u8);
        let mut y2 = u32::from((vertical & 0xFF) as u8);
        if y2 > 160 || y1 > y2 {
            y2 = 160;
        }
        Ok(Self {
            window0,
            obj_window,
            x1,
            x2,
            y1,
            y2,
            inside,
            outside,
        })
    }

    /// `layer` is 0..3 for BG0..BG3 and 4 for OBJ. `obj_pixel` marks a
    /// non-transparent OBJ-window sprite pixel at this position.
    pub(super) fn allows(&self, x: u32, y: u32, layer: u8, obj_pixel: bool) -> bool {
        if !self.window0 && !self.obj_window {
            return true;
        }
        let inside_window0 =
            self.window0 && x >= self.x1 && x < self.x2 && y >= self.y1 && y < self.y2;
        let mask = if inside_window0 {
            self.inside
        } else if self.obj_window && obj_pixel {
            (self.outside >> 8) & 0x3F
        } else {
            self.outside & 0x3F
        };
        mask & (1 << layer) != 0
    }
}
