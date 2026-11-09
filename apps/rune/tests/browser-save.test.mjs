// Exercise the inline IndexedDB adapter with a transactional in-memory test double.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = await readFile(new URL('../src/saves/browser.rs', import.meta.url), 'utf8');
const inline = source.match(/inline_js = r##"([\s\S]*?)"##\)\]/)?.[1];
assert.ok(inline, 'browser adapter exposes its inline IndexedDB module');

class FakeTransaction {
  constructor(database, mode) {
    this.database = database;
    this.mode = mode;
    this.records = mode === 'readwrite'
      ? new Map([...database.records].map(([key, value]) => [key, structuredClone(value)]))
      : database.records;
    this.pending = 0;
    this.aborted = false;
    this.finishScheduled = false;
  }

  objectStore() {
    const request = (action, write = false) => {
      const result = { result: undefined, error: null, onsuccess: null, onerror: null };
      this.pending += 1;
      queueMicrotask(() => {
        if (this.aborted) return;
        try {
          if (write && this.database.failNextWrite) {
            this.database.failNextWrite = false;
            throw new DOMException('quota exceeded', 'QuotaExceededError');
          }
          result.result = action();
          result.onsuccess?.({ target: result });
        } catch (error) {
          result.error = error;
          result.onerror?.({ target: result });
          this.abort(error);
        } finally {
          this.pending -= 1;
          this.finishSoon();
        }
      });
      return result;
    };
    return {
      get: (key) => request(() => structuredClone(this.records.get(key))),
      getAllKeys: () => request(() => [...this.records.keys()]),
      getAll: () => request(() => [...this.records.values()].map((value) => structuredClone(value))),
      put: (value) => request(() => {
        this.records.set(value.id, structuredClone(value));
        return value.id;
      }, true),
      add: (value) => request(() => {
        if (this.records.has(value.id)) throw new DOMException('key already exists', 'ConstraintError');
        this.records.set(value.id, structuredClone(value));
        return value.id;
      }, true),
      delete: (key) => request(() => this.records.delete(key), true),
    };
  }

  abort(error = new DOMException('transaction aborted', 'AbortError')) {
    if (this.aborted) return;
    this.aborted = true;
    this.error = error;
    setTimeout(() => this.onabort?.({ target: this }), 0);
  }

  finishSoon() {
    if (this.pending || this.finishScheduled || this.aborted) return;
    this.finishScheduled = true;
    setTimeout(() => {
      this.finishScheduled = false;
      if (this.pending || this.aborted) return;
      if (this.mode === 'readwrite') this.database.records = this.records;
      this.oncomplete?.({ target: this });
    }, 0);
  }
}

class FakeDatabase {
  constructor() {
    this.records = new Map();
    this.failNextWrite = false;
    this.stores = new Set();
    this.objectStoreNames = { contains: (name) => this.stores.has(name) };
  }

  createObjectStore(name) {
    this.stores.add(name);
  }

  transaction(_store, mode) {
    return new FakeTransaction(this, mode);
  }

  close() {}
}

class FakeIndexedDB {
  constructor() {
    this.databases = new Map();
  }

  open(name) {
    const request = { result: null, error: null };
    queueMicrotask(() => {
      let database = this.databases.get(name);
      if (!database) {
        database = new FakeDatabase();
        this.databases.set(name, database);
        request.result = database;
        request.onupgradeneeded?.({ target: request });
      } else {
        request.result = database;
      }
      request.onsuccess?.({ target: request });
    });
    return request;
  }
}

async function adapter(seedEntries = {}, inspect = {}, indexedDB = new FakeIndexedDB()) {
  const entries = new Map(Object.entries(seedEntries));
  const downloads = [];
  const storage = new Proxy({
    get length() { return entries.size; },
    key(index) { return [...entries.keys()][index] ?? null; },
    getItem(key) { return entries.has(key) ? entries.get(key) : null; },
    setItem(key, value) { entries.set(String(key), String(value)); },
    removeItem(key) { entries.delete(key); },
    clear() { entries.clear(); },
  }, {
    ownKeys() { return [...entries.keys()]; },
    getOwnPropertyDescriptor(target, key) {
      if (entries.has(key)) return { configurable: true, enumerable: true, writable: true, value: entries.get(key) };
      return Reflect.getOwnPropertyDescriptor(target, key);
    },
  });
  inspect.snapshotLocalStorage = () => [...entries.entries()];
  inspect.failNextWrite = () => {
    const db = indexedDB.databases.get('gba-native.rune.saves');
    assert.ok(db, 'test has opened the IndexedDB database');
    db.failNextWrite = true;
  };
  inspect.downloads = downloads;
  const context = vm.createContext({
    localStorage: storage,
    indexedDB,
    document: {
      createElement() {
        return { click() { downloads.push({ fileName: this.download, blob: inspect.blob }); } };
      },
    },
    Uint8Array,
    Blob,
    URL: {
      createObjectURL(blob) { inspect.blob = blob; return 'blob:save'; },
      revokeObjectURL() {},
    },
    setTimeout,
  });
  const module = new vm.SourceTextModule(inline, { context });
  await module.link(() => {});
  await module.evaluate();
  inspect.database = indexedDB;
  return module.namespace;
}

function syntheticFixture(seed) {
  const bytes = new Uint8Array(512);
  bytes.set(new TextEncoder().encode('GBANSV01'));
  bytes[8] = seed;
  for (let index = 9; index < bytes.length; index += 1) {
    bytes[index] = ((index * 31) + seed) & 0xff;
  }
  return bytes;
}

function encode(bytes) {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, '0')).join('');
}

