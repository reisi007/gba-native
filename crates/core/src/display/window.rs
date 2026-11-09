//! Static Window 0/1 and OBJ-window clipping for the Mode 0/1 compositor.

use crate::bus::Bus;

pub(super) struct Window {
    window0: bool,
    window1: bool,
    obj_window: bool,
    x0: u32,
    x0_end: u32,
    y0: u32,
    y0_end: u32,
    x1: u32,
    x1_end: u32,
    y1: u32,
    y1_end: u32,
    inside0: u16,
    inside1: u16,
    outside: u16,
    obj_outside: u16,
}

impl Window {
    pub(super) fn new(bus: &Bus, dispcnt: u16) -> Self {
        let window0 = dispcnt & 0x2000 != 0;
        let window1 = dispcnt & 0x4000 != 0;
        let obj_window = dispcnt & 0x8000 != 0;
        let (h0, v0, winin, winout) = bus.window0();
        let (h1, v1) = bus.window1();
        let (x01, x02) = ranges(h0, 240);
        let (y01, y02) = ranges(v0, 160);
        let (x11, x12) = ranges(h1, 240);
        let (y11, y12) = ranges(v1, 160);
        Self {
            window0,
            window1,
            obj_window,
            x0: x01,
            x0_end: x02,
            y0: y01,
            y0_end: y02,
            x1: x11,
            x1_end: x12,
            y1: y11,
            y1_end: y12,
            inside0: winin & 0x3F,
            inside1: (winin >> 8) & 0x3F,
            outside: winout & 0x3F,
            obj_outside: (winout >> 8) & 0x3F,
        }
    }

    /// `layer` is 0..3 for BG0..BG3 and 4 for OBJ. `obj_pixel` marks a
    /// non-transparent OBJ-window sprite pixel at this position.
    pub(super) fn allows(&self, x: u32, y: u32, layer: u8, obj_pixel: bool) -> bool {
        self.mask(x, y, obj_pixel) & (1 << layer) != 0
    }

    pub(super) fn color_effects_allowed(&self, x: u32, y: u32, obj_pixel: bool) -> bool {
        self.mask(x, y, obj_pixel) & 0x20 != 0
    }

    fn mask(&self, x: u32, y: u32, obj_pixel: bool) -> u16 {
        if !self.window0 && !self.window1 && !self.obj_window {
            return 0x3F;
        }
        let inside_window0 =
            self.window0 && x >= self.x0 && x < self.x0_end && y >= self.y0 && y < self.y0_end;
        let inside_window1 =
            self.window1 && x >= self.x1 && x < self.x1_end && y >= self.y1 && y < self.y1_end;
        if inside_window0 {
            self.inside0
        } else if inside_window1 {
            self.inside1
        } else if self.obj_window && obj_pixel {
            self.obj_outside
        } else {
            self.outside & 0x3F
        }
    }
}

fn ranges(register: u16, limit: u32) -> (u32, u32) {
    let start = u32::from((register >> 8) as u8);
    let mut end = u32::from(register as u8);
    if end > limit || start > end {
        end = limit;
    }
    (start, end)
}
