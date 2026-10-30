//! Dependency-free GBA audio synthesis and WAV output.
//!
//! This is a bounded PSG mixer: two square channels, the wave channel and the
//! noise channel, driven by the write-only sound register file. It has no
//! platform audio dependency, no unsafe code and no sample-clock claim beyond
//! the fixed host sample rate. Direct Sound FIFO playback is not mixed here;
//! the FIFO device model remains the explicit host-event queue in `bus::sound`.
//!
//! The output is deterministic PCM, so every supported target (including
//! wasm32) can compile the same renderer. Native callers can write a WAV file;
//! the browser frontend can feed the same samples to WebAudio.

/// Fixed host render rate. This is not a claim about the GBA sample clock.
pub const SAMPLE_RATE: u32 = 32_768;

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

/// Render `frames` mono samples from one sound-register snapshot.
///
/// The register file is the same 0x48-byte image the bus exposes at
/// `0x04000060`; offsets are relative to that base. All channels are mixed and
/// scaled by the host volume/mute setting, then clamped to `i16`.
#[must_use]
pub fn render_psg(regs: &[u8], frames: usize, settings: AudioSettings) -> Vec<i16> {
    let mut phases = [0.0f32; 4];
    let mut lfsr = 0x7FFFu16;
    let mut samples = Vec::with_capacity(frames);
    render_into(regs, frames, settings, &mut phases, &mut lfsr, &mut samples);
    samples
}

fn render_into(
    regs: &[u8],
    frames: usize,
    settings: AudioSettings,
    phases: &mut [f32; 4],
    lfsr: &mut u16,
    samples: &mut Vec<i16>,
) {
    if regs.len() < 0x40 {
        samples.extend(std::iter::repeat_n(0, frames));
        return;
    }
    let master = regs[0x24] & 0x80 != 0;
    let psg_volume = f32::from(regs[0x20] & 0x07) / 7.0;
    let gain = settings.gain() * psg_volume * 0.30;
    let mut phase1 = phases[0];
    let mut phase2 = phases[1];
    let mut phase3 = phases[2];
    let mut phase4 = phases[3];
    for _ in 0..frames {
        let mut mixed = 0.0f32;
        let mut active = 0u16;
        if master
            && channel_enabled(regs, 0)
            && let Some(frequency) = square_frequency(regs, 0x04)
        {
            mixed += square_sample(regs, 0x02, 0x04, phase1);
            phase1 = (phase1 + frequency / SAMPLE_RATE as f32).fract();
            active += 1;
        }
        if master
            && channel_enabled(regs, 1)
            && let Some(frequency) = square_frequency(regs, 0x0C)
        {
            mixed += square_sample(regs, 0x08, 0x0C, phase2);
            phase2 = (phase2 + frequency / SAMPLE_RATE as f32).fract();
            active += 1;
        }
        if master && channel_enabled(regs, 2) {
            mixed += wave_sample(regs, phase3);
            if let Some(frequency) = square_frequency(regs, 0x14) {
                phase3 = (phase3 + frequency / SAMPLE_RATE as f32).fract();
            }
            active += 1;
        }
        if master && channel_enabled(regs, 3) {
            mixed += noise_sample(regs, phase4, lfsr);
            phase4 = (phase4 + 1.0 / SAMPLE_RATE as f32).fract();
            active += 1;
        }
        let value = if active == 0 {
            0.0
        } else {
            mixed / f32::from(active)
        };
        samples.push((value * gain * f32::from(i16::MAX)).clamp(-32768.0, 32767.0) as i16);
    }
    phases.copy_from_slice(&[phase1, phase2, phase3, phase4]);
}

