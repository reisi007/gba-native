//! Dependency-free GBA audio synthesis and WAV output.
//!
//! A bounded PSG mixer (two square channels, the wave channel and the noise
//! channel) plus Direct Sound FIFO A/B playback, both driven by the write-only
//! GBA sound register file. There is no platform audio dependency and no unsafe
//! code. The output is deterministic PCM, so every supported target (including
//! wasm32) compiles the same renderer. Native callers can write a WAV file; a
//! browser frontend can feed the same samples to WebAudio.
//!
//! **Explicit limits.** [`SAMPLE_RATE`] is a fixed host render rate, not a GBA
//! sample-clock claim. Direct Sound consumes one signed 8-bit FIFO byte per
//! timer-0/1 overflow using the documented
//! `16.777216 MHz / (prescaler * (65536 - reload))` rate, but the timer model
//! itself advances on an instruction budget, not hardware cycles. A FIFO
//! underrun holds the last sample instead of inventing silence.

mod direct_sound;

#[cfg(test)]
mod tests;

use crate::bus::{Bus, INSTRUCTION_CYCLES};
use crate::error::Result;
use direct_sound::DirectSound;

/// Fixed host render rate. This is not a claim about the GBA sample clock.
pub const SAMPLE_RATE: u32 = 32_768;

const CYCLES_PER_SAMPLE: u32 = 16_777_216 / SAMPLE_RATE;

/// Host-side volume and mute state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioSettings {
    /// 0..=100 percent.
    pub master_volume: u8,
    pub muted: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master_volume: 80,
            muted: false,
        }
    }
}

impl AudioSettings {
    #[must_use]
    pub fn new(master_volume: u8, muted: bool) -> Self {
        Self {
            master_volume: master_volume.min(100),
            muted,
        }
    }

    #[must_use]
    pub fn gain(self) -> f32 {
        if self.muted {
            0.0
        } else {
            f32::from(self.master_volume.min(100)) / 100.0
        }
    }

    pub fn set_volume(&mut self, volume: u8) {
        self.master_volume = volume.min(100);
    }
}

fn u16_at(regs: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([regs[offset], regs[offset + 1]])
}

fn channel_enabled(regs: &[u8], index: usize) -> bool {
    let control = u16_at(regs, 0x20);
    control & (1 << (8 + index)) != 0 || control & (1 << (12 + index)) != 0
}

fn square_frequency(regs: &[u8], x_offset: usize) -> Option<f32> {
    let raw = u16_at(regs, x_offset);
    if raw & 0x8000 == 0 {
        return None;
    }
    let rate = raw & 0x07FF;
    if rate >= 2048 {
        return None;
    }
    Some(131_072.0 / f64::from(2048 - rate) as f32)
}

fn duty_threshold(duty: u16) -> f32 {
    match duty & 3 {
        0 => 0.125,
        1 => 0.25,
        2 => 0.5,
        _ => 0.75,
    }
}

fn square_sample(regs: &[u8], h_offset: usize, x_offset: usize, phase: f32) -> f32 {
    if square_frequency(regs, x_offset).is_none() {
        return 0.0;
    }
    let control = u16_at(regs, h_offset);
    let volume = f32::from((control >> 12) & 0xF) / 15.0;
    let duty = duty_threshold((control >> 6) & 3);
    if phase < duty { volume } else { -volume }
}

fn wave_sample(regs: &[u8], phase: f32) -> f32 {
    let enable = u16_at(regs, 0x10);
    if enable & 0x0080 == 0 {
        return 0.0;
    }
    if square_frequency(regs, 0x14).is_none() {
        return 0.0;
    }
    let index = ((phase * 32.0) as usize) & 31;
    let byte = regs[0x30 + index / 2];
    let nibble = if index.is_multiple_of(2) {
        byte >> 4
    } else {
        byte & 0x0F
    };
    let volume_shift = (u16_at(regs, 0x12) >> 13) & 3;
    let sample = (f32::from(nibble) / 15.0) * 2.0 - 1.0;
    sample / f32::from(1u16 << volume_shift)
}

fn noise_sample(regs: &[u8], phase: f32, lfsr: &mut u16) -> f32 {
    let control = u16_at(regs, 0x18);
    let volume = f32::from((control >> 12) & 0xF) / 15.0;
    let raw = u16_at(regs, 0x1C);
    if raw & 0x8000 == 0 {
        return 0.0;
    }
    let divisor = u32::from(raw & 0x07FF).max(1);
    let steps = (phase * (524_288.0 / divisor as f32)) as u32;
    if steps.is_multiple_of(2) {
        let bit = (*lfsr ^ (*lfsr >> 1)) & 1;
        *lfsr = (*lfsr >> 1) | (bit << 14);
    }
    if *lfsr & 1 == 0 { volume } else { -volume }
}

