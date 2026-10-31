//! Unit tests for the stateful mixer, Direct Sound FIFOs and WAV encoding.

use super::*;
use crate::bus::{Bus, EWRAM_BASE, SoundFifo};
use crate::cart::Cartridge;

fn square_regs() -> [u8; 0x48] {
    let mut regs = [0u8; 0x48];
    // 440 Hz: 2048 - 131072/440 = 1750.
    regs[0x02..0x04].copy_from_slice(&0xF080u16.to_le_bytes());
    regs[0x04..0x06].copy_from_slice(&(0x8000u16 | 1750u16).to_le_bytes());
    regs[0x20..0x22].copy_from_slice(&0x0107u16.to_le_bytes());
    regs[0x24] = 0x80;
    regs
}

/// A bus with timer 0 at 65536 Hz (reload 0xFF00, prescaler F/1).
fn direct_sound_bus() -> Bus {
    let mut bus = Bus::new(Cartridge::from_bytes(vec![0u8; 0x200]).expect("cart"));
    bus.write16(0x0400_0100, 0xFF00).expect("timer reload");
    bus.write16(0x0400_0102, 0x0080).expect("timer start");
    // SOUNDCNT_H: A volume 100%, A enabled on both sides, timer 0.
    bus.write16(0x0400_0082, 0x0301).expect("sound control");
    bus.write16(0x0400_0084, 0x0080).expect("master enable");
    bus
}

