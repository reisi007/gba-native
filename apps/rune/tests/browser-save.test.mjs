// Exercise Rune's inline browser storage adapter with an enumerable localStorage stub.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = await readFile(new URL('../src/saves.rs', import.meta.url), 'utf8');
const inline = source.match(/inline_js = r##"([\s\S]*?)"##\)\]/)?.[1];
assert.ok(inline, 'save adapter exposes its inline browser module');

async function adapter() {
  const entries = new Map();
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
  const context = vm.createContext({
    localStorage: storage,
    document: { querySelector() { return { click() {} }; } },
    Uint8Array,
    Blob,
    URL,
  });
  const module = new vm.SourceTextModule(inline, { context });
  await module.link(() => {});
  await module.evaluate();
  return module.namespace;
}

test('browser save slots do not mistake the default marker for a save', async () => {
  const api = await adapter();
  assert.equal(api.runeSaveStore('slot-1', Uint8Array.from([0x47, 0x42])), true);
  assert.equal(api.runeSaveNames(), 'slot-1');
  assert.equal(api.runeSaveSize('slot-1'), 2);
  assert.equal(api.runeSaveIsDefault('slot-1'), true);
  assert.equal(api.runeSaveRead('slot-1'), '4742');

  assert.equal(api.runeSaveStore('slot-2', Uint8Array.from([0x53])), true);
  assert.equal(api.runeSaveSetDefault('slot-2'), true);
  assert.equal(api.runeSaveIsDefault('slot-2'), true);
  assert.equal(api.runeSaveDelete('slot-2'), true);
  assert.equal(api.runeSaveNames(), 'slot-1');
  assert.equal(api.runeSaveIsDefault('slot-1'), true);
});