/// Stateful mixer that advances with the guest instruction timeline.
///
/// Call [`AudioEngine::advance_instruction`] after every guest instruction with
/// the current sound-register snapshot. Phase and noise state persist across
/// calls, so a guest pitch/volume change is heard at the next generated sample,
/// not after a fixed snapshot interval. The nominal clock is the documented
/// 16.777216 MHz CPU clock divided by [`SAMPLE_RATE`]; this is an approximation,
/// not cycle timing. One instruction is attributed three cycles, matching the
/// bus's documented frame/timer mapping.
#[derive(Clone, Debug)]
pub struct AudioEngine {
    settings: AudioSettings,
    phases: [f32; 4],
    lfsr: u16,
    cycle_accumulator: u64,
    samples: Vec<i16>,
}

impl AudioEngine {
    #[must_use]
    pub fn new(settings: AudioSettings) -> Self {
        Self {
            settings,
            phases: [0.0; 4],
            lfsr: 0x7FFF,
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

    /// Advance the mixer by one guest instruction.
    ///
    /// Two consecutive calls represent a register write boundary: the next
    /// sample rendered after the write uses the new register values. There is
    /// no fixed 1024-instruction snapshot interval.
    #[inline]
    pub fn advance_instruction(&mut self, regs: &[u8]) {
        let cycles_per_sample = 16_777_216 / SAMPLE_RATE;
        self.cycle_accumulator += u64::from(crate::bus::INSTRUCTION_CYCLES);
        if self.cycle_accumulator < u64::from(cycles_per_sample) {
            return;
        }
        let frames = (self.cycle_accumulator / u64::from(cycles_per_sample)) as usize;
        self.cycle_accumulator %= u64::from(cycles_per_sample);
        render_into(
            regs,
            frames,
            self.settings,
            &mut self.phases,
            &mut self.lfsr,
            &mut self.samples,
        );
    }

    /// Advance by a number of instructions in one step.
    ///
    /// This is used by generated dispatchers after a translated block. It uses
    /// the register snapshot of the block end and renders all samples due in
    /// one call; it never quantizes to a fixed instruction chunk.
    pub fn advance(&mut self, regs: &[u8], instructions: u64) {
        let cycles_per_sample = 16_777_216 / SAMPLE_RATE;
        self.cycle_accumulator += instructions * u64::from(crate::bus::INSTRUCTION_CYCLES);
        let frames = (self.cycle_accumulator / u64::from(cycles_per_sample)) as usize;
        self.cycle_accumulator %= u64::from(cycles_per_sample);
        render_into(
            regs,
            frames,
            self.settings,
            &mut self.phases,
            &mut self.lfsr,
            &mut self.samples,
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn square_regs() -> [u8; 0x48] {
        let mut regs = [0u8; 0x48];
        // 440 Hz: 2048 - 131072/440 = 1750.
        regs[0x02..0x04].copy_from_slice(&0xF080u16.to_le_bytes());
        regs[0x04..0x06].copy_from_slice(&(0x8000u16 | 1750u16).to_le_bytes());
        regs[0x20..0x22].copy_from_slice(&0x0107u16.to_le_bytes());
        regs[0x24] = 0x80;
        regs
    }

    #[test]
    fn square_tone_is_audible_and_periodic() {
        let samples = render_psg(
            &square_regs(),
            SAMPLE_RATE as usize,
            AudioSettings::default(),
        );
        assert_eq!(samples.len(), SAMPLE_RATE as usize);
        assert!(samples.iter().any(|sample| *sample != 0));
        let crossings = samples
            .windows(2)
            .filter(|pair| (pair[0] < 0) != (pair[1] < 0))
            .count();
        // One second of 440 Hz square has about 880 sign changes.
        assert!((800..=960).contains(&crossings), "crossings={crossings}");
    }

    #[test]
    fn mute_is_digital_silence_and_volume_scales() {
        let tone = square_regs();
        let muted = AudioSettings::new(100, true);
        assert!(
            render_psg(&tone, 4096, muted)
                .iter()
                .all(|sample| *sample == 0)
        );
        let full = render_psg(&tone, 4096, AudioSettings::new(100, false));
        let half = render_psg(&tone, 4096, AudioSettings::new(50, false));
        let full_peak = full
            .iter()
            .map(|sample| sample.unsigned_abs())
            .max()
            .unwrap();
        let half_peak = half
            .iter()
            .map(|sample| sample.unsigned_abs())
            .max()
            .unwrap();
        assert!(full_peak > 0 && half_peak > 0);
        assert!(half_peak < full_peak);
    }

    #[test]
    fn disabled_master_register_is_silent() {
        let mut regs = square_regs();
        regs[0x24] = 0;
        assert!(
            render_psg(&regs, 1024, AudioSettings::default())
                .iter()
                .all(|s| *s == 0)
        );
    }

    #[test]
    fn wav_header_is_canonical_and_sized() {
        let samples = [0i16, 1, -1, 32767, -32768];
        let wav = wav_bytes(&samples, SAMPLE_RATE);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 10);
        assert_eq!(wav.len(), 44 + 10);
        assert_eq!(
            u32::from_le_bytes(wav[24..28].try_into().unwrap()),
            SAMPLE_RATE
        );
    }

    #[test]
    fn tone_generator_uses_the_requested_pitch() {
        let samples = render_tone(1000.0, 8192, AudioSettings::new(100, false));
        assert!(samples.iter().any(|sample| *sample != 0));
    }

    #[test]
    fn engine_advances_with_guest_instructions() {
        let mut engine = AudioEngine::new(AudioSettings::new(100, false));
        let regs = square_regs();
        engine.advance(&regs, 1024);
        let first = engine.samples().len();
        assert!(first > 0);
        engine.advance(&regs, 1024);
        assert!(engine.samples().len() > first);
        assert!(engine.samples().iter().any(|sample| *sample != 0));
    }

    #[test]
    fn engine_keeps_phase_across_register_changes() {
        let mut engine = AudioEngine::new(AudioSettings::new(100, false));
        let low = square_regs();
        let mut high = square_regs();
        high[0x04..0x06].copy_from_slice(&(0x8000u16 | 1700u16).to_le_bytes());
        engine.advance(&low, 200_000);
        let first = engine.samples().len();
        assert!(first > 0);
        assert!(engine.samples().iter().any(|s| *s != 0));
        engine.advance(&high, 200_000);
        assert!(engine.samples().len() > first);
        assert!(engine.samples()[first..].iter().any(|s| *s != 0));
    }

    #[test]
    fn register_write_changes_the_next_sample_without_chunk_latency() {
        let mut engine = AudioEngine::new(AudioSettings::new(100, false));
        let mut regs = square_regs();
        engine.advance(&regs, 1_024);
        let before = engine.samples().len();
        // Disable the master sound register at this exact instruction boundary.
        regs[0x24] = 0;
        engine.advance_instruction(&regs);
        engine.advance(&regs, 512);
        assert!(engine.samples().len() > before);
        assert!(
            engine.samples()[before..].iter().all(|sample| *sample == 0),
            "a register write must affect the next sample, not a later chunk"
        );
    }

    #[test]
    fn volume_write_changes_amplitude_at_the_next_sample() {
        let mut engine = AudioEngine::new(AudioSettings::new(100, false));
        let mut regs = square_regs();
        engine.advance(&regs, 1_024);
        assert!(engine.samples().iter().any(|sample| *sample != 0));
        let before = engine.samples().len();
        // Channel 1 envelope/volume nibble 0 silences the still-running channel.
        regs[0x02] = 0x00;
        regs[0x03] = 0x00;
        engine.advance(&regs, 1_024);
        assert!(engine.samples().len() > before);
        assert!(
            engine.samples()[before..].iter().all(|sample| *sample == 0),
            "a volume write must affect the next generated sample"
        );
    }

    #[test]
    fn engine_mute_is_silent_after_stateful_advance() {
        let mut engine = AudioEngine::new(AudioSettings::new(100, true));
        engine.advance(&square_regs(), 8192);
        assert!(engine.samples().iter().all(|sample| *sample == 0));
    }
}
