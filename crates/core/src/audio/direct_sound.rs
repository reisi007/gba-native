//! Direct Sound FIFO A/B playback.
//!
//! Each enabled channel consumes one signed 8-bit sample from its 32-byte FIFO
//! at the overflow rate of the selected timer (0 or 1), using the documented
//! `16.777216 MHz / (prescaler * (65536 - reload))` formula. A sound-timed DMA
//! burst is requested once the FIFO is at most half full; without a configured
//! channel the FIFO simply drains and the last sample is held, which matches a
//! hardware underrun rather than silently muting the channel.
//!
//! **Explicit limits.** The timer rate comes from the instruction-budget timer
//! model, so this is a host audio approximation and not cycle timing. SOUNDBIAS
//! PWM resolution/rate and stereo panning are not modeled: the renderer is mono
//! and a channel is audible when either side-enable bit is set.

use super::{AudioStats, SAMPLE_RATE, u16_at};
use crate::bus::{Bus, SoundFifo};
use crate::error::Result;

/// FIFO bytes are consumed in bursts of four words; refill at half capacity.
const REFILL_THRESHOLD: usize = 16;
/// A valid timer rate never needs more pops than the 16.777216 MHz / 32768 Hz
/// ratio; the bound keeps a malformed register configuration from looping.
const MAX_STEPS_PER_FRAME: u32 = 1024;
/// Direct Sound mix headroom, matching the PSG mix amplitude.
const DIRECT_SOUND_GAIN: f32 = 0.30;

#[derive(Clone, Copy, Debug)]
struct Channel {
    /// Fractional FIFO samples owed at the current timer rate.
    accumulator: f32,
    /// Last consumed sample, held across underruns.
    last: i8,
    has_sample: bool,
}

impl Channel {
    const fn new() -> Self {
        Self {
            accumulator: 0.0,
            last: 0,
            has_sample: false,
        }
    }

    fn mix_frame(
        &mut self,
        bus: &mut Bus,
        fifo: SoundFifo,
        timer: usize,
        volume: f32,
        gain: f32,
        stats: &mut AudioStats,
    ) -> Result<f32> {
        let Some(rate) = bus.sound_timer_rate(timer) else {
            return Ok(0.0);
        };
        if !rate.is_finite() || rate <= 0.0 {
            return Ok(0.0);
        }
        self.accumulator += rate / SAMPLE_RATE as f32;
        let mut consumed = 0u32;
        while self.accumulator >= 1.0 && consumed < MAX_STEPS_PER_FRAME {
            self.accumulator -= 1.0;
            consumed += 1;
            if let Some(sample) = bus.pop_sound_fifo_sample(fifo) {
                self.last = sample;
                self.has_sample = true;
                stats.direct_sound_samples += 1;
            } else {
                stats.direct_sound_underruns += 1;
            }
            let queued = bus.sound_fifo_len(fifo);
            if queued <= REFILL_THRESHOLD {
                bus.request_sound_fifo_dma(fifo)?;
                if bus.sound_fifo_len(fifo) > queued {
                    stats.direct_sound_dma_refills += 1;
                }
            }
        }
        if self.accumulator >= 1.0 {
            self.accumulator = self.accumulator.fract();
        }
        if !self.has_sample {
            return Ok(0.0);
        }
        Ok(f32::from(self.last) / 128.0 * volume * gain * DIRECT_SOUND_GAIN)
    }
}

/// Both Direct Sound FIFO channels and their resampling state.
#[derive(Clone, Debug)]
pub(crate) struct DirectSound {
    a: Channel,
    b: Channel,
    stats: AudioStats,
}

impl DirectSound {
    pub(crate) fn new() -> Self {
        Self {
            a: Channel::new(),
            b: Channel::new(),
            stats: AudioStats::default(),
        }
    }

    pub(crate) fn stats(&self) -> AudioStats {
        self.stats
    }

    /// Produce one host sample of the normalized Direct Sound mix.
    pub(crate) fn mix_frame(&mut self, bus: &mut Bus, regs: &[u8], gain: f32) -> Result<f32> {
        if regs.len() < 0x26 || regs[0x24] & 0x80 == 0 {
            return Ok(0.0);
        }
        let control = u16_at(regs, 0x22);
        let mut mixed = 0.0f32;
        if control & 0x0300 != 0 {
            mixed += self.a.mix_frame(
                bus,
                SoundFifo::A,
                usize::from((control >> 10) & 1),
                volume_gain(control & 0x3),
                gain,
                &mut self.stats,
            )?;
        }
        if control & 0x3000 != 0 {
            mixed += self.b.mix_frame(
                bus,
                SoundFifo::B,
                usize::from((control >> 14) & 1),
                volume_gain((control >> 2) & 0x3),
                gain,
                &mut self.stats,
            )?;
        }
        Ok(mixed)
    }
}

/// Documented SOUNDCNT_H Sound A/B volume field.
///
/// `0 = 50%`, `1 = 100%`, `2 = 200%`; the fourth encoding is documented as
/// prohibited, so it renders silent instead of guessing an amplitude.
fn volume_gain(field: u16) -> f32 {
    match field & 0x3 {
        0 => 0.5,
        1 => 1.0,
        2 => 2.0,
        _ => 0.0,
    }
}
