//! DMA channels 0..3: register file, addressing modes and the bounded transfer.
//!
//! There is no cycle, scanline or frame model here, so a transfer executes
//! exactly once at the moment the channel is enabled. Real silicon starts a
//! VBlank/HBlank channel later, and re-triggers a repeating channel every event;
//! this prototype records that as a stated limitation instead of pretending.
//!
//! Documented from GBATEK `#40000bah---dma0cnt_h---dma-0-control-rw` and
//! `#40000b8h---dma0cnt_l---dma-0-word-count-w-14-bit-14000h`: SAD, DAD and
//! CNT_L keep their written values, a count of zero means the channel maximum,
//! and the enable bit clears itself after a non-repeating transfer.

use super::Bus;
use crate::error::{Error, Result};

pub(super) const DMA_BASE: u32 = 0x0400_00B0;
const CHANNEL_STRIDE: u32 = 12;
const CHANNELS: usize = 4;
/// DMA0..DMA2 are 14-bit channels, DMA3 is 16-bit.
const MAX_COUNT_NARROW: u32 = 0x4000;
const MAX_COUNT_DMA3: u32 = 0x1_0000;
const ENABLE: u16 = 1 << 15;
const REPEAT: u16 = 1 << 9;
const WORD: u16 = 1 << 10;
const DEST_CONTROL: u16 = 3 << 5;
const SOURCE_CONTROL: u16 = 3 << 7;

#[derive(Clone, Copy, Default)]
pub(super) struct Channel {
    sad: u32,
    dad: u32,
    count: u16,
    control: u16,
}

#[derive(Clone, Copy, Default)]
pub(super) struct DmaState {
    channels: [Channel; CHANNELS],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Register {
    SadLow,
    SadHigh,
    DadLow,
    DadHigh,
    Count,
    Control,
}

/// Map an aligned I/O halfword address onto a DMA channel register.
pub(super) fn classify(addr: u32) -> Option<(usize, Register)> {
    let offset = addr.checked_sub(DMA_BASE)?;
    if offset >= CHANNEL_STRIDE * CHANNELS as u32 {
        return None;
    }
    let channel = (offset / CHANNEL_STRIDE) as usize;
    let register = match offset % CHANNEL_STRIDE {
        0 => Register::SadLow,
        2 => Register::SadHigh,
        4 => Register::DadLow,
        6 => Register::DadHigh,
        8 => Register::Count,
        10 => Register::Control,
        _ => return None,
    };
    Some((channel, register))
}

impl DmaState {
    pub(super) fn read16(&self, addr: u32) -> Option<u16> {
        let (channel, register) = classify(addr)?;
        let channel = &self.channels[channel];
        Some(match register {
            Register::SadLow => channel.sad as u16,
            Register::SadHigh => (channel.sad >> 16) as u16,
            Register::DadLow => channel.dad as u16,
            Register::DadHigh => (channel.dad >> 16) as u16,
            Register::Count => channel.count,
            Register::Control => channel.control,
        })
    }

    pub(super) fn write_raw(&mut self, addr: u32, value: u16) {
        let Some((channel, register)) = classify(addr) else {
            return;
        };
        let channel = &mut self.channels[channel];
        match register {
            Register::SadLow => channel.sad = (channel.sad & 0xFFFF_0000) | u32::from(value),
            Register::SadHigh => channel.sad = (channel.sad & 0xFFFF) | (u32::from(value) << 16),
            Register::DadLow => channel.dad = (channel.dad & 0xFFFF_0000) | u32::from(value),
            Register::DadHigh => channel.dad = (channel.dad & 0xFFFF) | (u32::from(value) << 16),
            Register::Count => channel.count = value,
            Register::Control => channel.control = value,
        }
    }

    pub(super) fn append_digest(&self, bytes: &mut Vec<u8>) {
        for channel in &self.channels {
            bytes.extend_from_slice(&channel.sad.to_le_bytes());
            bytes.extend_from_slice(&channel.dad.to_le_bytes());
            bytes.extend_from_slice(&channel.count.to_le_bytes());
            bytes.extend_from_slice(&channel.control.to_le_bytes());
        }
    }
}

impl Bus {
    /// Handle a 16-bit DMA register access. `None` means the address is not a
    /// DMA register and the caller falls through to the ordinary I/O map.
    pub(super) fn dma_read16(&self, addr: u32) -> Option<u16> {
        self.io.dma().read16(addr)
    }

    pub(super) fn dma_write16(&mut self, addr: u32, value: u16) -> Option<Result<()>> {
        let (channel, register) = classify(addr)?;
        let was_enabled = self.io.dma().is_enabled(channel);
        self.io.dma_mut().write_raw(addr, value);
        if register != Register::Control {
            return Some(Ok(()));
        }
        let control = self.io.dma().control(channel);
        if control & ENABLE == 0 {
            return Some(Ok(()));
        }
        if was_enabled {
            // A write while the channel is already running does not re-trigger
            // it on silicon; the repeating channel is re-armed by its event.
            return Some(Ok(()));
        }
        Some(self.transfer(channel))
    }

