import assert from "node:assert/strict";
import test from "node:test";

import {
  MAX_SAVE_BYTES,
  SaveStore,
  SYNTHETIC_SAVE_ROM,
  base64ToBytes,
  bytesToBase64,
  validSlotName,
} from "../../www/saves.mjs";

class MemoryStorage {
  constructor() {
    this.map = new Map();
  }
  getItem(key) {
    return this.map.has(key) ? this.map.get(key) : null;
  }
  setItem(key, value) {
    this.map.set(key, String(value));
  }
  removeItem(key) {
    this.map.delete(key);
  }
}

const ROM = SYNTHETIC_SAVE_ROM.sha;
const OTHER = "1".repeat(64);

test("base64 round-trips arbitrary save bytes", () => {
  const bytes = Uint8Array.from({ length: 4096 }, (_, index) => index & 0xff);
  assert.deepEqual(base64ToBytes(bytesToBase64(bytes)), bytes);
});

test("multiple slots are independent and the first becomes default", () => {
  const store = new SaveStore(new MemoryStorage());
  const first = Uint8Array.from([0x47, 0x53, 0x41, 0x56, 1]);
  const second = Uint8Array.from([0x47, 0x53, 0x41, 0x56, 2]);
  store.save(ROM, "alpha", first);
  store.save(ROM, "beta", second);
  assert.deepEqual(
    store.list(ROM).map((slot) => slot.name),
    ["alpha", "beta"],
  );
  assert.equal(store.defaultSlot(ROM), "alpha");
  assert.deepEqual(store.list(ROM)[0].bytes, first);
  assert.deepEqual(store.list(ROM)[1].bytes, second);
  store.setDefault(ROM, "beta");
  assert.equal(store.defaultSlot(ROM), "beta");
  assert.deepEqual(
    store.list(ROM).map((slot) => slot.default),
    [false, true],
  );
});

test("digests are isolated and deletion falls back to a remaining slot", () => {
  const store = new SaveStore(new MemoryStorage());
  store.save(ROM, "alpha", Uint8Array.of(1));
  store.save(ROM, "beta", Uint8Array.of(2));
  store.save(OTHER, "alpha", Uint8Array.of(3));
  assert.equal(store.list(OTHER).length, 1);
  assert.deepEqual(store.list(OTHER)[0].bytes, Uint8Array.of(3));
  assert.equal(store.remove(ROM, "alpha"), "beta");
  assert.equal(store.defaultSlot(ROM), "beta");
  assert.throws(() => store.remove(ROM, "alpha"), /unknown save slot/);
});

test("invalid names and image lengths are rejected", () => {
  const store = new SaveStore(new MemoryStorage());
  for (const name of ["", ".", "..", "../x", "a/b", "x".repeat(33)]) {
    assert.equal(validSlotName(name), false);
    assert.throws(() => store.save(ROM, name, Uint8Array.of(1)), /invalid slot name/);
  }
  assert.throws(() => store.save(ROM, "slot", new Uint8Array()), /save image/);
  assert.throws(
    () => store.save(ROM, "slot", new Uint8Array(MAX_SAVE_BYTES + 1)),
    /save image/,
  );
});

test("the document survives a new store instance", () => {
  const storage = new MemoryStorage();
  const first = new SaveStore(storage);
  first.save(ROM, "persisted", Uint8Array.of(7, 8, 9));
  const second = new SaveStore(storage);
  assert.deepEqual(second.list(ROM)[0].bytes, Uint8Array.of(7, 8, 9));
  assert.equal(second.defaultSlot(ROM), "persisted");
});

test("corrupt storage falls back to an empty library", () => {
  const storage = new MemoryStorage();
  storage.setItem("gba-native-saves-v1", "{not json");
  const store = new SaveStore(storage);
  assert.deepEqual(store.list(ROM), []);
});
