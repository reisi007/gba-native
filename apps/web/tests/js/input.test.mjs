// Unit tests for the portable browser input/settings model.
//
// The same model is used by app.js; these tests run under the pinned Node
// runtime in scripts/web.sh without a browser or a cartridge.

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  BUTTONS,
  DEFAULT_BINDINGS,
  DEFAULT_SETTINGS,
  LANDSCAPE,
  STORAGE_KEY,
  bindingLabel,
  buttonForCode,
  hitTest,
  humanKey,
  loadSettings,
  maskForButtons,
  normaliseSettings,
  parseSettings,
  saveSettings,
  serialiseSettings,
} from '../../www/input.mjs';

class MemoryStorage {
  constructor() {
    this.values = new Map();
  }
  getItem(key) {
    return this.values.has(key) ? this.values.get(key) : null;
  }
  setItem(key, value) {
    this.values.set(key, String(value));
  }
}

test('defaults cover every documented button', () => {
  assert.deepEqual(Object.keys(DEFAULT_BINDINGS), BUTTONS);
  assert.equal(new Set(BUTTONS).size, 10);
  assert.equal(buttonForCode('KeyZ', DEFAULT_BINDINGS), 'A');
  assert.equal(buttonForCode('ArrowUp', DEFAULT_BINDINGS), 'Up');
  assert.equal(buttonForCode('Enter', DEFAULT_BINDINGS), 'Start');
  assert.equal(buttonForCode('NoSuchKey', DEFAULT_BINDINGS), null);
});

test('action masks use KEYINPUT order', () => {
  assert.equal(maskForButtons(['A']), 1);
  assert.equal(maskForButtons(['B']), 2);
  assert.equal(maskForButtons(['Select']), 4);
  assert.equal(maskForButtons(['Start']), 8);
  assert.equal(maskForButtons(['A', 'B', 'Start']), 1 | 2 | 8);
  assert.equal(maskForButtons(['L']), 1 << 9);
  assert.equal(maskForButtons(['unknown']), 0);
});

test('settings round-trip and unknown values fall back', () => {
  const storage = new MemoryStorage();
  const settings = normaliseSettings({
    fullscreen: true,
    touchControls: false,
    integerScaling: false,
    masterVolume: 37,
    muted: true,
    bindings: { A: ['Digit1'] },
  });
  assert.equal(settings.masterVolume, 37);
  assert.equal(settings.muted, true);
  assert.equal(saveSettings(storage, settings), true);
  assert.equal(storage.getItem(STORAGE_KEY), serialiseSettings(settings));
  assert.deepEqual(loadSettings(storage), settings);
  assert.equal(loadSettings(storage).bindings.B.length, 2);

  assert.deepEqual(parseSettings('not json'), normaliseSettings(DEFAULT_SETTINGS));
  const repaired = normaliseSettings({ bindings: { A: [] } });
  assert.deepEqual(repaired.bindings.A, DEFAULT_BINDINGS.A);
  assert.equal(normaliseSettings({ masterVolume: 500 }).masterVolume, 100);
  assert.equal(normaliseSettings({ masterVolume: -5 }).masterVolume, 0);
});

test('labels are human readable', () => {
  assert.equal(humanKey('KeyZ'), 'Z');
  assert.equal(humanKey('Digit1'), '1');
  assert.equal(humanKey('ArrowLeft'), 'Left');
  assert.equal(bindingLabel('A', DEFAULT_BINDINGS), 'Z / J');
  assert.equal(bindingLabel('Start', DEFAULT_BINDINGS), 'Enter');
});

test('touch hit testing reaches every control and rejects the screen', () => {
  for (const [button, rect] of Object.entries(LANDSCAPE.controls)) {
    const x = rect.x + rect.width / 2;
    const y = rect.y + rect.height / 2;
    assert.equal(hitTest(x, y), button);
  }
  const screen = LANDSCAPE.screen;
  assert.equal(hitTest(screen.x + screen.width / 2, screen.y + screen.height / 2), null);
  assert.equal(hitTest(-0.1, 0.5), null);
  assert.equal(hitTest(0.5, 1.1), null);
});
