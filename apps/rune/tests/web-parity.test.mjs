// Executable parity checks against the existing browser frontend modules.
//
// The browser save panel is plain ES modules under `apps/web/www`, so these
// tests import the real implementations instead of a copy. The Rust parity
// test (`apps/rune/src/ui/parity_tests.rs`) drives the desktop save state
// machine and Rune's shared UI; this file covers the browser side of the same
// contract:
//
// - a Rune-created `GBANSV01` fixture survives a browser store/export round
//   trip byte for byte,
// - slot creation, default selection and default reassignment on delete follow
//   the same sequence as desktop and Rune,
// - the shared settings defaults match the native contract.
//
// Accepted, documented differences are asserted explicitly so they cannot
// drift silently: the browser rejects the three JS prototype names and uses a
// JSON settings document, while the native library stores a `key=value`
// document and treats invalid input as an error instead of normalising it.

import assert from "node:assert/strict";
import test from "node:test";

import {
  DEFAULT_SETTINGS,
  BUTTONS,
  normaliseSettings,
  parseSettings,
  serialiseSettings,
  loadSettings,
  saveSettings,
} from "../../web/www/input.mjs";
import { SYNTHETIC_SAVE_ROM, SaveStore } from "../../web/www/saves.mjs";

const DIGEST = SYNTHETIC_SAVE_ROM.sha;

function storage() {
  const values = new Map();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, String(value)),
    removeItem: (key) => values.delete(key),
  };
}

// The exact fixture `SaveSlots::synthetic_image(seed)` produces in
// `crates/core/src/frontend/saves.rs`: `GBANSV01`, the seed byte, then
// `(index as u8) * 31 + seed` for every following byte.
function runeFixture(seed) {
  const bytes = new Uint8Array(512);
  bytes.set(new TextEncoder().encode("GBANSV01"));
  bytes[8] = seed;
  for (let index = 9; index < bytes.length; index += 1) {
    bytes[index] = ((index & 0xff) * 31 + seed) & 0xff;
  }
  return bytes;
}

function slots(store) {
  return store.list(DIGEST).map((slot) => [slot.name, slot.default]);
}

test("browser store round-trips the exact Rune fixture bytes", () => {
  const store = new SaveStore(storage());
  const fixture = runeFixture(0);
  const saved = store.save(DIGEST, "slot1", fixture);
  assert.equal(saved.default, true);
  assert.equal(store.defaultSlot(DIGEST), "slot1");

  const [slot] = store.list(DIGEST);
  assert.equal(slot.name, "slot1");
  assert.equal(slot.bytes.length, 512);
  assert.equal(new TextDecoder().decode(slot.bytes.subarray(0, 8)), "GBANSV01");
  assert.deepEqual([...slot.bytes], [...fixture]);
});

test("browser default transitions match the desktop and Rune sequence", () => {
  const store = new SaveStore(storage());
  store.save(DIGEST, "slot1", runeFixture(0));
  store.save(DIGEST, "slot2", runeFixture(1));
  // The first slot stays default when a second one is added.
  assert.deepEqual(slots(store), [
    ["slot1", true],
    ["slot2", false],
  ]);

  store.setDefault(DIGEST, "slot2");
  assert.deepEqual(slots(store), [
    ["slot1", false],
    ["slot2", true],
  ]);

  // Deleting the default reassigns it to the first remaining name, like the
  // native `SaveLibrary::delete` path used by desktop and Rune.
  assert.equal(store.remove(DIGEST, "slot2"), "slot1");
  assert.deepEqual(slots(store), [["slot1", true]]);
});

test("browser accepts the generated slot names and rejects only its own guard names", () => {
  const store = new SaveStore(storage());
  for (const name of ["slot1", "slot2", "imported", "a.b_c-d"]) {
    assert.doesNotThrow(() => store.save(DIGEST, name, runeFixture(0)), name);
  }
  // Browser-only storage guard: these names would touch the JS prototype chain.
  // The native filesystem library accepts them, which is why this is recorded
  // as an accepted difference instead of silently assumed equal.
  for (const name of ["__proto__", "prototype", "constructor"]) {
    assert.throws(() => store.save(DIGEST, name, runeFixture(0)), /invalid slot name/);
  }
});

test("browser settings defaults and round trip match the shared contract", () => {
  assert.equal(DEFAULT_SETTINGS.schema, 1);
  assert.equal(DEFAULT_SETTINGS.fullscreen, false);
  assert.equal(DEFAULT_SETTINGS.touchControls, true);
  assert.equal(DEFAULT_SETTINGS.integerScaling, true);
  assert.equal(DEFAULT_SETTINGS.masterVolume, 80);
  assert.equal(DEFAULT_SETTINGS.muted, false);
  assert.deepEqual(Object.keys(DEFAULT_SETTINGS.bindings).sort(), [...BUTTONS].sort());

  const normalised = normaliseSettings(DEFAULT_SETTINGS);
  assert.deepEqual(normalised, DEFAULT_SETTINGS);
  assert.deepEqual(parseSettings("not a settings document"), DEFAULT_SETTINGS);
  assert.deepEqual(
    normaliseSettings(JSON.parse(serialiseSettings(DEFAULT_SETTINGS))),
    DEFAULT_SETTINGS,
  );
  assert.equal(normaliseSettings({ touchControls: false }).touchControls, false);
  assert.equal(normaliseSettings({}).touchControls, true);
});

test("browser settings transitions persist the same shared values as Rune and desktop", () => {
  const store = storage();
  const current = loadSettings(store);
  const transitioned = normaliseSettings({
    ...current,
    touchControls: false,
    integerScaling: false,
    muted: true,
    masterVolume: current.masterVolume + 10,
    bindings: {
      ...current.bindings,
      A: ["KeyC"],
    },
  });

  assert.equal(saveSettings(store, transitioned), true);
  const restored = loadSettings(store);
  assert.deepEqual(restored, transitioned);
  assert.deepEqual(
    {
      touchControls: restored.touchControls,
      integerScaling: restored.integerScaling,
      muted: restored.muted,
      masterVolume: restored.masterVolume,
      bindingA: restored.bindings.A,
    },
    {
      touchControls: false,
      integerScaling: false,
      muted: true,
      masterVolume: 90,
      bindingA: ["KeyC"],
    },
  );
});