/// Phase and noise state of the four PSG channels.
#[derive(Clone, Debug)]
struct PsgMixer {
    phases: [f32; 4],
    lfsr: u16,
}

impl PsgMixer {
    fn new() -> Self {
        Self {
            phases: [0.0; 4],
            lfsr: 0x7FFF,
        }
    }

    /// One host sample of the normalized PSG mix plus the active-channel count.
    fn frame(&mut self, regs: &[u8]) -> (f32, u16) {
        if regs.len() < 0x40 || regs[0x24] & 0x80 == 0 {
            return (0.0, 0);
        }
        let mut mixed = 0.0f32;
        let mut active = 0u16;
        if channel_enabled(regs, 0)
            && let Some(frequency) = square_frequency(regs, 0x04)
        {
            mixed += square_sample(regs, 0x02, 0x04, self.phases[0]);
            self.phases[0] = (self.phases[0] + frequency / SAMPLE_RATE as f32).fract();
            active += 1;
        }
        if channel_enabled(regs, 1)
            && let Some(frequency) = square_frequency(regs, 0x0C)
        {
            mixed += square_sample(regs, 0x08, 0x0C, self.phases[1]);
            self.phases[1] = (self.phases[1] + frequency / SAMPLE_RATE as f32).fract();
            active += 1;
        }
        if channel_enabled(regs, 2) {
            mixed += wave_sample(regs, self.phases[2]);
            if let Some(frequency) = square_frequency(regs, 0x14) {
                self.phases[2] = (self.phases[2] + frequency / SAMPLE_RATE as f32).fract();
            }
            active += 1;
        }
        if channel_enabled(regs, 3) {
            mixed += noise_sample(regs, self.phases[3], &mut self.lfsr);
            self.phases[3] = (self.phases[3] + 1.0 / SAMPLE_RATE as f32).fract();
            active += 1;
        }
        (mixed, active)
    }

    fn render_into(
        &mut self,
        regs: &[u8],
        frames: usize,
        settings: AudioSettings,
        samples: &mut Vec<i16>,
    ) {
        if regs.len() < 0x40 {
            samples.extend(std::iter::repeat_n(0, frames));
            return;
        }
        let psg_volume = f32::from(regs[0x20] & 0x07) / 7.0;
        let gain = settings.gain() * psg_volume * 0.30;
        for _ in 0..frames {
            let (mixed, active) = self.frame(regs);
            let value = if active == 0 {
                0.0
            } else {
                mixed / f32::from(active)
            };
            samples.push((value * gain * f32::from(i16::MAX)).clamp(-32768.0, 32767.0) as i16);
        }
    }
}

/// Host-side counters for the Direct Sound path.
///
/// These are diagnostics, not hardware timing: `direct_sound_samples` counts
/// FIFO bytes consumed, `direct_sound_underruns` counts sample slots where the
/// FIFO was empty, and `direct_sound_dma_refills` counts observed burst refills.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioStats {
    pub direct_sound_samples: u64,
    pub direct_sound_underruns: u64,
    pub direct_sound_dma_refills: u64,
}

/// Render `frames` mono samples from one sound-register snapshot.
///
/// The register file is the same 0x48-byte image the bus exposes at
/// `0x04000060`; offsets are relative to that base. All PSG channels are mixed
/// and scaled by the host volume/mute setting, then clamped to `i16`. Direct
/// Sound needs a live bus and is mixed by [`AudioEngine::advance_instruction_bus`].
#[must_use]
pub fn render_psg(regs: &[u8], frames: usize, settings: AudioSettings) -> Vec<i16> {
    let mut mixer = PsgMixer::new();
    let mut samples = Vec::with_capacity(frames);
    mixer.render_into(regs, frames, settings, &mut samples);
    samples
}

/// Stateful mixer that advances with the guest instruction timeline.
///
/// Call [`AudioEngine::advance_instruction_bus`] after every guest instruction
/// when the live bus is available: it renders PSG and Direct Sound from current
/// guest state, consumes FIFO A/B bytes at the timer-selected rate and requests
/// the half-empty sound DMA refill. [`AudioEngine::advance_instruction`] remains
/// the register-only PSG path for tests that have no bus.
///
/// Phase, LFSR, FIFO position and resampling accumulators persist across calls,
/// so a guest pitch, volume, timer or FIFO change is heard at the next generated
/// sample, not after a fixed snapshot interval. One instruction is attributed
/// three nominal cycles, matching the bus's documented frame/timer mapping.
#[derive(Clone, Debug)]
pub struct AudioEngine {
    settings: AudioSettings,
    psg: PsgMixer,
    direct_sound: DirectSound,
    cycle_accumulator: u64,
    samples: Vec<i16>,
}

impl AudioEngine {
    #[must_use]
    pub fn new(settings: AudioSettings) -> Self {
        Self {
            settings,
            psg: PsgMixer::new(),
            direct_sound: DirectSound::new(),
            cycle_accumulator: 0,
            samples: Vec::new(),
        }
    }