async function call(api, operation, profile = 'default', name = '', value = '') {
  const id = api.runeSaveStart(operation, profile, name, value);
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const result = api.runeSaveTakeResult();
    if (result) {
      const [returnedId, succeeded, ...payload] = result.split('\t');
      assert.equal(Number(returnedId), id);
      return { succeeded: succeeded === '1', payload: payload.join('\t') };
    }
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
  throw new Error(`IndexedDB operation ${operation} did not complete`);
}

async function succeeds(api, ...args) {
  const result = await call(api, ...args);
  assert.equal(result.succeeded, true, result.payload);
  return result.payload;
}

const legacy = {
  'gba-native.rune.slot.slot-1': '1300ff42',
  'gba-native.rune.slot.slot-2': '4742414e53563031',
  'gba-native.rune.slot.invalid': 'not-hex',
  'gba-native.rune.default-slot': 'slot-1',
  'gba-native.rune.settings': 'muted=true\n',
  'gba-native.rune.save.pending': '4742',
};

test('pre-release localStorage save keys remain untouched and are not read as IndexedDB slots', async () => {
  const inspect = {};
  const api = await adapter(legacy, inspect);
  assert.equal(await succeeds(api, 'profiles'), 'default');
  assert.equal(await succeeds(api, 'slots'), '');
  assert.deepEqual(inspect.snapshotLocalStorage(), Object.entries(legacy));

  const reopened = await adapter(Object.fromEntries(inspect.snapshotLocalStorage()), {}, inspect.database);
  assert.equal(await succeeds(reopened, 'slots'), '');
});

test('profiles create and switch with isolated slots and defaults across adapter reloads', async () => {
  const inspect = {};
  const api = await adapter({
    'gba-native.rune.slot.legacy': '1300ff42',
    'gba-native.rune.default-slot': 'legacy',
  }, inspect);
  await succeeds(api, 'profiles');
  const profile = await succeeds(api, 'create-profile');
  assert.equal(profile, 'profile-1');
  await succeeds(api, 'store', profile, 'shared', encode(syntheticFixture(0)));
  await succeeds(api, 'store', 'default', 'shared', encode(syntheticFixture(1)));
  await succeeds(api, 'default', profile, 'shared');
  assert.equal(await succeeds(api, 'slots', 'default'), 'shared\t512\t1');
  assert.equal(await succeeds(api, 'slots', profile), 'shared\t512\t1');

  const reopened = await adapter({}, {}, inspect.database);
  assert.equal(await succeeds(reopened, 'profiles'), 'default\nprofile-1');
  assert.equal(await succeeds(reopened, 'slots', profile), 'shared\t512\t1');
  assert.deepEqual(inspect.snapshotLocalStorage(), Object.entries({
    'gba-native.rune.slot.legacy': '1300ff42',
    'gba-native.rune.default-slot': 'legacy',
  }));
});

