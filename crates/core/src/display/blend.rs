//! Static alpha and brightness blending for the Mode 0/1 compositor.
//!
//! The 8-bit channel arithmetic is a stated design approximation of the 5-bit
//! hardware channels. Alpha results clamp at 255 instead of wrapping; EVA/EVB
//! values 17..31 clamp to 16/16 as documented.

use crate::bus::Bus;
use crate::error::Result;

pub(super) struct Blend {
    mode: u8,
    first: u16,
    second: u16,
    eva: u16,
    evb: u16,
    y: u16,
}

impl Blend {
    pub(super) fn new(bus: &Bus) -> Result<Self> {
        let bldcnt = bus.bldcnt();
        let bldalpha = bus.bldalpha();
        Ok(Self {
            mode: ((bldcnt >> 6) & 0x3) as u8,
            first: bldcnt & 0x3F,
            second: (bldcnt >> 8) & 0x3F,
            eva: (bldalpha & 0x1F).min(16),
            evb: ((bldalpha >> 8) & 0x1F).min(16),
            y: (bus.bldy() & 0x1F).min(16),
        })
    }

    pub(super) fn mode(&self) -> u8 {
        self.mode
    }

    pub(super) fn is_first(&self, layer: u8) -> bool {
        self.first & (1 << layer) != 0
    }

    pub(super) fn is_second(&self, layer: u8) -> bool {
        self.second & (1 << layer) != 0
    }

    pub(super) fn alpha(&self, first: u32, second: u32) -> u32 {
        let channel = |shift: u32| {
            let first = (first >> shift) & 0xFF;
            let second = (second >> shift) & 0xFF;
            ((first * u32::from(self.eva) + second * u32::from(self.evb)) / 16).min(255)
        };
        (channel(16) << 16) | (channel(8) << 8) | channel(0)
    }

    /// Applies the first-target brightness mode to `color`.
    pub(super) fn brightness(&self, layer: u8, color: u32) -> u32 {
        if self.mode == 0 || !self.is_first(layer) {
            return color;
        }
        match self.mode {
            2 => brighten(color, u32::from(self.y)),
            3 => darken(color, u32::from(self.y)),
            _ => color,
        }
    }
}

fn brighten(color: u32, amount: u32) -> u32 {
    let channel = |shift: u32| {
        let value = (color >> shift) & 0xFF;
        (value + (255 - value) * amount / 16).min(255)
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

fn darken(color: u32, amount: u32) -> u32 {
    let channel = |shift: u32| {
        let value = (color >> shift) & 0xFF;
        value - value * amount / 16
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}