    pub fn set_settings(&mut self, settings: AudioSettings) {
        self.settings = settings;
    }

    #[must_use]
    pub fn settings(&self) -> AudioSettings {
        self.settings
    }

    /// Advance the register-only PSG mixer by one guest instruction.
    ///
    /// Two consecutive calls represent a register write boundary: the next
    /// sample rendered after the write uses the new register values. There is
    /// no fixed 1024-instruction snapshot interval. Direct Sound is not mixed
    /// here because it needs the bus FIFOs; use
    /// [`AudioEngine::advance_instruction_bus`] for guest audio capture.
    #[inline]
    pub fn advance_instruction(&mut self, regs: &[u8]) {
        let frames = self.due_frames(1);
        if frames > 0 {
            self.psg
                .render_into(regs, frames, self.settings, &mut self.samples);
        }
    }

    /// Advance the register-only PSG mixer by many instructions in one step.
    ///
    /// Direct Sound is not mixed here; see
    /// [`AudioEngine::advance_instruction_bus`].
    pub fn advance(&mut self, regs: &[u8], instructions: u64) {
        let frames = self.due_frames(instructions);
        if frames > 0 {
            self.psg
                .render_into(regs, frames, self.settings, &mut self.samples);
        }
    }

    /// Advance PSG and Direct Sound by one guest instruction.
    ///
    /// The live bus provides the sound register file, the timer overflow rates
    /// and the Sound A/B FIFOs. FIFO samples are consumed at the timer rate and
    /// a sound-timed DMA burst is requested once a FIFO is at most half full.
    pub fn advance_instruction_bus(&mut self, bus: &mut Bus) -> Result<()> {
        let frames = self.due_frames(1);
        self.render_bus(bus, frames)
    }

    /// Advance PSG and Direct Sound by many instructions in one step.
    ///
    /// Batch variant for callers that already hold a bus and want one register
    /// snapshot for several instructions; generated dispatchers call
    /// [`AudioEngine::advance_instruction_bus`] at every instruction boundary.
    pub fn advance_bus(&mut self, bus: &mut Bus, instructions: u64) -> Result<()> {
        let frames = self.due_frames(instructions);
        self.render_bus(bus, frames)
    }

    fn due_frames(&mut self, instructions: u64) -> usize {
        self.cycle_accumulator += instructions.saturating_mul(u64::from(INSTRUCTION_CYCLES));
        let frames = (self.cycle_accumulator / u64::from(CYCLES_PER_SAMPLE)) as usize;
        self.cycle_accumulator %= u64::from(CYCLES_PER_SAMPLE);
        frames
    }

    fn render_bus(&mut self, bus: &mut Bus, frames: usize) -> Result<()> {
        if frames == 0 {
            return Ok(());
        }
        let regs = bus.sound_registers();
        let gain = self.settings.gain();
        let psg_volume = if regs.len() >= 0x21 {
            f32::from(regs[0x20] & 0x07) / 7.0
        } else {
            0.0
        };
        let psg_gain = gain * psg_volume * 0.30;
        for _ in 0..frames {
            let (mixed, active) = self.psg.frame(&regs);
            let psg = if active == 0 {
                0.0
            } else {
                mixed / f32::from(active)
            };
            let direct = self.direct_sound.mix_frame(bus, &regs, gain)?;
            let value = psg * psg_gain + direct;
            self.samples
                .push((value * f32::from(i16::MAX)).clamp(-32768.0, 32767.0) as i16);
        }
        Ok(())
    }

    /// Direct Sound FIFO consumption counters for diagnostics.
    #[must_use]
    pub fn audio_stats(&self) -> AudioStats {
        self.direct_sound.stats()
    }

    #[must_use]
    pub fn samples(&self) -> &[i16] {
        &self.samples
    }

    pub fn take_samples(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.samples)
    }

    #[must_use]
    pub fn wav(&self) -> Vec<u8> {
        wav_bytes(&self.samples, SAMPLE_RATE)
    }
}

/// Render a fixed square test tone from a synthetic register image.
#[must_use]
pub fn render_tone(frequency: f32, frames: usize, settings: AudioSettings) -> Vec<i16> {
    let mut regs = [0u8; 0x48];
    let rate = (2048.0 - 131_072.0 / frequency.max(1.0)).round() as u16 & 0x07FF;
    regs[0x02..0x04].copy_from_slice(&0xF080u16.to_le_bytes());
    regs[0x04..0x06].copy_from_slice(&(0x8000 | rate).to_le_bytes());
    regs[0x20..0x22].copy_from_slice(&0x0107u16.to_le_bytes());
    regs[0x24] = 0x80;
    render_psg(&regs, frames, settings)
}

/// Encode mono 16-bit PCM as a canonical RIFF/WAVE file.
#[must_use]
pub fn wav_bytes(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}
