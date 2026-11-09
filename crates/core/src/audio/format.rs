use super::{AudioSettings, render_psg};

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
