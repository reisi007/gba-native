//! Native live audio sink for the bounded guest renderer.
//!
//! The core renders mono signed 16-bit PCM at [`sample_rate`]. This module owns a
//! small shared ring buffer and a platform output stream (CoreAudio, WASAPI or
//! ALSA through `cpal`). It performs host playback only: no guest timing, mixing
//! or frame-scheduling claim is added here.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

/// Host render rate, kept in sync with the core mixer.
pub(crate) const SAMPLE_RATE: u32 = 32_768;
const CAPACITY: usize = SAMPLE_RATE as usize * 2;
#[cfg(test)]
const CHANNELS: usize = 2;

/// Handle to the output stream. The stream thread lives for the process
/// lifetime; this handle only feeds it, and an unavailable device is a silent
/// no-op rather than a failed session.
///
/// The device is opened on demand instead of when the state is built: unit
/// tests and headless runs construct the state without a game, and opening a
/// host device there (WASAPI in CI, for instance) crashes the process on exit.
pub(crate) struct AudioSink {
    buffer: Arc<Mutex<VecDeque<i16>>>,
    available: bool,
}

impl AudioSink {
    /// A sink with an idle buffer and no output device.
    pub(crate) fn idle() -> Self {
        Self {
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(CAPACITY))),
            available: false,
        }
    }

    /// Opens the host output stream unless one already runs.
    pub(crate) fn start(&mut self) {
        if self.available {
            return;
        }
        self.available = spawn(Arc::clone(&self.buffer)).is_ok();
    }

    /// Queue one rendered slice, keeping at most two seconds of backlog.
    pub(crate) fn push(&self, samples: &[i16]) {
        if !self.available || samples.is_empty() {
            return;
        }
        if let Ok(mut buffer) = self.buffer.lock() {
            for sample in samples {
                if buffer.len() >= CAPACITY {
                    buffer.pop_front();
                }
                buffer.push_back(*sample);
            }
        }
    }
}

fn spawn(buffer: Arc<Mutex<VecDeque<i16>>>) -> Result<(), String> {
    std::thread::Builder::new()
        .name("gba-native-audio".to_owned())
        .spawn(move || match run(buffer) {
            Ok(stream) => {
                // Keep the stream (and therefore the device) alive for the
                // lifetime of the process; the audio callback owns consumption.
                let _stream = stream;
                loop {
                    std::thread::park();
                }
            }
            Err(error) => eprintln!("audio output unavailable: {error}"),
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn run(buffer: Arc<Mutex<VecDeque<i16>>>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("no default output device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let sample_format = supported.sample_format();
    let device_rate = supported.sample_rate().0;
    let config = StreamConfig {
        channels: supported.channels(),
        sample_rate: supported.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };
    let step = f64::from(SAMPLE_RATE) / f64::from(device_rate.max(1));
    let err_fn = |error| eprintln!("audio stream error: {error}");
    let stream = match sample_format {
        SampleFormat::I16 => {
            let mut phase = 0.0_f64;
            device.build_output_stream(
                &config,
                move |out: &mut [i16], _| {
                    fill(
                        out,
                        config.channels as usize,
                        step,
                        &mut phase,
                        &buffer,
                        |s| s,
                    )
                },
                err_fn,
                None,
            )
        }
        SampleFormat::F32 => {
            let mut phase = 0.0_f64;
            device.build_output_stream(
                &config,
                move |out: &mut [f32], _| {
                    fill(
                        out,
                        config.channels as usize,
                        step,
                        &mut phase,
                        &buffer,
                        |s| f32::from(s) / 32_768.0,
                    )
                },
                err_fn,
                None,
            )
        }
        SampleFormat::U16 => {
            let mut phase = 0.0_f64;
            device.build_output_stream(
                &config,
                move |out: &mut [u16], _| {
                    fill(
                        out,
                        config.channels as usize,
                        step,
                        &mut phase,
                        &buffer,
                        |s| (i32::from(s) + 32_768) as u16,
                    )
                },
                err_fn,
                None,
            )
        }
        other => return Err(format!("unsupported sample format {other:?}")),
    };
    let stream = stream.map_err(|error| error.to_string())?;
    stream.play().map_err(|error| error.to_string())?;
    Ok(stream)
}

/// Fill an interleaved output buffer from the mono ring buffer.
fn fill<T>(
    out: &mut [T],
    channels: usize,
    step: f64,
    phase: &mut f64,
    buffer: &Arc<Mutex<VecDeque<i16>>>,
    convert: impl Fn(i16) -> T,
) where
    T: Copy + Default,
{
    let mut guard = match buffer.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    let mut next = |phase: &mut f64| -> T {
        let Some(&sample) = guard.front() else {
            return T::default();
        };
        *phase += step;
        while *phase >= 1.0 {
            *phase -= 1.0;
            guard.pop_front();
        }
        convert(sample)
    };
    for frame in out.chunks_mut(channels.max(1)) {
        let sample = next(phase);
        for channel in frame.iter_mut() {
            *channel = sample;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_keeps_only_the_most_recent_two_seconds() {
        let sink = AudioSink {
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            available: true,
        };
        let chunk = vec![7_i16; CAPACITY];
        sink.push(&chunk);
        sink.push(&chunk);
        assert_eq!(sink.buffer.lock().expect("buffer").len(), CAPACITY);
    }

    #[test]
    fn host_output_accepts_a_short_buffer() {
        if std::env::var_os("RUNE_AUDIO_SMOKE").is_none() {
            return;
        }
        let mut sink = AudioSink::idle();
        sink.start();
        assert!(sink.available, "no native output device");
        sink.push(&vec![0; SAMPLE_RATE as usize / 10]);
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    #[test]
    fn fill_expands_into_interleaved_channels_and_holds_silence() {
        let buffer = Arc::new(Mutex::new(VecDeque::from(vec![1000_i16, 2000])));
        let mut out = [0_i16; 6];
        let mut phase = 0.0;
        fill(&mut out, CHANNELS, 1.0, &mut phase, &buffer, |s| s);
        assert_eq!(out, [1000, 1000, 2000, 2000, 0, 0]);
    }
}
