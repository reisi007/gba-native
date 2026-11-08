// Exercise Rune's inline browser storage adapter with an enumerable localStorage stub.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = await readFile(new URL('../src/saves.rs', import.meta.url), 'utf8');
const inline = source.match(/inline_js = r##"([\s\S]*?)"##\)\]/)?.[1];
assert.ok(inline, 'save adapter exposes its inline browser module');

async function adapter(seedEntries = {}, downloads = [], inspect = {}) {
  const entries = new Map();
  for (const [key, value] of Object.entries(seedEntries)) entries.set(key, value);
  let blob;
  const storage = new Proxy({
    getItem(key) { return entries.has(key) ? entries.get(key) : null; },
    setItem(key, value) { entries.set(String(key), String(value)); },
    removeItem(key) { entries.delete(key); },
  }, {
    ownKeys() { return [...entries.keys()]; },
    getOwnPropertyDescriptor(target, key) {
      if (entries.has(key)) return { configurable: true, enumerable: true, writable: true, value: entries.get(key) };
      return Reflect.getOwnPropertyDescriptor(target, key);
    },
  });
  inspect.read = (name) => entries.get(`gba-native.rune.slot.${name}`);
  const context = vm.createContext({
    localStorage: storage,
    document: {
      querySelector() { return { click() {} }; },
      createElement() { return { click() { downloads.push({ fileName: this.download, blob }); } }; },
    },
    Uint8Array,
    Blob,
    URL: { createObjectURL(value) { blob = value; return 'blob:save'; }, revokeObjectURL() {} },
    setTimeout() {},
  });
  const module = new vm.SourceTextModule(inline, { context });
  await module.link(() => {});
  await module.evaluate();
  return module.namespace;
}

function syntheticFixture(seed) {
  const bytes = new Uint8Array(512);
  bytes.set(new TextEncoder().encode('GBANSV01'));
  bytes[8] = seed;
  for (let index = 9; index < bytes.length; index += 1) {
    bytes[index] = ((index & 0xff) * 31 + seed) & 0xff;
  }
  return bytes;
}

function encode(bytes) {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, '0')).join('');
}

test('browser save slots do not mistake the default marker for a save', async () => {
  const api = await adapter();
  assert.equal(api.runeSaveStore('invalid', Uint8Array.from([0x47, 0x42])), false);
  const wrongMagic = syntheticFixture(0);
  wrongMagic[0] ^= 1;
  assert.equal(api.runeSaveStore('wrong-magic', wrongMagic), false);
  const corruptedFixture = syntheticFixture(0);
  corruptedFixture[100] ^= 1;
  assert.equal(api.runeSaveStore('corrupted', corruptedFixture), false);
  assert.equal(api.runeSaveNames(), '');

  assert.equal(api.runeSaveStore('slot-1', syntheticFixture(0)), true);
  assert.equal(api.runeSaveNames(), 'slot-1');
  assert.equal(api.runeSaveSize('slot-1'), 512);
  assert.equal(api.runeSaveIsDefault('slot-1'), true);
  assert.equal(api.runeSaveRead('slot-1')?.slice(0, 16), '4742414e53563031');

  assert.equal(api.runeSaveStore('slot-2', syntheticFixture(1)), true);
  assert.equal(api.runeSaveSetDefault('slot-2'), true);
  assert.equal(api.runeSaveIsDefault('slot-2'), true);
  assert.equal(api.runeSaveDelete('slot-2'), true);
  assert.equal(api.runeSaveNames(), 'slot-1');
  assert.equal(api.runeSaveIsDefault('slot-1'), true);
});

test('browser adapter round-trips arbitrary valid raw images and hides invalid storage', async () => {
  const raw = Uint8Array.from([0x13, 0x00, 0xff, 0x42, 0xa1, 0x55, 0x80]);
  const oversized = encode(new Uint8Array(128 * 1024 + 1));
  const downloads = [];
  const api = await adapter({
    'gba-native.rune.slot.raw-image': encode(raw),
    'gba-native.rune.slot.empty': '',
    'gba-native.rune.slot.odd': 'a',
    'gba-native.rune.slot.malformed': 'not-hex',
    'gba-native.rune.slot.oversized': oversized,
  }, downloads);

  assert.equal(api.runeSaveNames(), 'raw-image');
  assert.equal(api.runeSaveSize('raw-image'), raw.length);
  assert.equal(api.runeSaveRead('raw-image'), encode(raw));
  for (const name of ['empty', 'odd', 'malformed', 'oversized']) {
    assert.equal(api.runeSaveSize(name), 0);
    assert.equal(api.runeSaveRead(name), null);
    assert.equal(api.runeSaveSetDefault(name), false);
    assert.equal(api.runeSaveDelete(name), false);
  }

  assert.equal(api.runeSaveSetDefault('raw-image'), true);
  assert.equal(api.runeSaveIsDefault('raw-image'), true);
  const decoded = Uint8Array.from(api.runeSaveRead('raw-image').match(/../g), (byte) => Number.parseInt(byte, 16));
  assert.equal(api.runeSaveDownload('raw-image', decoded), true);
  assert.equal(downloads[0].fileName, 'raw-image.sav');
  assert.deepEqual([...new Uint8Array(await downloads[0].blob.arrayBuffer())], [...raw]);
  assert.equal(api.runeSaveDelete('raw-image'), true);
  assert.equal(api.runeSaveNames(), '');
});

test('browser creation skips a hidden corrupt slot without replacing it', async () => {
  const corrupt = 'not-a-generated-fixture';
  const storage = {};
  const api = await adapter({ 'gba-native.rune.slot.slot2': corrupt }, [], storage);

  assert.equal(api.runeSaveCreate(), true);
  assert.equal(api.runeSaveNames(), 'slot3');
  assert.equal(storage.read('slot2'), corrupt);
  assert.equal(api.runeSaveRead('slot3'), encode(syntheticFixture(1)));
});

test('browser creation keeps the free-name count and fixture-seed sequence', async () => {
  const api = await adapter();

  assert.equal(api.runeSaveCreate(), true);
  assert.equal(api.runeSaveCreate(), true);
  assert.equal(api.runeSaveNames(), 'slot1\nslot2');
  assert.equal(api.runeSaveRead('slot1'), encode(syntheticFixture(0)));
  assert.equal(api.runeSaveRead('slot2'), encode(syntheticFixture(1)));
});