    fn transfer(&mut self, channel: usize) -> Result<()> {
        let state = self.io.dma().channels[channel];
        let control = state.control;
        let width = if control & WORD != 0 { 4 } else { 2 };
        let units = match u32::from(state.count) {
            0 if channel == 3 => MAX_COUNT_DMA3,
            0 => MAX_COUNT_NARROW,
            count => count,
        };
        let source_step = step((control & SOURCE_CONTROL) >> 7, true)?;
        let dest_step = step((control & DEST_CONTROL) >> 5, false)?;
        let mut source = state.sad;
        let mut destination = state.dad;
        for _ in 0..units {
            if width == 4 {
                let value = self.read32(source)?;
                self.write32(destination, value)?;
            } else {
                let value = self.read16(source)?;
                self.write16(destination, value)?;
            }
            // The step is one data unit, not one byte: a 32-bit transfer
            // advances by four bytes per unit.
            source = source.wrapping_add_signed(source_step * width);
            destination = destination.wrapping_add_signed(dest_step * width);
        }
        if control & REPEAT == 0 {
            self.io.dma_mut().clear_enable(channel);
        }
        Ok(())
    }
}

/// 0 = increment, 1 = decrement, 2 = fixed, 3 = destination reload or a
/// prohibited source setting.
fn step(bits: u16, source: bool) -> Result<i32> {
    Ok(match bits {
        0 => 1,
        1 => -1,
        2 => 0,
        _ if source => {
            return Err(Error::UnsupportedDma {
                detail: "source address control 3 is prohibited",
            });
        }
        // Without a repeat frame the reload variant degrades to increment.
        _ => 1,
    })
}

impl DmaState {
    pub(super) fn is_enabled(&self, channel: usize) -> bool {
        self.channels[channel].control & ENABLE != 0
    }

    pub(super) fn control(&self, channel: usize) -> u16 {
        self.channels[channel].control
    }

    pub(super) fn clear_enable(&mut self, channel: usize) {
        self.channels[channel].control &= !ENABLE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{EWRAM_BASE, IWRAM_BASE};
    use crate::cart::Cartridge;

    const DMA3_SAD: u32 = DMA_BASE + 3 * CHANNEL_STRIDE;
    const DMA3_CNT_L: u32 = DMA3_SAD + 8;
    const DMA3_CNT_H: u32 = DMA3_SAD + 10;
    const ENABLE_WORD_IMMEDIATE: u16 = 0x8400;

    fn bus() -> Bus {
        Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"))
    }

    #[test]
    fn word_copy_advances_by_four_bytes_per_unit() {
        let mut bus = bus();
        bus.write32(EWRAM_BASE, 0x1122_3344).expect("seed 0");
        bus.write32(EWRAM_BASE + 4, 0x5566_7788).expect("seed 1");
        bus.write32(DMA3_SAD, EWRAM_BASE).expect("sad");
        bus.write32(DMA3_SAD + 4, IWRAM_BASE).expect("dad");
        bus.write16(DMA3_CNT_L, 2).expect("count");
        bus.write16(DMA3_CNT_H, ENABLE_WORD_IMMEDIATE)
            .expect("start");
        assert_eq!(bus.read32(IWRAM_BASE).expect("dst 0"), 0x1122_3344);
        assert_eq!(bus.read32(IWRAM_BASE + 4).expect("dst 1"), 0x5566_7788);
        assert_eq!(bus.read16(DMA3_CNT_H).expect("control") & 0x8000, 0);
    }

    #[test]
    fn fixed_source_fills_the_destination() {
        let mut bus = bus();
        bus.write16(EWRAM_BASE, 0xABCD).expect("seed");
        bus.write32(DMA3_SAD, EWRAM_BASE).expect("sad");
        bus.write32(DMA3_SAD + 4, IWRAM_BASE).expect("dad");
        bus.write16(DMA3_CNT_L, 3).expect("count");
        // Enable, halfword units, source fixed (bits 7-8 = 2).
        bus.write16(DMA3_CNT_H, 0x8100).expect("start");
        assert_eq!(bus.read16(IWRAM_BASE).expect("dst"), 0xABCD);
        assert_eq!(bus.read16(IWRAM_BASE + 2).expect("dst"), 0xABCD);
        assert_eq!(bus.read16(IWRAM_BASE + 4).expect("dst"), 0xABCD);
    }

    #[test]
    fn count_register_keeps_the_written_value() {
        let mut bus = bus();
        bus.write16(DMA3_CNT_L, 0x002A).expect("count");
        assert_eq!(bus.read16(DMA3_CNT_L).expect("read back"), 0x002A);
    }

    #[test]
    fn repeat_transfer_keeps_the_channel_enabled() {
        let mut bus = bus();
        bus.write32(DMA3_SAD, EWRAM_BASE).expect("sad");
        bus.write32(DMA3_SAD + 4, IWRAM_BASE).expect("dad");
        bus.write16(DMA3_CNT_L, 1).expect("count");
        // Enable, word units, repeat (bit 9).
        bus.write16(DMA3_CNT_H, 0x8600).expect("start");
        assert_ne!(bus.read16(DMA3_CNT_H).expect("control") & 0x8000, 0);
    }
}