fn fill_fifo_a(bus: &mut Bus, byte: u8, words: usize) {
    let word = u32::from_le_bytes([byte; 4]);
    for _ in 0..words {
        bus.write32(crate::bus::FIFO_A_ADDR, word)
            .expect("fifo write");
    }
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

#[test]
fn direct_sound_timer_rate_follows_reload_and_prescaler() {
    let mut bus = direct_sound_bus();
    let rate = bus.sound_timer_rate(0).expect("timer 0 rate");
    assert!((rate - 65_536.0).abs() < 0.001, "rate={rate}");
    // Prescaler F/64 divides the documented overflow rate.
    bus.write16(0x0400_0102, 0x0081).expect("timer restart");
    let slow = bus.sound_timer_rate(0).expect("timer 0 rate");
    assert!((slow - 1_024.0).abs() < 0.001, "rate={slow}");
    bus.write16(0x0400_0102, 0x0000).expect("timer stop");
    assert_eq!(bus.sound_timer_rate(0), None);
}

#[test]
fn direct_sound_consumes_fifo_a_at_the_timer_rate() {
    let mut bus = direct_sound_bus();
    fill_fifo_a(&mut bus, 0x7F, 8);
    assert_eq!(bus.sound_fifo_len(SoundFifo::A), 32);
    let mut engine = AudioEngine::new(AudioSettings::new(100, false));
    engine.advance_bus(&mut bus, 2_048).expect("audio advance");
    // 2048 instructions = 6144 cycles = 12 host samples at 65536 Hz -> 24 pops.
    assert_eq!(bus.sound_fifo_len(SoundFifo::A), 8);
    assert!(engine.samples().iter().all(|sample| *sample > 0));
    assert!(engine.samples().iter().all(|sample| *sample < 12_000));
    assert_eq!(engine.audio_stats().direct_sound_samples, 24);
    assert_eq!(engine.audio_stats().direct_sound_underruns, 0);
}

#[test]
fn direct_sound_holds_the_last_sample_on_underrun() {
    let mut bus = direct_sound_bus();
    fill_fifo_a(&mut bus, 0x40, 1);
    let mut engine = AudioEngine::new(AudioSettings::new(100, false));
    engine.advance_bus(&mut bus, 4_096).expect("audio advance");
    assert_eq!(bus.sound_fifo_len(SoundFifo::A), 0);
    let samples = engine.samples();
    assert!(samples.len() > 4);
    let held = samples[samples.len() - 1];
    assert!(held > 0);
    assert_eq!(samples[4], held, "an empty FIFO holds the last sample");
    assert_eq!(engine.audio_stats().direct_sound_samples, 4);
    assert!(engine.audio_stats().direct_sound_underruns > 0);
}

#[test]
fn direct_sound_volume_and_enable_control_the_mix() {
    let samples_for = |control: u16| {
        let mut bus = direct_sound_bus();
        bus.write16(0x0400_0082, control).expect("sound control");
        fill_fifo_a(&mut bus, 0x7F, 8);
        let mut engine = AudioEngine::new(AudioSettings::new(100, false));
        engine.advance_bus(&mut bus, 1_024).expect("audio advance");
        engine.samples().to_vec()
    };
    let full = samples_for(0x0301);
    let half = samples_for(0x0300);
    let disabled = samples_for(0x0001);
    assert!(full.iter().any(|sample| *sample > 0));
    assert!(disabled.iter().all(|sample| *sample == 0));
    let full_peak = full.iter().map(|s| s.unsigned_abs()).max().unwrap();
    let half_peak = half.iter().map(|s| s.unsigned_abs()).max().unwrap();
    assert!(half_peak > 0 && half_peak < full_peak);

    // Channel B is independent: timer 1 at 16384 Hz, volume 100%, both sides.
    let mut bus = direct_sound_bus();
    bus.write16(0x0400_0104, 0xFC00).expect("timer 1 reload");
    bus.write16(0x0400_0106, 0x0080).expect("timer 1 start");
    bus.write16(0x0400_0082, 0x7004).expect("sound B control");
    for _ in 0..2 {
        bus.write32(crate::bus::FIFO_B_ADDR, 0x4040_4040)
            .expect("fifo B write");
    }
    let mut engine = AudioEngine::new(AudioSettings::new(100, false));
    engine.advance_bus(&mut bus, 8_192).expect("audio advance");
    assert!(engine.samples().iter().any(|sample| *sample > 0));
    assert_eq!(engine.audio_stats().direct_sound_samples, 8);
    assert_eq!(bus.sound_fifo_len(SoundFifo::B), 0);
}

#[test]
fn direct_sound_refills_from_sound_timed_dma() {
    let mut bus = direct_sound_bus();
    for index in 0..16u32 {
        bus.write32(EWRAM_BASE + index * 4, 0x1111_1111)
            .expect("seed dma source");
    }
    const DMA1_SAD: u32 = 0x0400_00BC;
    bus.write32(DMA1_SAD, EWRAM_BASE).expect("dma source");
    bus.write32(DMA1_SAD + 4, crate::bus::FIFO_A_ADDR)
        .expect("dma destination");
    // Enable, word units, fixed destination, repeat, special sound timing.
    bus.write16(DMA1_SAD + 10, 0xB640).expect("dma control");
    fill_fifo_a(&mut bus, 0x7F, 4);
    let mut engine = AudioEngine::new(AudioSettings::new(100, false));
    engine.advance_bus(&mut bus, 8_192).expect("audio advance");
    // Consumed the queued 0x7F samples, then the DMA-refilled 0x11 bytes.
    assert!(engine.samples().iter().any(|sample| *sample > 8_000));
    assert!(
        engine
            .samples()
            .iter()
            .any(|sample| *sample > 1_000 && *sample < 2_000),
        "the DMA burst must reach the mixer"
    );
}

#[test]
fn direct_sound_register_write_changes_the_next_sample() {
    let mut bus = direct_sound_bus();
    fill_fifo_a(&mut bus, 0x7F, 8);
    let mut engine = AudioEngine::new(AudioSettings::new(100, false));
    engine.advance_bus(&mut bus, 512).expect("audio advance");
    assert!(engine.samples().iter().any(|sample| *sample > 0));
    let before = engine.samples().len();
    // Same instruction boundary: disabling channel A silences the next frame.
    bus.write16(0x0400_0082, 0x0001).expect("disable A");
    engine.advance_bus(&mut bus, 171).expect("audio advance");
    assert!(engine.samples().len() > before);
    assert!(
        engine.samples()[before..].iter().all(|sample| *sample == 0),
        "a Sound enable write must affect the next generated sample"
    );
}
