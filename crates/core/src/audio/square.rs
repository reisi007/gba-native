//! Bounded host-sampled Channels 1 and 2 square-wave generation.

use super::u16_at;

pub(super) fn square_frequency(regs: &[u8], x_offset: usize) -> Option<f32> {
    let rate = u16_at(regs, x_offset) & 0x07FF;
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

pub(super) fn square_sample(
    regs: &[u8],
    h_offset: usize,
    phase: f32,
    envelope_volume: Option<u8>,
) -> f32 {
    let control = u16_at(regs, h_offset);
    let volume = f32::from(envelope_volume.unwrap_or(((control >> 12) & 0xF) as u8)) / 15.0;
    let duty = duty_threshold((control >> 6) & 3);
    if phase < duty { volume } else { -volume }
}
