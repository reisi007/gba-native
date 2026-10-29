//! PPU register accessors used by the static snapshot renderer.

use super::Bus;

impl Bus {
    pub(crate) fn bgcnt(&self, index: u8) -> u16 {
        self.io.bgcnt(index)
    }

    pub(crate) fn bg_scroll(&self, index: u8) -> (u16, u16) {
        self.io.bg_scroll(index)
    }

    /// BG2/BG3 affine matrix (`PA`, `PB`, `PC`, `PD`) as documented.
    pub(crate) fn bg_affine(&self, index: u8) -> [i16; 4] {
        self.io.bg_affine(index)
    }

    /// BG2/BG3 reference point (`X`, `Y`) in 8.8 fixed point.
    pub(crate) fn bg_reference(&self, index: u8) -> [u32; 2] {
        self.io.bg_reference(index)
    }

    pub(crate) fn bldalpha(&self) -> u16 {
        self.io.bldalpha()
    }

    pub(crate) fn bldcnt(&self) -> u16 {
        self.io.bldcnt()
    }

    pub(crate) fn bldy(&self) -> u16 {
        self.io.bldy()
    }

    /// Window 0 ranges and the inside/outside enable masks.
    pub(crate) fn window0(&self) -> (u16, u16, u16, u16) {
        self.io.window0()
    }
}
