// Browser audio artifact: mono 16-bit WAV of the reference tone.
//
// This mirrors the native `frontend::tone` contract so every target can produce
// the same artifact (WAV) in addition to live playback (WebAudio here, a
// platform player on desktop and MediaPlayer on Android).

export const SAMPLE_RATE = 32768;
export const FREQUENCY = 440;
export const SECONDS = 1;

export function renderToneSamples(gain = 1) {
  const clamped = Math.max(0, Math.min(1, gain));
  const samples = new Int16Array(SAMPLE_RATE * SECONDS);
  const amplitude = clamped * 0.5;
  for (let index = 0; index < samples.length; index += 1) {
    const value = Math.sin((2 * Math.PI * FREQUENCY * index) / SAMPLE_RATE);
    samples[index] = Math.round(value * amplitude * 32767);
  }
  return samples;
}

export function wavBytes(samples, sampleRate = SAMPLE_RATE) {
  const dataLength = samples.length * 2;
  const buffer = new ArrayBuffer(44 + dataLength);
  const view = new DataView(buffer);
  const text = (offset, value) => {
    for (let index = 0; index < value.length; index += 1) {
      view.setUint8(offset + index, value.charCodeAt(index));
    }
  };
  text(0, "RIFF");
  view.setUint32(4, 36 + dataLength, true);
  text(8, "WAVE");
  text(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  text(36, "data");
  view.setUint32(40, dataLength, true);
  for (let index = 0; index < samples.length; index += 1) {
    view.setInt16(44 + index * 2, samples[index], true);
  }
  return buffer;
}

export function renderToneWav(gain = 1) {
  return wavBytes(renderToneSamples(gain));
}
