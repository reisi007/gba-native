import assert from "node:assert/strict";
import test from "node:test";

import { SAMPLE_RATE, renderToneSamples, renderToneWav, wavBytes } from "../../www/audio.mjs";

test("the tone WAV is a mono 16-bit RIFF artifact", () => {
  const wav = renderToneWav(1);
  const view = new DataView(wav);
  const text = (offset, length) =>
    String.fromCharCode(...new Uint8Array(wav, offset, length));
  assert.equal(text(0, 4), "RIFF");
  assert.equal(text(8, 4), "WAVE");
  assert.equal(view.getUint16(20, true), 1, "PCM");
  assert.equal(view.getUint16(22, true), 1, "mono");
  assert.equal(view.getUint16(34, true), 16, "16-bit");
  assert.equal(view.getUint32(24, true), SAMPLE_RATE);
  assert.equal(wav.byteLength, 44 + SAMPLE_RATE * 2);
});

test("muting produces a silent, complete artifact", () => {
  const samples = renderToneSamples(0);
  assert.equal(samples.length, SAMPLE_RATE);
  assert.ok(samples.every((sample) => sample === 0));
  const wav = wavBytes(samples);
  assert.equal(wav.byteLength, 44 + SAMPLE_RATE * 2);
});

test("an audible gain produces a non-silent artifact", () => {
  const samples = renderToneSamples(1);
  assert.ok(samples.some((sample) => sample !== 0));
  assert.ok(samples.every((sample) => sample >= -16384 && sample <= 16384));
});
