//! Bounded host-sampled Channel 4 noise generator.

use super::SAMPLE_RATE;

pub(super) fn noise_seed(nr43: u8) -> u16 {
    if nr43 & 0x08 != 0 { 0x0040 } else { 0x4000 }
}

pub(super) fn noise_frequency(regs: &[u8]) -> Option<f32> {
    let raw = u16::from_le_bytes([regs[0x1C], regs[0x1D]]);
    if raw & 0x8000 == 0 {
        return None;
    }
    let ratio = match raw & 0x7 {
        0 => 0.5,
        value => f32::from(value),
    };
    let shift = u32::from((raw >> 4) & 0xF);
    Some(524_288.0 / ratio / (1_u32 << (shift + 1)) as f32)
}

pub(super) fn noise_sample(
    regs: &[u8],
    phase: f32,
    lfsr: &mut u16,
    output_high: &mut bool,
    envelope_volume: Option<u8>,
) -> (f32, f32) {
    let Some(frequency) = noise_frequency(regs) else {
        return (0.0, phase);
    };
    let nr43 = regs[0x1C];
    let control = u16::from_le_bytes([regs[0x18], regs[0x19]]);
    let volume = f32::from(envelope_volume.unwrap_or(((control >> 12) & 0xF) as u8)) / 15.0;
    let advance = phase + frequency / SAMPLE_RATE as f32;
    let steps = advance.floor() as u32;
    let feedback = if nr43 & 0x08 != 0 { 0x0060 } else { 0x6000 };
    let mask = if nr43 & 0x08 != 0 { 0x007F } else { 0x7FFF };
    for _ in 0..steps {
        let carry = *lfsr & 1 != 0;
        *lfsr >>= 1;
        if carry {
            *lfsr ^= feedback;
        }
        *lfsr &= mask;
        *output_high = carry;
    }
    let sample = if *output_high { volume } else { -volume };
    (sample, advance.fract())
}