test('slot create, default reassignment and deletion commit as profile transactions', async () => {
  const api = await adapter();
  await succeeds(api, 'profiles');
  assert.equal(await succeeds(api, 'create'), 'slot1');
  assert.equal(await succeeds(api, 'create'), 'slot2');
  assert.equal(await succeeds(api, 'slots'), 'slot1\t512\t1\nslot2\t512\t0');
  await succeeds(api, 'default', 'default', 'slot2');
  assert.equal(await succeeds(api, 'slots'), 'slot1\t512\t0\nslot2\t512\t1');
  await succeeds(api, 'delete', 'default', 'slot2');
  assert.equal(await succeeds(api, 'slots'), 'slot1\t512\t1');
});

test('quota failure preserves the old slot and default marker and reports an error', async () => {
  const inspect = {};
  const api = await adapter({}, inspect);
  await succeeds(api, 'profiles');
  await succeeds(api, 'store', 'default', 'slot-one', encode(syntheticFixture(0)));
  await succeeds(api, 'store', 'default', 'slot-two', encode(syntheticFixture(1)));
  inspect.failNextWrite();

  const failed = await call(api, 'default', 'default', 'slot-two');
  assert.equal(failed.succeeded, false);
  assert.match(failed.payload, /QuotaExceededError: quota exceeded/);
  assert.equal(await succeeds(api, 'slots'), 'slot-one\t512\t1\nslot-two\t512\t0');

  inspect.failNextWrite();
  const failedReplacement = await call(api, 'store', 'default', 'slot-one', encode(syntheticFixture(2)));
  assert.equal(failedReplacement.succeeded, false);
  assert.equal(await succeeds(api, 'export', 'default', 'slot-one'), 'downloaded slot-one.sav');
  const exported = inspect.downloads.at(-1);
  assert.equal(exported.fileName, 'slot-one.sav');
  assert.deepEqual([...new Uint8Array(await exported.blob.arrayBuffer())], [...syntheticFixture(0)]);
});

test('browser import consumes the staged file only after its IDB transaction commits', async () => {
  const inspect = {};
  const staged = encode(syntheticFixture(4));
  const api = await adapter({ 'gba-native.rune.save.pending': staged }, inspect);
  await succeeds(api, 'profiles');
  inspect.failNextWrite();
  const failed = await call(api, 'import');
  assert.equal(failed.succeeded, false);
  assert.equal(inspect.snapshotLocalStorage()[0][1], staged);

  await succeeds(api, 'store', 'default', 'imported', encode(syntheticFixture(8)));
  assert.equal(inspect.snapshotLocalStorage()[0][1], staged);

  assert.equal(await succeeds(api, 'import'), 'imported');
  assert.equal(await succeeds(api, 'slots'), 'imported\t512\t1');
  assert.equal(await succeeds(api, 'export', 'default', 'imported'), 'downloaded imported.sav');
  assert.deepEqual(
    [...new Uint8Array(await inspect.downloads.at(-1).blob.arrayBuffer())],
    [...syntheticFixture(4)],
  );
  assert.equal(inspect.snapshotLocalStorage().some(([key]) => key === 'gba-native.rune.save.pending'), false);
});

test('missing IndexedDB returns a surfaced persistence error', async () => {
  const api = await adapter({}, {}, null);
  const result = await call(api, 'profiles');
  assert.equal(result.succeeded, false);
  assert.match(result.payload, /IndexedDB is unavailable/);
});
